/**
 * Playwright global teardown: kill servers and clean up temp files.
 */
import { readFileSync, unlinkSync, existsSync } from 'fs';
import { join } from 'path';

const PID_FILE = join(__dirname, '.server-pids');
const ENV_FILE = join(__dirname, '.env.test');

function killPid(pid: number): void {
  try {
    // Kill the process group (negative PID) since we used detached: true
    process.kill(-pid, 'SIGTERM');
  } catch {
    // Process may already be gone — try killing just the PID
    try {
      process.kill(pid, 'SIGTERM');
    } catch {
      // Already dead, that's fine
    }
  }
}

function cleanupFile(path: string): void {
  try {
    if (existsSync(path)) unlinkSync(path);
  } catch {
    // Ignore cleanup errors
  }
}

async function globalTeardown(): Promise<void> {
  console.log('[globalTeardown] Stopping servers...');

  if (existsSync(PID_FILE)) {
    const content = readFileSync(PID_FILE, 'utf-8').trim();
    const pids = content
      .split('\n')
      .map((s) => parseInt(s, 10))
      .filter((n) => !isNaN(n));

    for (const pid of pids) {
      console.log(`[globalTeardown] Killing PID ${pid}`);
      killPid(pid);
    }
  }

  cleanupFile(PID_FILE);
  cleanupFile(ENV_FILE);

  console.log('[globalTeardown] Cleanup complete.');
}

export default globalTeardown;
