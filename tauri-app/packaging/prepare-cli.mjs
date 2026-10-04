import { buildCli } from './common.mjs';

if (process.platform === 'darwin' || process.platform === 'linux') buildCli();
