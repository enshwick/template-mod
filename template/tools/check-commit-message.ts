// Checks that a commit message is a conventional commit.
// Usage: bun tools/check-commit-message.ts <message-file>
//        bun tools/check-commit-message.ts --range <from>..<to>

const TYPES = ['feat', 'fix', 'perf', 'refactor', 'docs', 'test', 'build', 'ci', 'chore', 'revert'];
const HEADER = new RegExp(`^(${TYPES.join('|')})(\\([a-z0-9-]+\\))?!?: [^A-Z\\s](.*[^.])?$`, 'u');
const MAX_HEADER = 72;
// GitHub appends ` (#<number>)` to a squashed pull request's title.
const SQUASH_SUFFIX = / \(#\d+\)$/u;
// Messages git writes itself.
const GENERATED = /^(Merge |Revert "|fixup! |squash! |amend! )/u;

const problems = (message: string): string[] => {
  const text = message
    .split('\n')
    .filter((line) => !line.startsWith('#'))
    .join('\n')
    .trim();
  const header = text.split('\n')[0] ?? '';
  const found: string[] = [];

  if (!GENERATED.test(header)) {
    if (!HEADER.test(header)) {
      found.push(
        `"${header}" is not "<type>(<scope>): <subject>" with a type of ${TYPES.join(', ')}, ` +
          'a lower-case subject and no final period',
      );
    }

    // release-please reads a release pull request's notes back as HTML: a
    // `<word>` in a changelog line hides the releases after it.
    if (/[<>]/u.test(header)) {
      found.push('the header has < or >, which break the release notes');
    }

    const length = header.replace(SQUASH_SUFFIX, '').length;
    if (length > MAX_HEADER) {
      found.push(`the header is ${length} characters, at most ${MAX_HEADER}`);
    }
  }


  return found;
};

const messages = async (args: string[]): Promise<Array<{ name: string; text: string }>> => {
  if (args[0] === '--range' && args[1]) {
    // A new repository has no remote main yet: every unpushed commit counts.
    const [from, to] = args[1].split('..');
    const known = from && Bun.spawnSync(['git', 'rev-parse', '--verify', '--quiet', from]).exitCode === 0;
    const range = known || !to ? [args[1]] : [to, '--not', '--remotes'];
    const log = Bun.spawnSync(['git', 'log', '--format=%H%x00%B%x01', ...range]);
    if (log.exitCode !== 0) {
      throw new Error(log.stderr.toString());
    }

    return log.stdout
      .toString()
      .split('\x01')
      .map((entry) => entry.trim())
      .filter(Boolean)
      .map((entry) => {
        const [hash, body] = entry.split('\0');

        return { name: (hash ?? '').slice(0, 7), text: body ?? '' };
      });
  }

  if (!args[0]) {
    throw new Error('usage: check-commit-message.ts <message-file> | --range <from>..<to>');
  }

  return [{ name: 'this commit', text: await Bun.file(args[0]).text() }];
};

let failed = false;

for (const { name, text } of await messages(Bun.argv.slice(2))) {
  for (const problem of problems(text)) {
    console.error(`${name}: ${problem}`);
    failed = true;
  }
}

process.exit(failed ? 1 : 0);
