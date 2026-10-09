export interface FuzzyMatch {
  score: number;
  indices: number[];
}

export function fuzzyMatch(query: string, text: string): FuzzyMatch | null {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  if (!normalizedQuery) return { score: 0, indices: [] };

  const normalizedText = text.toLocaleLowerCase();
  const indices: number[] = [];
  let score = 0;
  let cursor = 0;
  let previous = -2;

  for (const character of normalizedQuery) {
    const index = normalizedText.indexOf(character, cursor);
    if (index < 0) return null;

    const priorCharacter = text[index - 1] ?? '';
    const startsWord = index === 0 || /[\s/_.-]/.test(priorCharacter);
    score += 10;
    if (index === 0) score += 18;
    if (startsWord) score += 12;
    if (index === previous + 1) score += 14;
    score -= Math.min(8, Math.max(0, index - cursor));
    if (text[index] === character) score += 1;
    indices.push(index);
    cursor = index + 1;
    previous = index;
  }

  if (normalizedText === normalizedQuery) score += 60;
  else if (normalizedText.startsWith(normalizedQuery)) score += 30;

  return { score, indices };
}
