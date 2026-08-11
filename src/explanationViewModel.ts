import type {
  ExampleItem,
  ExplanationCard,
  KeyPointItem,
  NearMeaningItem,
  PhraseItem,
} from "./types/explanation";
import { sourceSentenceForDisplay } from "./sourceSentenceDisplay";

type SharedResult = {
  sourceText: string;
  learningTargetText: string;
};

export type WordExplanationResult = SharedResult & {
  kind: "word";
  headword: string;
  partOfSpeech?: string;
  phonetic?: string;
  basicMeanings: string[];
  contextMeaning?: string;
  sourceSentence?: string;
  sourceSentenceZh?: string;
  phrases: PhraseItem[];
  nearMeanings: NearMeaningItem[];
  examples: ExampleItem[];
  reviewHint?: string;
};

export type PhraseExplanationResult = SharedResult & {
  kind: "phrase";
  basicMeaning: string;
  contextMeaning?: string;
  composition?: string;
  sourceSentence?: string;
  sourceSentenceZh?: string;
  examples: ExampleItem[];
  reviewHint?: string;
};

export type SentenceExplanationResult = SharedResult & {
  kind: "sentence";
  translation: string;
  keyPoints: KeyPointItem[];
  explanation?: string;
  reviewHint?: string;
};

export type ParagraphExplanationResult = SharedResult & {
  kind: "paragraph";
  translation: string;
  keyPoints: KeyPointItem[];
  summary?: string;
};

export type ExplanationResult =
  | WordExplanationResult
  | PhraseExplanationResult
  | SentenceExplanationResult
  | ParagraphExplanationResult;

function optionalText(value?: string | null) {
  const normalized = value?.trim();
  return normalized || undefined;
}

export function mapExplanationCard(card: ExplanationCard): ExplanationResult {
  switch (card.queryType) {
    case "word": {
      const sourceSentence = sourceSentenceForDisplay(
        card.sourceSentence,
        card.sourceSentenceZh,
      );
      return {
        kind: "word",
        sourceText: card.sourceText,
        learningTargetText: card.learningTargetText,
        headword: card.headword,
        partOfSpeech: optionalText(card.partOfSpeech),
        phonetic: optionalText(card.phonetic),
        basicMeanings: card.basicMeanings,
        contextMeaning: optionalText(card.contextMeaning),
        ...sourceSentence,
        phrases: card.phrases,
        nearMeanings: card.nearMeanings,
        examples: card.examples,
        reviewHint: optionalText(card.reviewHint),
      };
    }
    case "phrase": {
      const sourceSentence = sourceSentenceForDisplay(
        card.sourceSentence,
        card.sourceSentenceZh,
      );
      return {
        kind: "phrase",
        sourceText: card.sourceText,
        learningTargetText: card.learningTargetText,
        basicMeaning: card.basicMeaning,
        contextMeaning: optionalText(card.contextMeaning),
        composition: optionalText(card.composition),
        ...sourceSentence,
        examples: card.examples,
        reviewHint: optionalText(card.reviewHint),
      };
    }
    case "sentence":
      return {
        kind: "sentence",
        sourceText: card.sourceText,
        learningTargetText: card.learningTargetText,
        translation: card.translation,
        keyPoints: card.keyPoints,
        explanation: optionalText(card.explanation),
        reviewHint: optionalText(card.reviewHint),
      };
    case "paragraph":
      return {
        kind: "paragraph",
        sourceText: card.sourceText,
        learningTargetText: card.learningTargetText,
        translation: card.translation,
        keyPoints: card.keyPoints,
        summary: optionalText(card.summary),
      };
  }
}

export function preferredAnchoredWidth(result: ExplanationResult) {
  switch (result.kind) {
    case "word":
      return 500;
    case "phrase":
      return 540;
    case "sentence":
      return 640;
    case "paragraph":
      return 700;
  }
}
