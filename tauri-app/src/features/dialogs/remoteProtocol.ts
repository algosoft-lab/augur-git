export type ConversionReason =
  | 'credentials'
  | 'port'
  | 'user'
  | 'path'
  | 'host'
  | 'platform'
  | 'encoding'
  | 'query'
  | 'unsupported';

export type RemoteConversion =
  | { target: 'ssh' | 'https'; url: string; reason?: never }
  | { target: 'ssh' | 'https' | null; reason: ConversionReason; url?: never };

/** Convert only addresses whose host and repository path need no inferred mapping. */
export function remoteConversion(input: string): RemoteConversion {
  const value = input.trim();
  const web = /^(https?):\/\/([^/]+)\/(.*)$/i.exec(value);
  const ssh = /^ssh:\/\/([^/]+)\/(.*)$/i.exec(value);
  const scp = /^([^@/:]+)@([^/:]+):(.*)$/.exec(value);
  const target = web ? 'ssh' : ssh || scp ? 'https' : null;
  const fail = (reason: ConversionReason): RemoteConversion => ({ target, reason });
  if (!target) return fail('unsupported');
  if (value.includes('%')) return fail('encoding');
  if (/[?#]/.test(value)) return fail('query');

  let host: string;
  let path: string;
  if (web) {
    host = web[2]!;
    path = web[3]!;
    if (host.includes('@')) return fail('credentials');
  } else if (ssh) {
    const authority = ssh[1]!;
    if (!authority.startsWith('git@')) return fail('user');
    host = authority.slice(4);
    path = ssh[2]!;
  } else {
    if (scp![1] !== 'git') return fail('user');
    host = scp![2]!;
    path = scp![3]!;
  }
  if (host.includes(':') || host.includes('[')) return fail('port');
  // Single-label names may be SSH aliases; converting them needs a user-supplied mapping.
  if (!/^(?=.{1,253}$)[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$/i.test(host) || !host.includes('.')) {
    return fail('host');
  }
  if (
    /(^|\.)(dev\.azure\.com|visualstudio\.com)$/i.test(host) ||
    /^(ssh|altssh)\./i.test(host) ||
    /^(scm|v3)\//i.test(path)
  )
    return fail('platform');
  if (
    !/^[a-z0-9._-]+(?:\/[a-z0-9._-]+)*\/?$/i.test(path) ||
    path.split('/').some((part) => part === '.' || part === '..')
  )
    return fail('path');
  return { target, url: target === 'ssh' ? `git@${host}:${path}` : `https://${host}/${path}` };
}
