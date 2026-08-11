export type ExplanationRequestScope = "manual" | "anchored";

export class ExplanationRequestAuthority {
  constructor(
    cancelRequest: (
      scope: ExplanationRequestScope,
      requestKey: string,
    ) => void,
    sessionNonceFactory?: () => string,
  );
  begin(scope: ExplanationRequestScope): string;
  invalidate(scope: ExplanationRequestScope): void;
  invalidateAll(): void;
  isCurrent(scope: ExplanationRequestScope, requestKey: string): boolean;
  finish(scope: ExplanationRequestScope, requestKey: string): boolean;
}

export function isExplanationRequestCancelled(error: unknown): boolean;
