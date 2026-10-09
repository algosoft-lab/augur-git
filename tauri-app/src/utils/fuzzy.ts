export function normalize(value: string): string {
  let out = '';
  for (const character of value) {
    if (/\s/.test(character) || character === '_' || character === '-') {
      continue;
    }
    out += character.toLowerCase();
  }
  return out;
}

export function subsequence(haystack: string, needle: string): boolean {
  let from = 0;
  for (const character of needle) {
    from = haystack.indexOf(character, from);
    if (from === -1) {
      return false;
    }
    from += 1;
  }
  return true;
}

export function fuzzyScore(query: string, text: string): number | null {
  const needle = normalize(query);
  if (!needle) {
    return null;
  }

  const characters: string[] = [];
  const sourcePositions: number[] = [];
  let sourcePosition = 0;
  for (const character of text) {
    if (/\s/.test(character) || character === '_' || character === '-') {
      sourcePosition += 1;
      continue;
    }
    for (const normalizedCharacter of character.toLowerCase()) {
      characters.push(normalizedCharacter);
      sourcePositions.push(sourcePosition);
    }
    sourcePosition += 1;
  }

  const sourceCharacters = Array.from(text);
  const positions: number[] = [];
  let from = 0;
  for (const character of needle) {
    const index = characters.indexOf(character, from);
    if (index === -1) {
      return null;
    }
    positions.push(index);
    from = index + 1;
  }

  let score = 0;
  for (let index = 0; index < positions.length; index += 1) {
    const position = positions[index]!;
    score += 4;
    if (index === 0 && position === 0) {
      score += 24;
    }
    if (index > 0) {
      const previous = positions[index - 1]!;
      if (position === previous + 1) {
        score += 10;
      } else {
        score -= Math.min(position - previous - 1, 8);
      }
    }

    const sourcePosition = sourcePositions[position]!;
    const current = sourceCharacters[sourcePosition];
    const previous = sourceCharacters[sourcePosition - 1];
    if (
      sourcePosition === 0 ||
      /[\s_\-./]/.test(previous ?? '') ||
      (current !== undefined &&
        previous !== undefined &&
        current !== current.toLowerCase() &&
        previous === previous.toLowerCase())
    ) {
      score += 8;
    }
  }

  return score;
}
