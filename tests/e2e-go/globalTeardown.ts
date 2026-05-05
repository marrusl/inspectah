/**
 * Global teardown: kill all server processes started by globalSetup.
 */
import { readFileSync, unlinkSync, existsSync } from 'fs';
import { join } from 'path';

const PID_FILE = join(__dirname, '.server-pids');
const ENV_FILE = join(__dirname, '.env.test');

export default async function globalTeardown(): Promise<void> {
  if (!existsSync(PID_FILE)) {
    console.log('[e2e-go] No PID file found, nothing to tear down.');
    return;
  }

  const pids = readFileSync(PID_FILE, 'utf-8')
    .trim()
    .split('\n')
    .filter(Boolean)
    .map(Number);

  for (const pid of pids) {
    try {
      // Kill the process group (negative PID) to catch any children
      process.kill(-pid, 'SIGTERM');
      console.log(`[e2e-go] Sent SIGTERM to process group ${pid}`);
    } catch (err: unknown) {
      const code = (err as NodeJS.ErrnoException).code;
      if (code === 'ESRCH') {
        console.log(`[e2e-go] Process ${pid} already exited.`);
      } else {
        console.warn(`[e2e-go] Failed to kill process ${pid}: ${err}`);
      }
    }
  }

  // Clean up temp files
  try { unlinkSync(PID_FILE); } catch { /* ignore */ }
  try { unlinkSync(ENV_FILE); } catch { /* ignore */ }

  console.log('[e2e-go] Teardown complete.');
}
