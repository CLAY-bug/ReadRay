import { invoke } from "@tauri-apps/api/core";
import type {
  GeneratedReviewCard,
  PrepareReviewFeedCardInput,
  ReviewFeedPage,
  ReviewFeedItemState,
  ReviewOutcomeWriteResult,
  ReviewQualityFeedback,
  SaveReviewQualityFeedbackInput,
  SubmitReviewOutcomeInput,
  UndoReviewOutcomeInput,
  UndoReviewQualityFeedbackInput,
} from "./types/review";

export interface ReviewRepository {
  loadFeedPage(
    dayStartUnixMs: number,
    dayEndUnixMs: number,
    cursor?: number,
    pageSize?: number,
  ): Promise<ReviewFeedPage>;
  loadFeedItemState(feedItemId: number): Promise<ReviewFeedItemState>;
  prepareFeedCard(input: PrepareReviewFeedCardInput): Promise<GeneratedReviewCard>;
  submitOutcome(input: SubmitReviewOutcomeInput): Promise<ReviewOutcomeWriteResult>;
  undoOutcome(input: UndoReviewOutcomeInput): Promise<ReviewOutcomeWriteResult>;
  saveQualityFeedback(
    input: SaveReviewQualityFeedbackInput,
  ): Promise<ReviewQualityFeedback>;
  undoQualityFeedback(
    input: UndoReviewQualityFeedbackInput,
  ): Promise<ReviewQualityFeedback>;
}

export class TauriReviewRepository implements ReviewRepository {
  loadFeedPage(
    dayStartUnixMs: number,
    dayEndUnixMs: number,
    cursor?: number,
    pageSize?: number,
  ) {
    return invoke<ReviewFeedPage>("get_review_feed_page", {
      dayStartUnixMs,
      dayEndUnixMs,
      cursor,
      pageSize,
    });
  }

  loadFeedItemState(feedItemId: number) {
    return invoke<ReviewFeedItemState>("get_review_feed_item_state", {
      feedItemId,
    });
  }

  prepareFeedCard(input: PrepareReviewFeedCardInput) {
    return invoke<GeneratedReviewCard>("prepare_review_feed_card", { input });
  }

  submitOutcome(input: SubmitReviewOutcomeInput) {
    return invoke<ReviewOutcomeWriteResult>("submit_review_outcome", { input });
  }

  undoOutcome(input: UndoReviewOutcomeInput) {
    return invoke<ReviewOutcomeWriteResult>("undo_review_outcome", { input });
  }

  saveQualityFeedback(input: SaveReviewQualityFeedbackInput) {
    return invoke<ReviewQualityFeedback>("save_review_quality_feedback", {
      input,
    });
  }

  undoQualityFeedback(input: UndoReviewQualityFeedbackInput) {
    return invoke<ReviewQualityFeedback>("undo_review_quality_feedback", {
      input,
    });
  }
}
