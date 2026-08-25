export const BALANCE_REFRESH_INTERVAL_MS = 5 * 60 * 1_000;
export const BALANCE_RETRY_INTERVAL_MS = 3 * 1_000;

export type BalanceRefreshContext = {
  active: boolean;
  visible: boolean;
  apiKeyConfigured: boolean;
};

export type BalanceRefreshEvent<T> =
  | { type: "reset" }
  | { type: "loading" }
  | { type: "success"; value: T }
  | { type: "error"; error: unknown };

export type BalanceRefreshState<T> = {
  status: "idle" | "loading" | "success" | "error";
  value?: T;
  error?: unknown;
};

export function reduceBalanceRefreshState<T>(
  state: BalanceRefreshState<T>,
  event: BalanceRefreshEvent<T>,
): BalanceRefreshState<T> {
  switch (event.type) {
    case "reset":
      return { status: "idle" };
    case "loading":
      return { ...state, status: "loading", error: undefined };
    case "success":
      return { status: "success", value: event.value };
    case "error":
      return { ...state, status: "error", error: event.error };
  }
}

export type BalanceRefreshScheduler = {
  set(callback: () => void, delayMs: number): unknown;
  clear(handle: unknown): void;
};

const defaultScheduler: BalanceRefreshScheduler = {
  set(callback, delayMs) {
    return globalThis.setTimeout(callback, delayMs);
  },
  clear(handle) {
    globalThis.clearTimeout(handle as ReturnType<typeof setTimeout>);
  },
};

const inactiveContext: BalanceRefreshContext = {
  active: false,
  visible: false,
  apiKeyConfigured: false,
};

export class BalanceRefreshController<T> {
  private readonly load: () => Promise<T>;

  private readonly notify: (event: BalanceRefreshEvent<T>) => void;

  private readonly scheduler: BalanceRefreshScheduler;

  private context = inactiveContext;

  private timer: unknown;

  private requestInFlight = false;

  private requestGeneration = 0;

  private refreshAfterCurrent = false;

  private retryAttempt = 0;

  private disposed = false;

  constructor(
    load: () => Promise<T>,
    notify: (event: BalanceRefreshEvent<T>) => void,
    scheduler: BalanceRefreshScheduler = defaultScheduler,
  ) {
    this.load = load;
    this.notify = notify;
    this.scheduler = scheduler;
  }

  updateContext(context: BalanceRefreshContext) {
    if (this.disposed) return;
    const wasEligible = this.isEligible();
    this.context = { ...context };
    const isEligible = this.isEligible();
    if (!isEligible) {
      this.retryAttempt = 0;
      this.pause();
      return;
    }
    if (!wasEligible) {
      if (this.requestInFlight) {
        this.refreshAfterCurrent = true;
      } else {
        this.startRequest();
      }
    }
  }

  refreshNow() {
    if (this.disposed || !this.isEligible() || this.requestInFlight) {
      return false;
    }
    this.clearTimer();
    this.retryAttempt = 0;
    this.startRequest();
    return true;
  }

  replaceCredential(context: BalanceRefreshContext) {
    if (this.disposed) return false;
    this.clearTimer();
    this.requestGeneration += 1;
    this.refreshAfterCurrent = false;
    this.retryAttempt = 0;
    this.context = { ...context };
    this.notify({ type: "reset" });
    if (!this.isEligible()) return false;
    if (this.requestInFlight) {
      this.refreshAfterCurrent = true;
    } else {
      this.startRequest();
    }
    return true;
  }

  dispose() {
    if (this.disposed) return;
    this.disposed = true;
    this.pause();
  }

  private isEligible() {
    return (
      !this.disposed &&
      this.context.active &&
      this.context.visible &&
      this.context.apiKeyConfigured
    );
  }

  private pause() {
    this.clearTimer();
    this.requestGeneration += 1;
    this.refreshAfterCurrent = false;
    this.retryAttempt = 0;
  }

  private clearTimer() {
    if (this.timer === undefined) return;
    this.scheduler.clear(this.timer);
    this.timer = undefined;
  }

  private scheduleNext(delayMs = BALANCE_REFRESH_INTERVAL_MS) {
    this.clearTimer();
    if (!this.isEligible() || this.requestInFlight) return;
    this.timer = this.scheduler.set(() => {
      this.timer = undefined;
      this.startRequest();
    }, delayMs);
  }

  private startRequest() {
    if (!this.isEligible() || this.requestInFlight) return;
    this.clearTimer();
    this.requestInFlight = true;
    this.refreshAfterCurrent = false;
    const requestGeneration = this.requestGeneration + 1;
    this.requestGeneration = requestGeneration;
    this.notify({ type: "loading" });
    void this.load().then(
      (value) => this.finishRequest(requestGeneration, { type: "success", value }),
      (error) => this.finishRequest(requestGeneration, { type: "error", error }),
    );
  }

  private finishRequest(
    requestGeneration: number,
    event: BalanceRefreshEvent<T>,
  ) {
    this.requestInFlight = false;
    if (this.disposed) return;
    if (requestGeneration !== this.requestGeneration || !this.isEligible()) {
      if (this.isEligible() && this.refreshAfterCurrent) {
        this.startRequest();
      }
      return;
    }
    this.notify(event);
    if (event.type === "success") {
      this.retryAttempt = 0;
      this.scheduleNext();
    } else if (this.retryAttempt === 0) {
      this.retryAttempt = 1;
      this.scheduleNext(BALANCE_RETRY_INTERVAL_MS);
    } else {
      this.scheduleNext();
    }
  }
}
