import { buildCli } from './common.mjs';

if (['darwin', 'linux', 'win32'].includes(process.platform)) buildCli();
