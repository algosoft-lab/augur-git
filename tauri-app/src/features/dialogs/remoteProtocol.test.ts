import { describe, expect, it } from 'vitest';
import { remoteConversion } from './remoteProtocol';

describe('remote protocol conversion', () => {
  it.each([
    ['https://github.com/Owner/Repo.git', 'git@github.com:Owner/Repo.git', 'ssh'],
    ['http://git.example.com/group/sub/Repo', 'git@git.example.com:group/sub/Repo', 'ssh'],
    ['git@gitlab.com:group/sub/Repo.git', 'https://gitlab.com/group/sub/Repo.git', 'https'],
    ['ssh://git@git.example.com/group/Repo.git', 'https://git.example.com/group/Repo.git', 'https'],
    ['git@bitbucket.org:Owner/Repo.git', 'https://bitbucket.org/Owner/Repo.git', 'https'],
    [' https://git.example.com/Owner/Repo.git/ ', 'git@git.example.com:Owner/Repo.git/', 'ssh']
  ])('converts %s without changing the repository path', (input, url, target) => {
    expect(remoteConversion(input)).toEqual({ target, url });
  });

  it.each([
    ['https://user:token@git.example.com/group/repo.git', 'credentials'],
    ['https://git.example.com:443/group/repo.git', 'port'],
    ['ssh://git@git.example.com:22/group/repo.git', 'port'],
    ['https://git.example.com/group/repo?token=secret', 'query'],
    ['https://git.example.com/group/repo#main', 'query'],
    ['https://git.example.com/group/a%20b.git', 'encoding'],
    ['ssh://alice@git.example.com/group/repo.git', 'user'],
    ['alice@git.example.com:group/repo.git', 'user'],
    ['git@work:group/repo.git', 'host'],
    ['git@git.example.com:/absolute/repo.git', 'path'],
    ['git@git.example.com:~/repo.git', 'path'],
    ['https://git.example.com/a/../repo.git', 'path'],
    ['https://git.example.com/a//repo.git', 'path'],
    ['https://dev.azure.com/org/project/_git/repo', 'platform'],
    ['git@ssh.dev.azure.com:v3/org/project/repo', 'platform'],
    ['https://git.example.com/scm/team/repo.git', 'platform'],
    ['git@altssh.gitlab.com:group/repo.git', 'platform'],
    ['/home/user/repo.git', 'unsupported'],
    ['C:\\repos\\repo.git', 'unsupported'],
    ['file:///tmp/repo.git', 'unsupported'],
    ['git://git.example.com/group/repo.git', 'unsupported'],
    ['', 'unsupported']
  ])('requires manual editing for %s', (input, reason) => {
    expect(remoteConversion(input).reason).toBe(reason);
    expect(remoteConversion(input).url).toBeUndefined();
  });
});
