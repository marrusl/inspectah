/**
 * Playwright global setup: generate fixtures (if stale), start servers,
 * health-check, and write .env.test + .server-pids for tests/teardown.
 */
import { execSync, spawn, ChildProcess } from 'child_process';
import { writeFileSync, readFileSync, existsSync } from 'fs';
import { join } from 'path';

const ROOT = join(__dirname, '..', '..');
const FIXTURES = join(__dirname, 'fixtures');
const SCHEMA_FILE = join(FIXTURES, '.schema-version');
const ENV_FILE = join(__dirname, '.env.test');
const PID_FILE = join(__dirname, '.server-pids');

interface ServerDef {
  name: string;
  envKey: string;
  command: string[];
  port: number;
}

const SERVERS: ServerDef[] = [
  {
    name: 'fleet-refine',
    envKey: 'REFINE_FLEET_URL',
    command: ['uv', 'run', 'yoinkc', 'refine', 'tests/e2e/fixtures/fleet-3host.tar.gz', '--no-browser', '--port', '9100'],
    port: 9100,
  },
  {
    name: 'single-refine',
    envKey: 'REFINE_SINGLE_URL',
    command: ['uv', 'run', 'yoinkc', 'refine', 'tests/e2e/fixtures/single-host.tar.gz', '--no-browser', '--port', '9101'],
    port: 9101,
  },
  {
    name: 'architect',
    envKey: 'ARCHITECT_URL',
    command: ['uv', 'run', 'yoinkc', 'architect', 'tests/e2e/fixtures/architect-topology', '--no-browser', '--port', '9102'],
    port: 9102,
  },
];

/** Check if fixtures need regenerating based on schema version. */
function fixturesAreStale(): boolean {
  if (!existsSync(SCHEMA_FILE)) return true;
  try {
    const stored = readFileSync(SCHEMA_FILE, 'utf-8').trim();
    const current = execSync(
      'uv run python -c "from yoinkc.schema import SCHEMA_VERSION; print(SCHEMA_VERSION)"',
      { cwd: ROOT, encoding: 'utf-8' },
    ).trim();
    return stored !== current;
  } catch {
    return true;
  }
}

/** Regenerate fixture tarballs. */
function regenerateFixtures(): void {
  console.log('[globalSetup] Regenerating fixtures...');
  execSync('uv run python tests/e2e/generate-fixtures.py --force', {
    cwd: ROOT,
    stdio: 'inherit',
  });
}

/** Poll /api/health until it responds 200 or timeout is reached. */
async function waitForHealth(port: number, name: string, timeoutMs = 30_000): Promise<void> {
  const url = `http://localhost:${port}/api/health`;
  const start = Date.now();
  const interval = 250;

  while (Date.now() - start < timeoutMs) {
    try {
      const res = await fetch(url);
      if (res.ok) {
        console.log(`[globalSetup] ${name} healthy on port ${port}`);
        return;
      }
    } catch {
      // Server not ready yet
    }
    await new Promise((r) => setTimeout(r, interval));
  }
  throw new Error(`[globalSetup] ${name} did not become healthy within ${timeoutMs}ms`);
}

async function globalSetup(): Promise<void> {
  console.log('[globalSetup] Starting...');

  // 1. Regenerate fixtures if schema changed
  if (fixturesAreStale()) {
    regenerateFixtures();
  } else {
    console.log('[globalSetup] Fixtures up to date.');
  }

  // 2. Start servers
  const pids: number[] = [];
  const procs: ChildProcess[] = [];

  for (const srv of SERVERS) {
    console.log(`[globalSetup] Starting ${srv.name} on port ${srv.port}...`);
    const proc = spawn(srv.command[0], srv.command.slice(1), {
      cwd: ROOT,
      stdio: 'ignore',
      detached: true,
    });
    proc.unref();
    if (!proc.pid) {
      throw new Error(`[globalSetup] Failed to start ${srv.name}`);
    }
    pids.push(proc.pid);
    procs.push(proc);
  }

  // 3. Write PIDs for teardown
  writeFileSync(PID_FILE, pids.join('\n') + '\n', 'utf-8');

  // 4. Health-check all servers in parallel
  await Promise.all(
    SERVERS.map((srv) => waitForHealth(srv.port, srv.name)),
  );

  // 5. Write .env.test for test processes
  const envLines = SERVERS.map((srv) => `${srv.envKey}=http://localhost:${srv.port}`);
  writeFileSync(ENV_FILE, envLines.join('\n') + '\n', 'utf-8');

  console.log('[globalSetup] All servers healthy. Ready for tests.');
}

export default globalSetup;
