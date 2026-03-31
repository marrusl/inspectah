import { readFileSync, existsSync } from 'fs';
import { join } from 'path';

const envFile = join(__dirname, '..', '.env.test');
const envVars: Record<string, string> = {};
if (existsSync(envFile)) {
  for (const line of readFileSync(envFile, 'utf-8').split('\n')) {
    const [key, val] = line.split('=');
    if (key && val) envVars[key] = val;
  }
}

export const FLEET_URL = envVars.REFINE_FLEET_URL || process.env.REFINE_FLEET_URL || 'http://localhost:9100';
export const SINGLE_URL = envVars.REFINE_SINGLE_URL || process.env.REFINE_SINGLE_URL || 'http://localhost:9101';
export const ARCHITECT_URL = envVars.ARCHITECT_URL || process.env.ARCHITECT_URL || 'http://localhost:9102';
