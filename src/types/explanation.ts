export type QueryType = "word" | "phrase" | "sentence" | "paragraph";

export type CaptureInput = {
  queryText: string;
  contextText?: string | null;
  sourceType: "manual" | "clipboard" | "windows_uia" | "app_adapter" | "ocr";
  sourceApp?: string | null;
};

export type PhraseItem = {
  phrase: string;
  meaning: string;
};

export type NearMeaningItem = {
  term: string;
  meaning: string;
};

export type ExampleItem = {
  en: string;
  zh: string;
};

export type KeyPointItem = {
  expression: string;
  meaning: string;
};

export type WordExplanationCard = {
  queryType: "word";
  sourceText: string;
  headword: string;
  partOfSpeech?: string | null;
  phonetic?: string | null;
  basicMeanings: string[];
  contextMeaning?: string | null;
  sourceSentence?: string | null;
  sourceSentenceZh?: string | null;
  phrases: PhraseItem[];
  nearMeanings: NearMeaningItem[];
  examples: ExampleItem[];
  reviewHint?: string | null;
};

export type PhraseExplanationCard = {
  queryType: "phrase";
  sourceText: string;
  basicMeaning: string;
  contextMeaning?: string | null;
  composition?: string | null;
  sourceSentence?: string | null;
  sourceSentenceZh?: string | null;
  examples: ExampleItem[];
  reviewHint?: string | null;
};

export type SentenceExplanationCard = {
  queryType: "sentence";
  sourceText: string;
  translation: string;
  keyPoints: KeyPointItem[];
  explanation?: string | null;
  reviewHint?: string | null;
};

export type ParagraphExplanationCard = {
  queryType: "paragraph";
  sourceText: string;
  translation: string;
  keyPoints: KeyPointItem[];
  summary?: string | null;
};

export type ExplanationCard =
  | WordExplanationCard
  | PhraseExplanationCard
  | SentenceExplanationCard
  | ParagraphExplanationCard;
