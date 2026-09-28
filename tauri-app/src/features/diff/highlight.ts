/**
 * Lightweight syntax highlighting.
 *
 * The reference application uses tree-sitter. A webview cannot rely on a native
 * grammar bundle, so this module tokenizes the languages the backend maps with
 * regular expressions. It is intentionally cheap: a line that matches nothing is
 * returned as plain text, and the per-file byte ceiling stops a generated file
 * from stalling a paint.
 */

/** Above this size a source is rendered without highlighting. */
export const MAX_HIGHLIGHT_BYTES = 10 * 1024 * 1024;

export type TokenKind =
  'keyword' | 'string' | 'number' | 'comment' | 'type' | 'function' | 'punct' | 'tag' | 'attr';

export interface Token {
  kind: TokenKind;
  start: number;
  end: number;
}

interface Rule {
  kind: TokenKind;
  pattern: RegExp;
}

const SHARED: Rule[] = [
  { kind: 'comment', pattern: /\/\/[^\n]*|\/\*[\s\S]*?\*\/|#[^\n]*|--[^\n]*/y },
  { kind: 'string', pattern: /"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'|`(?:[^`\\]|\\.)*`/y },
  { kind: 'number', pattern: /\b0[xX][0-9a-fA-F_]+\b|\b\d[\d_]*(?:\.\d+)?(?:[eE][+-]?\d+)?\b/y }
];

const RULES: Record<string, Rule[]> = {
  rust: [
    {
      kind: 'keyword',
      pattern:
        /\b(?:as|async|await|break|const|continue|crate|dyn|else|enum|extern|fn|for|if|impl|in|let|loop|match|mod|move|mut|pub|ref|return|self|Self|static|struct|super|trait|type|unsafe|use|where|while)\b/y
    },
    {
      kind: 'type',
      pattern:
        /\b(?:bool|char|f32|f64|i8|i16|i32|i64|i128|isize|str|u8|u16|u32|u64|u128|usize|String|Vec|Option|Result|Box|Arc|HashMap|HashSet)\b/y
    },
    { kind: 'keyword', pattern: /\b(?:true|false)\b/y },
    ...SHARED,
    { kind: 'function', pattern: /\b[A-Za-z_][A-Za-z0-9_]*(?=::|\()/y },
    { kind: 'attr', pattern: /#!?\[[^\]]*\]/y },
    { kind: 'punct', pattern: /[{}()[\];,.:?<>=+\-*/%!&|^~@]+/y }
  ],
  typescript: [
    {
      kind: 'keyword',
      pattern:
        /\b(?:abstract|as|async|await|break|case|catch|class|const|continue|declare|default|delete|do|else|enum|export|extends|finally|for|from|function|get|if|implements|import|in|instanceof|interface|let|new|of|private|protected|public|readonly|return|set|static|super|switch|this|throw|try|type|typeof|var|void|while|yield)\b/y
    },
    { kind: 'keyword', pattern: /\b(?:true|false|null|undefined)\b/y },
    {
      kind: 'type',
      pattern: /\b(?:any|boolean|never|number|object|string|symbol|unknown|bigint)\b/y
    },
    ...SHARED,
    { kind: 'function', pattern: /\b[A-Za-z_$][A-Za-z0-9_$]*(?=\s*\()/y },
    { kind: 'attr', pattern: /@[A-Za-z_$][A-Za-z0-9_$]*/y },
    { kind: 'punct', pattern: /[{}()[\];,.:?<>=+\-*/%!&|^~]+/y }
  ],
  python: [
    {
      kind: 'keyword',
      pattern:
        /\b(?:and|as|assert|async|await|break|class|continue|def|del|elif|else|except|finally|for|from|global|if|import|in|is|lambda|nonlocal|not|or|pass|raise|return|try|while|with|yield|match|case)\b/y
    },
    { kind: 'keyword', pattern: /\b(?:True|False|None)\b/y },
    { kind: 'type', pattern: /\b(?:int|str|float|bool|bytes|list|dict|set|tuple|object)\b/y },
    { kind: 'attr', pattern: /\bself\b|\b[A-Za-z_][A-Za-z0-9_]*(?=\()/y },
    ...SHARED,
    { kind: 'punct', pattern: /[{}()[\];,.:<>=+\-*/%!&|^~@]+/y }
  ],
  json: [
    { kind: 'attr', pattern: /"(?:[^"\\]|\\.)*"(?=\s*:)/y },
    { kind: 'string', pattern: /"(?:[^"\\]|\\.)*"/y },
    { kind: 'number', pattern: /-?\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b/y },
    { kind: 'keyword', pattern: /\b(?:true|false|null)\b/y },
    { kind: 'punct', pattern: /[{}[\],:]+/y }
  ],
  css: [
    { kind: 'comment', pattern: /\/\*[\s\S]*?\*\//y },
    { kind: 'tag', pattern: /[.#]?[A-Za-z_][\w-]*(?=[^;{}]*\{)/y },
    { kind: 'attr', pattern: /[a-zA-Z-]+(?=\s*:)/y },
    { kind: 'string', pattern: /"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'/y },
    { kind: 'number', pattern: /-?\b\d+(?:\.\d+)?(?:px|em|rem|%|vh|vw|s|ms|deg|fr)?\b/y },
    { kind: 'punct', pattern: /[{}();:,>+~*]+/y }
  ],
  html: [
    { kind: 'comment', pattern: /<!--[\s\S]*?-->/y },
    { kind: 'tag', pattern: /<\/?[A-Za-z][\w-]*/y },
    { kind: 'attr', pattern: /\b[A-Za-z-]+(?==)/y },
    { kind: 'string', pattern: /"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'/y },
    { kind: 'punct', pattern: /<\/?>|\/>/y }
  ],
  markdown: [
    { kind: 'keyword', pattern: /^#{1,6} [^\n]*/my },
    { kind: 'attr', pattern: /`[^`\n]*`/y },
    { kind: 'string', pattern: /\*\*[^*\n]+\*\*|__[^_\n]+__/y },
    { kind: 'punct', pattern: /^\s*[-*+]\s|\[[^\]\n]*\]\([^)\n]*\)/my }
  ],
  shell: [
    {
      kind: 'keyword',
      pattern:
        /\b(?:case|do|done|elif|else|esac|fi|for|function|if|in|local|return|then|until|while)\b/y
    },
    { kind: 'attr', pattern: /\$\{?[A-Za-z_][A-Za-z0-9_]*\}?/y },
    ...SHARED,
    { kind: 'punct', pattern: /[|&;<>(){}[\]]+/y }
  ],
  yaml: [
    { kind: 'attr', pattern: /^[ \t]*[-\w.]+(?=\s*:)/my },
    { kind: 'string', pattern: /"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'/y },
    { kind: 'number', pattern: /-?\b\d+(?:\.\d+)?\b/y },
    { kind: 'keyword', pattern: /\b(?:true|false|null|yes|no)\b/y },
    { kind: 'punct', pattern: /[-:[\]{},>|&*?]+/y }
  ],
  sql: [
    {
      kind: 'keyword',
      pattern:
        /\b(?:SELECT|FROM|WHERE|INSERT|INTO|VALUES|UPDATE|SET|DELETE|CREATE|TABLE|INDEX|DROP|ALTER|JOIN|LEFT|RIGHT|INNER|OUTER|ON|GROUP|BY|ORDER|HAVING|LIMIT|OFFSET|UNION|ALL|DISTINCT|AS|AND|OR|NOT|NULL|PRIMARY|KEY|FOREIGN|REFERENCES|DEFAULT|UNIQUE)\b/iy
    },
    ...SHARED,
    { kind: 'number', pattern: /\b\d+(?:\.\d+)?\b/y },
    { kind: 'punct', pattern: /[(),;.*=<>+\-/|]+/y }
  ],
  go: [
    {
      kind: 'keyword',
      pattern:
        /\b(?:break|case|chan|const|continue|default|defer|else|fallthrough|for|func|go|goto|if|import|interface|map|package|range|return|select|struct|switch|type|var)\b/y
    },
    { kind: 'keyword', pattern: /\b(?:true|false|nil|iota)\b/y },
    {
      kind: 'type',
      pattern:
        /\b(?:bool|byte|complex64|complex128|error|float32|float64|int|int8|int16|int32|int64|rune|string|uint|uint8|uint16|uint32|uint64|uintptr|any)\b/y
    },
    ...SHARED,
    { kind: 'function', pattern: /\b[A-Za-z_][A-Za-z0-9_]*(?=\()/y },
    { kind: 'punct', pattern: /[{}()[\];,.:<>+\-*/%!&|^~@]+/y }
  ],
  java: [
    {
      kind: 'keyword',
      pattern:
        /\b(?:abstract|assert|break|case|catch|class|continue|default|do|else|enum|extends|final|finally|for|if|implements|import|instanceof|interface|native|new|package|private|protected|public|return|static|strictfp|super|switch|synchronized|this|throw|throws|transient|try|volatile|while|record|sealed|var|yield)\b/y
    },
    { kind: 'keyword', pattern: /\b(?:true|false|null)\b/y },
    {
      kind: 'type',
      pattern:
        /\b(?:boolean|byte|char|double|float|int|long|short|void|String|List|Map|Set|Optional)\b/y
    },
    ...SHARED,
    { kind: 'attr', pattern: /@[A-Za-z_][A-Za-z0-9_]*/y },
    { kind: 'punct', pattern: /[{}()[\];,.:<>+\-*/%!&|^~?]+/y }
  ],
  c: [
    {
      kind: 'keyword',
      pattern:
        /\b(?:auto|break|case|char|const|continue|default|do|double|else|enum|extern|float|for|goto|if|inline|int|long|register|restrict|return|short|signed|sizeof|static|struct|switch|typedef|union|unsigned|void|volatile|while|_Bool)\b/y
    },
    ...SHARED,
    { kind: 'function', pattern: /\b[A-Za-z_][A-Za-z0-9_]*(?=\s*\()/y },
    { kind: 'attr', pattern: /#\s*\w+/y },
    { kind: 'punct', pattern: /[{}()[\];,.:<>=+\-*/%!&|^~@]+/y }
  ],
  diff: [
    { kind: 'keyword', pattern: /^@@[^\n]*/my },
    { kind: 'string', pattern: /^\+(?!\+\+)[^\n]*/my },
    { kind: 'keyword', pattern: /^-(?!--)[^\n]*/my }
  ],
  ruby: [
    {
      kind: 'keyword',
      pattern:
        /\b(?:alias|and|begin|break|case|class|def|defined\?|do|else|elsif|end|ensure|for|if|in|module|next|nil|not|or|redo|rescue|retry|return|self|super|then|undef|unless|until|when|while|yield|attr_accessor|require)\b/y
    },
    ...SHARED,
    { kind: 'attr', pattern: /@[A-Za-z_][A-Za-z0-9_]*/y },
    { kind: 'punct', pattern: /[{}()[\];,.:<>=+\-*/%!&|^~]+/y }
  ],
  php: [
    {
      kind: 'keyword',
      pattern:
        /<\?php|\?>|abstract|class|const|echo|else|elseif|extends|final|foreach|function|if|implements|include|namespace|new|private|protected|public|require|return|static|throw|try|use|var|while|match|fn|yield/iy
    },
    { kind: 'keyword', pattern: /\$?[A-Za-z_][A-Za-z0-9_]*(?=\s*\()/y },
    ...SHARED,
    { kind: 'punct', pattern: /[{}()[\];,.:<>=+\-*/%!&|^~@$?]+/y }
  ],
  lua: [
    {
      kind: 'keyword',
      pattern:
        /\b(?:and|break|do|else|elseif|end|for|function|goto|if|in|local|not|or|repeat|return|then|until|while)\b/y
    },
    { kind: 'keyword', pattern: /\b(?:true|false|nil)\b/y },
    ...SHARED,
    { kind: 'function', pattern: /\b[A-Za-z_][A-Za-z0-9_]*(?=\s*[({])/y },
    { kind: 'punct', pattern: /[{}()[\];,.:+\-*/%#=~^<>]+/y }
  ],
  toml: [
    { kind: 'attr', pattern: /^[ \t]*\[[^\]\n]+\]/my },
    { kind: 'attr', pattern: /^[ \t]*[A-Za-z0-9_.-]+(?=\s*=)/my },
    ...SHARED,
    { kind: 'punct', pattern: /[={}\[\],.]+/y }
  ],
  cmake: [
    {
      kind: 'keyword',
      pattern:
        /\b(?:cmake_minimum_required|project|add_executable|add_library|target_link_libraries|target_include_directories|set|if|elseif|else|endif|foreach|endforeach|find_package|include|option|install)\b/iy
    },
    ...SHARED,
    { kind: 'punct', pattern: /[(){}]+/y }
  ]
};

/** Aliases so a file extension resolves to the same grammar. */
const ALIASES: Record<string, keyof typeof RULES> = {
  javascript: 'typescript',
  jsx: 'typescript',
  mjs: 'typescript',
  cjs: 'typescript',
  tsx: 'typescript',
  mts: 'typescript',
  cts: 'typescript',
  jsdoc: 'typescript',
  htm: 'html',
  erb: 'ruby',
  ejs: 'html',
  pyi: 'python',
  h: 'c',
  hpp: 'c',
  cpp: 'c',
  sh: 'shell',
  bash: 'shell',
  zsh: 'shell',
  fish: 'shell',
  make: 'cmake',
  jsp: 'html',
  scala: 'java',
  cs: 'java',
  kt: 'java',
  kts: 'java',
  groovy: 'java',
  dart: 'java',
  swift: 'java',
  graphql: 'json',
  gql: 'json',
  astro: 'html',
  svelte: 'html',
  vue: 'html',
  less: 'css',
  scss: 'css',
  md: 'markdown',
  markdown: 'markdown',
  mdx: 'markdown',
  csharp: 'java',
  protobuf: 'c',
  proto: 'c',
  patch: 'diff',
  zig: 'c',
  hs: 'python',
  ex: 'ruby',
  exs: 'ruby',
  ml: 'python',
  r: 'python',
  pl: 'python'
};

export function grammarFor(language: string | null): Rule[] | null {
  if (!language) {
    return null;
  }
  // An alias may point at another alias, so resolution follows the chain and
  // stops on a cycle.
  const seen = new Set<string>();
  let key = language;
  while (!RULES[key] && ALIASES[key] && !seen.has(key)) {
    seen.add(key);
    key = ALIASES[key]!;
  }
  return RULES[key] ?? null;
}

/** Tokenize one line. Returns `null` when the grammar does not apply. */
export function tokenize(line: string, language: string | null): Token[] | null {
  const rules = grammarFor(language);
  if (!rules || line.length === 0) {
    return null;
  }
  const tokens: Token[] = [];
  let index = 0;
  while (index < line.length) {
    let matched = false;
    for (const rule of rules) {
      rule.pattern.lastIndex = index;
      const match = rule.pattern.exec(line);
      if (match && match.index === index && match[0].length > 0) {
        tokens.push({ kind: rule.kind, start: index, end: index + match[0].length });
        index += match[0].length;
        matched = true;
        break;
      }
    }
    if (!matched) {
      index += 1;
    }
  }
  return tokens.length ? tokens : null;
}
