export class ReviewAuthorityRefreshGate {
  private deferred = false;

  requestRefresh(blockedByMutation: boolean) {
    if (blockedByMutation) {
      this.deferred = true;
      return false;
    }
    return true;
  }

  releaseDeferredRefresh(blockedByMutation: boolean) {
    if (!this.deferred || blockedByMutation) return false;
    this.deferred = false;
    return true;
  }

  reset() {
    this.deferred = false;
  }

  get hasDeferredRefresh() {
    return this.deferred;
  }
}
