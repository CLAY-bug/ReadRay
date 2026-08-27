function isHanCharacter(character) {
  const codePoint = character.codePointAt(0);
  return (
    (codePoint >= 0x3400 && codePoint <= 0x4dbf) ||
    (codePoint >= 0x4e00 && codePoint <= 0x9fff) ||
    (codePoint >= 0xf900 && codePoint <= 0xfaff)
  );
}

export function isPrimarilyChineseSourceSentence(value) {
  for (const character of value) {
    if (isHanCharacter(character)) {
      return true;
    }
  }
  return false;
}

export function sourceSentenceForDisplay(sourceSentence, sourceSentenceZh) {
  const normalizedSource = sourceSentence?.trim() || undefined;
  const normalizedTranslation = sourceSentenceZh?.trim() || undefined;
  return {
    sourceSentence: normalizedSource,
    sourceSentenceZh:
      normalizedSource && !isPrimarilyChineseSourceSentence(normalizedSource)
        ? normalizedTranslation
        : undefined,
  };
}
