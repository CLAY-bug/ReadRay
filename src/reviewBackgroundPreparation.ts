import type { ReviewPreparationCoordinator } from "./reviewPreparationCoordinator";
import type { ReviewService } from "./reviewService";

export class ReviewBackgroundPreparationController {
  private readonly service: ReviewService;
  private readonly coordinator: ReviewPreparationCoordinator;
  private generation = 0;

  constructor(
    service: ReviewService,
    coordinator: ReviewPreparationCoordinator,
  ) {
    this.service = service;
    this.coordinator = coordinator;
  }

  async warmFirstPage() {
    if (this.coordinator.hasPageConsumer()) return false;
    const generation = ++this.generation;
    const feed = await this.service.loadFeedPage();
    if (
      generation !== this.generation ||
      this.coordinator.hasPageConsumer()
    ) {
      return false;
    }
    this.coordinator.syncFeed(feed);
    return true;
  }

  invalidate() {
    this.generation += 1;
  }
}
