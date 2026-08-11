const SESSION_NONCE_MAX_LENGTH = 96;
const SESSION_NONCE_PATTERN = /^[A-Za-z0-9_-]+$/;

function createSecureSessionNonce() {
  const cryptoApi = globalThis.crypto;
  if (typeof cryptoApi?.randomUUID === "function") {
    return cryptoApi.randomUUID();
  }
  if (typeof cryptoApi?.getRandomValues === "function") {
    const bytes = new Uint8Array(16);
    cryptoApi.getRandomValues(bytes);
    return Array.from(bytes, (value) => value.toString(16).padStart(2, "0")).join(
      "",
    );
  }
  throw new Error("ExplanationCard 请求身份无法安全生成。");
}

function validateSessionNonce(value) {
  const valid =
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= SESSION_NONCE_MAX_LENGTH &&
    SESSION_NONCE_PATTERN.test(value);
  if (!valid) {
    throw new Error("ExplanationCard session nonce 无效。");
  }
  return value;
}

export class ExplanationRequestAuthority {
  sequence = {
    manual: 0,
    anchored: 0,
  };

  active = {};

  constructor(cancelRequest, sessionNonceFactory = createSecureSessionNonce) {
    if (typeof sessionNonceFactory !== "function") {
      throw new Error("ExplanationCard session nonce factory 无效。");
    }
    this.cancelRequest = cancelRequest;
    this.sessionNonce = validateSessionNonce(sessionNonceFactory());
  }

  begin(scope) {
    this.invalidate(scope);
    const nextSequence = this.sequence[scope] + 1;
    if (!Number.isSafeInteger(nextSequence)) {
      throw new Error("ExplanationCard requestKey sequence 已耗尽。");
    }
    this.sequence[scope] = nextSequence;
    const requestKey = `${scope}:${this.sessionNonce}:${nextSequence}`;
    this.active[scope] = requestKey;
    return requestKey;
  }

  invalidate(scope) {
    const requestKey = this.active[scope];
    delete this.active[scope];
    if (requestKey) {
      this.cancelRequest(scope, requestKey);
    }
  }

  invalidateAll() {
    this.invalidate("manual");
    this.invalidate("anchored");
  }

  isCurrent(scope, requestKey) {
    return this.active[scope] === requestKey;
  }

  finish(scope, requestKey) {
    if (!this.isCurrent(scope, requestKey)) {
      return false;
    }
    delete this.active[scope];
    return true;
  }
}

export function isExplanationRequestCancelled(error) {
  return String(error).includes("READRAY_EXPLANATION_REQUEST_CANCELLED");
}
