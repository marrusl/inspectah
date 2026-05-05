/**
 * Global setup for inspectah Go port e2e tests.
 *
 * 1. Always builds the Go binary (eliminates stale-binary false greens).
 * 2. Verifies requested ports are free before starting servers.
 * 3. Starts three server instances with early-exit detection.
 * 4. Health-checks each server AND verifies the child PID is still alive.
 * 5. Writes .env.test with URLs and .server-pids for teardown.
 */
import { execSync, spawn, ChildProcess } from 'child_process';
import { writeFileSync, existsSync } from 'fs';
import { join } from 'path';
import { createConnection } from 'net';

const ROOT = join(__dirname, '..', '..');
const GO_MOD_DIR = join(ROOT, 'cmd', 'inspectah');
const GO_BINARY = join(GO_MOD_DIR, 'inspectah');
const FIXTURES = join(ROOT, 'tests', 'e2e', 'fixtures');
const ENV_FILE = join(__dirname, '.env.test');
const PID_FILE = join(__dirname, '.server-pids');

interface ServerDef {
  name: string;
  envKey: string;
  args: string[];
  port: number;
}

const SERVERS: ServerDef[] = [
  {
    name: 'fleet-refine',
    envKey: 'REFINE_FLEET_URL',
    args: ['refine', join(FIXTURES, 'fleet-3host.tar.gz'), '--no-browser', '--port', '9200'],
    port: 9200,
  },
  {
    name: 'single-refine',
    envKey: 'REFINE_SINGLE_URL',
    args: ['refine', join(FIXTURES, 'single-host.tar.gz'), '--no-browser', '--port', '9201'],
    port: 9201,
  },
  {
    name: 'architect',
    envKey: 'ARCHITECT_URL',
    args: ['architect', join(FIXTURES, 'architect-topology'), '--no-browser', '--port', '9202'],
    port: 9202,
  },
];

/**
 * Always build the Go binary. `go build` is fast when nothing changed
 * (sub-second with build cache), and eliminates the entire class of
 * stale-binary bugs where changes under internal/** or embedded assets
 * don't trigger a rebuild.
 */
function buildGoBinary(): void {
  console.log('[e2e-go] Building Go binary...');
  execSync('go build -o inspectah .', {
    cwd: GO_MOD_DIR,
    stdio: 'inherit',
    timeout: 120_000,
  });
  console.log('[e2e-go] Go binary built successfully.');
}

/** Check if a TCP port is already in use. Rejects if occupied. */
function assertPortFree(port: number): Promise<void> {
  return new Promise((resolve, reject) => {
    const socket = createConnection({ port, host: '127.0.0.1' });
    socket.setTimeout(500);
    socket.on('connect', () => {
      socket.destroy();
      reject(new Error(
        `Port ${port} is already in use. Kill the stale listener before running tests. ` +
        `Try: lsof -ti :${port} | xargs kill`
      ));
    });
    socket.on('error', () => {
      socket.destroy();
      resolve(); // Connection refused = port is free
    });
    socket.on('timeout', () => {
      socket.destroy();
      resolve(); // Timeout = nothing listening
    });
  });
}

/** Verify a PID is still alive. */
function isPidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0); // Signal 0 = existence check, no actual signal
    return true;
  } catch {
    return false;
  }
}

/** Wait for a server's health endpoint to respond. */
async function waitForHealth(url: string, timeoutMs: number = 15_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const resp = await fetch(url);
      if (resp.ok) return;
    } catch {
      // Server not ready yet
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`Server at ${url} did not become healthy within ${timeoutMs}ms`);
}

export default async function globalSetup(): Promise<void> {
  // Step 1: Build Go binary (always — Go build cache makes this cheap)
  buildGoBinary();

  // Step 2: Verify all ports are free BEFORE starting any server
  for (const server of SERVERS) {
    await assertPortFree(server.port);
  }

  // Step 3: Start servers with early-exit detection
  const pids: number[] = [];
  const envLines: string[] = [];
  const earlyExitErrors: Map<string, string> = new Map();

  for (const server of SERVERS) {
    console.log(`[e2e-go] Starting ${server.name} on port ${server.port}...`);

    const proc: ChildProcess = spawn(GO_BINARY, server.args, {
      stdio: ['ignore', 'pipe', 'pipe'],
      detached: true,
    });

    // Capture stderr for diagnostics
    let stderrBuf = '';
    proc.stderr?.on('data', (data: Buffer) => {
      const line = data.toString().trim();
      stderrBuf += line + '\n';
      if (line) console.log(`[${server.name}] ${line}`);
    });

    // Watch for early exit (bind failure, crash, etc.)
    proc.on('exit', (code, signal) => {
      earlyExitErrors.set(server.name,
        `${server.name} exited early (code=${code}, signal=${signal}). stderr:\n${stderrBuf}`
      );
    });

    if (!proc.pid) {
      throw new Error(`Failed to spawn ${server.name}`);
    }

    pids.push(proc.pid);
    proc.unref();

    const url = `http://localhost:${server.port}`;
    envLines.push(`${server.envKey}=${url}`);
  }

  // Brief pause to let early-exit errors surface
  await new Promise((r) => setTimeout(r, 300));

  // Check for any early exits before health-checking
  for (const server of SERVERS) {
    const err = earlyExitErrors.get(server.name);
    if (err) {
      // Kill any servers that did start successfully
      for (const pid of pids) {
        try { process.kill(-pid, 'SIGTERM'); } catch { /* ignore */ }
      }
      throw new Error(`Server startup failed:\n${err}`);
    }
  }

  // Step 4: Write PID file for teardown
  writeFileSync(PID_FILE, pids.join('\n'), 'utf-8');

  // Step 5: Health-check all servers AND verify PIDs are still alive
  for (let i = 0; i < SERVERS.length; i++) {
    const server = SERVERS[i];
    const pid = pids[i];
    const healthUrl = `http://127.0.0.1:${server.port}/api/health`;
    console.log(`[e2e-go] Waiting for ${server.name} health at ${healthUrl}...`);

    await waitForHealth(healthUrl);

    // After health goes green, verify the child PID is still ours
    if (!isPidAlive(pid)) {
      throw new Error(
        `${server.name} health endpoint responded but PID ${pid} is dead. ` +
        `A stale server on port ${server.port} may have answered the health check.`
      );
    }
    console.log(`[e2e-go] ${server.name} is ready (pid=${pid}).`);
  }

  // Step 6: Write env file
  writeFileSync(ENV_FILE, envLines.join('\n') + '\n', 'utf-8');

  // Step 7: Set env vars for this process
  for (const server of SERVERS) {
    process.env[server.envKey] = `http://localhost:${server.port}`;
  }

  console.log('[e2e-go] All servers started and healthy.');
}
