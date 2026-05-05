/**
 * Global setup for inspectah Go port e2e tests.
 *
 * 1. Builds the Go binary (if stale or missing).
 * 2. Starts three server instances:
 *    - refine server on fleet-3host.tar.gz  (port 9200)
 *    - refine server on single-host.tar.gz  (port 9201)
 *    - architect server on architect-topology/ (port 9202)
 * 3. Health-checks each server.
 * 4. Writes .env.test with URLs and .server-pids for teardown.
 */
import { execSync, spawn, ChildProcess } from 'child_process';
import { writeFileSync, existsSync, statSync } from 'fs';
import { join } from 'path';

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

/** Build the Go binary if it doesn't exist or is older than go source files. */
function buildGoBinary(): void {
  const needsBuild = !existsSync(GO_BINARY) || isGoBinaryStale();

  if (needsBuild) {
    console.log('[e2e-go] Building Go binary...');
    execSync('go build -o inspectah .', {
      cwd: GO_MOD_DIR,
      stdio: 'inherit',
      timeout: 120_000,
    });
    console.log('[e2e-go] Go binary built successfully.');
  } else {
    console.log('[e2e-go] Go binary is up to date.');
  }
}

/** Check if any .go file is newer than the binary. */
function isGoBinaryStale(): boolean {
  try {
    const binaryMtime = statSync(GO_BINARY).mtimeMs;
    // Check a few key source files rather than walking the entire tree
    const checkFiles = [
      join(GO_MOD_DIR, 'main.go'),
      join(GO_MOD_DIR, 'go.mod'),
      join(GO_MOD_DIR, 'go.sum'),
    ];
    for (const f of checkFiles) {
      if (existsSync(f) && statSync(f).mtimeMs > binaryMtime) {
        return true;
      }
    }
    return false;
  } catch {
    return true;
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
  // Step 1: Build Go binary
  buildGoBinary();

  // Step 2: Start servers
  const pids: number[] = [];
  const envLines: string[] = [];

  for (const server of SERVERS) {
    console.log(`[e2e-go] Starting ${server.name} on port ${server.port}...`);

    const proc: ChildProcess = spawn(GO_BINARY, server.args, {
      stdio: ['ignore', 'pipe', 'pipe'],
      detached: true,
    });

    // Log stderr for debugging startup issues
    proc.stderr?.on('data', (data: Buffer) => {
      const line = data.toString().trim();
      if (line) console.log(`[${server.name}] ${line}`);
    });

    if (!proc.pid) {
      throw new Error(`Failed to start ${server.name}`);
    }

    pids.push(proc.pid);
    proc.unref();

    const url = `http://localhost:${server.port}`;
    envLines.push(`${server.envKey}=${url}`);
  }

  // Step 3: Write PID file for teardown
  writeFileSync(PID_FILE, pids.join('\n'), 'utf-8');

  // Step 4: Health-check all servers
  for (const server of SERVERS) {
    const healthUrl = `http://127.0.0.1:${server.port}/api/health`;
    console.log(`[e2e-go] Waiting for ${server.name} health at ${healthUrl}...`);
    await waitForHealth(healthUrl);
    console.log(`[e2e-go] ${server.name} is ready.`);
  }

  // Step 5: Write env file
  writeFileSync(ENV_FILE, envLines.join('\n') + '\n', 'utf-8');

  // Step 6: Set env vars for this process
  for (const server of SERVERS) {
    process.env[server.envKey] = `http://localhost:${server.port}`;
  }

  console.log('[e2e-go] All servers started and healthy.');
}
