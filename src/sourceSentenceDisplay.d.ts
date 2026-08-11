export type DisplayedSourceSentence = {
  sourceSentence?: string;
  sourceSentenceZh?: string;
};

export function isPrimarilyChineseSourceSentence(value: string): boolean;

export function sourceSentenceForDisplay(
  sourceSentence?: string | null,
  sourceSentenceZh?: string | null,
): DisplayedSourceSentence;
