import type { ThemePackageSelection } from "./themeRepository.ts";
import type { ThemeService } from "./themeService.ts";
import {
  READRAY_BUILTIN_THEME_IDS,
  READRAY_DEFAULT_THEME_ID,
  type ReadRayThemeV1,
  type ThemeMode,
  type ThemeSnapshot,
} from "./themeProtocol.ts";

export type ThemeMutationRetry =
  | { kind: "import" }
  | { kind: "select"; themeId: string; mode: ThemeMode }
  | { kind: "delete"; themeId: string };

export type ThemeMutationReceipt =
  | { kind: "import"; themeId: string }
  | { kind: "select"; themeId: string; mode: ThemeMode }
  | { kind: "delete"; themeId: string };

export type ThemeMutationOutcome =
  | { status: "saved"; snapshot: ThemeSnapshot; mutation: ThemeMutationReceipt }
  | { status: "cancelled" }
  | {
      status: "failed";
      snapshot: ThemeSnapshot;
      retry: ThemeMutationRetry;
      message: string;
    }
  | {
      status: "conflict";
      snapshot: ThemeSnapshot;
      message: string;
    }
  | { status: "superseded" };

type ThemeMutationOperations = {
  service: ThemeService;
  apply: (snapshot: ThemeSnapshot) => void;
};

type ThemeMutationTarget =
  | { kind: "import"; theme: ReadRayThemeV1 }
  | { kind: "select"; themeId: string; mode: ThemeMode }
  | { kind: "delete"; themeId: string };

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function sameValue(left: unknown, right: unknown) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function sameThemeSets(left: ReadRayThemeV1[], right: ReadRayThemeV1[]) {
  if (left.length !== right.length) return false;
  const rightById = new Map(right.map((theme) => [theme.manifest.id, theme]));
  return left.every((theme) => sameValue(theme, rightById.get(theme.manifest.id)));
}

function mutationReceipt(target: ThemeMutationTarget): ThemeMutationReceipt {
  switch (target.kind) {
    case "import":
      return { kind: "import", themeId: target.theme.manifest.id };
    case "select":
      return { kind: "select", themeId: target.themeId, mode: target.mode };
    case "delete":
      return { kind: "delete", themeId: target.themeId };
  }
}

function mutationWasCommitted(
  previous: ThemeSnapshot,
  authority: ThemeSnapshot,
  target: ThemeMutationTarget,
) {
  if (authority.revision !== previous.revision + 1) return false;
  switch (target.kind) {
    case "import": {
      const themeId = target.theme.manifest.id;
      if (previous.themes.some((theme) => theme.manifest.id === themeId)) return false;
      if (
        authority.currentThemeId !== previous.currentThemeId ||
        authority.currentMode !== previous.currentMode ||
        authority.themes.length !== previous.themes.length + 1
      ) return false;
      const imported = authority.themes.find((theme) => theme.manifest.id === themeId);
      return sameValue(imported, target.theme) && previous.themes.every((theme) =>
        sameValue(
          theme,
          authority.themes.find((candidate) => candidate.manifest.id === theme.manifest.id),
        )
      );
    }
    case "select":
      return authority.currentThemeId === target.themeId &&
        authority.currentMode === target.mode &&
        sameThemeSets(authority.themes, previous.themes);
    case "delete": {
      const previousTarget = previous.themes.find(
        (theme) => theme.manifest.id === target.themeId,
      );
      if (!previousTarget || previousTarget.builtin) return false;
      if (
        authority.themes.some((theme) => theme.manifest.id === target.themeId) ||
        authority.themes.length !== previous.themes.length - 1
      ) return false;
      const expectedThemeId = previous.currentThemeId === target.themeId
        ? READRAY_DEFAULT_THEME_ID
        : previous.currentThemeId;
      const expectedMode = previous.currentThemeId === target.themeId
        ? "light"
        : previous.currentMode;
      return authority.currentThemeId === expectedThemeId &&
        authority.currentMode === expectedMode &&
        authority.themes.every((theme) => sameValue(
          theme,
          previous.themes.find((candidate) => candidate.manifest.id === theme.manifest.id),
        ));
    }
  }
}

export class ThemeMutationCoordinator {
  private generation = 0;
  private pendingRequests = 0;
  private readonly activeMutations = new Set<Promise<ThemeMutationOutcome>>();
  private readonly operations: ThemeMutationOperations;

  constructor(operations: ThemeMutationOperations) {
    this.operations = operations;
  }

  get isSaving() {
    return this.pendingRequests > 0;
  }

  dispose() {
    this.generation += 1;
  }

  importPackage(authority: ThemeSnapshot) {
    return this.track(this.runImport(authority));
  }

  select(authority: ThemeSnapshot, themeId: string, mode: ThemeMode) {
    const theme = authority.themes.find((candidate) => candidate.manifest.id === themeId);
    if (!theme || !theme.manifest.modes.includes(mode)) {
      return Promise.resolve<ThemeMutationOutcome>({
        status: "failed",
        snapshot: authority,
        retry: { kind: "select", themeId, mode },
        message: "主题不存在或不支持所选模式，已保持数据库权威主题。",
      });
    }
    const candidate = { ...authority, currentThemeId: themeId, currentMode: mode };
    return this.track(this.runMutation(
      authority,
      { kind: "select", themeId, mode },
      { kind: "select", themeId, mode },
      candidate,
      () => this.operations.service.select(themeId, mode, authority.revision),
    ));
  }

  delete(authority: ThemeSnapshot, themeId: string) {
    if ((READRAY_BUILTIN_THEME_IDS as readonly string[]).includes(themeId)) {
      return Promise.resolve<ThemeMutationOutcome>({
        status: "failed",
        snapshot: authority,
        retry: { kind: "delete", themeId },
        message: "ReadRay 内置主题不能删除。",
      });
    }
    const optimistic = authority.currentThemeId === themeId
      ? { ...authority, currentThemeId: READRAY_DEFAULT_THEME_ID, currentMode: "light" as const }
      : undefined;
    return this.track(this.runMutation(
      authority,
      { kind: "delete", themeId },
      { kind: "delete", themeId },
      optimistic,
      () => this.operations.service.delete(themeId, authority.revision),
    ));
  }

  retry(authority: ThemeSnapshot, retry: ThemeMutationRetry) {
    switch (retry.kind) {
      case "import":
        return this.importPackage(authority);
      case "select":
        return this.select(authority, retry.themeId, retry.mode);
      case "delete":
        return this.delete(authority, retry.themeId);
    }
  }

  async flush() {
    const failures: string[] = [];
    while (this.activeMutations.size) {
      const outcomes = await Promise.all([...this.activeMutations]);
      for (const outcome of outcomes) {
        if (outcome.status === "failed" || outcome.status === "conflict") {
          failures.push(outcome.message);
        }
      }
    }
    if (failures.length) throw new Error([...new Set(failures)].join("；"));
  }

  private track(mutation: Promise<ThemeMutationOutcome>) {
    this.activeMutations.add(mutation);
    void mutation.finally(() => this.activeMutations.delete(mutation));
    return mutation;
  }

  private async runImport(previousAuthority: ThemeSnapshot): Promise<ThemeMutationOutcome> {
    const generation = this.beginMutation();
    try {
      let selection: ThemePackageSelection;
      try {
        const prepared = await this.operations.service.prepareImport();
        if (generation !== this.generation) return { status: "superseded" };
        if (prepared === null) return { status: "cancelled" };
        selection = prepared;
      } catch (error) {
        return await this.reconcileWithoutCommit(
          generation,
          previousAuthority,
          { kind: "import" },
          error,
        );
      }

      const target = { kind: "import", theme: selection.theme } as const;
      try {
        const saved = await this.operations.service.importPreparedPackage(
          selection,
          previousAuthority.revision,
        );
        return this.confirmDirectResult(generation, saved, target);
      } catch (error) {
        return await this.reconcile(
          generation,
          previousAuthority,
          { kind: "import" },
          target,
          error,
        );
      }
    } finally {
      this.pendingRequests -= 1;
    }
  }

  private async runMutation(
    previousAuthority: ThemeSnapshot,
    retry: ThemeMutationRetry,
    target: ThemeMutationTarget,
    optimistic: ThemeSnapshot | undefined,
    mutate: () => Promise<ThemeSnapshot>,
  ): Promise<ThemeMutationOutcome> {
    const generation = this.beginMutation();
    try {
      try {
        if (optimistic) this.operations.apply(optimistic);
        const saved = await mutate();
        return this.confirmDirectResult(generation, saved, target);
      } catch (error) {
        return await this.reconcile(generation, previousAuthority, retry, target, error);
      }
    } finally {
      this.pendingRequests -= 1;
    }
  }

  private beginMutation() {
    const generation = this.generation + 1;
    this.generation = generation;
    this.pendingRequests += 1;
    return generation;
  }

  private confirmDirectResult(
    generation: number,
    saved: ThemeSnapshot,
    target: ThemeMutationTarget,
  ): ThemeMutationOutcome {
    if (generation !== this.generation) return { status: "superseded" };
    this.operations.apply(saved);
    return { status: "saved", snapshot: saved, mutation: mutationReceipt(target) };
  }

  private async reconcile(
    generation: number,
    previousAuthority: ThemeSnapshot,
    retry: ThemeMutationRetry,
    target: ThemeMutationTarget,
    error: unknown,
  ): Promise<ThemeMutationOutcome> {
    const reloaded = await this.reloadAuthority(generation, previousAuthority, retry, error);
    if ("outcome" in reloaded) return reloaded.outcome;
    const { authority, restoreError } = reloaded;
    if (mutationWasCommitted(previousAuthority, authority, target) && !restoreError) {
      return {
        status: "saved",
        snapshot: authority,
        mutation: mutationReceipt(target),
      };
    }
    if (sameValue(authority, previousAuthority)) {
      return {
        status: "failed",
        snapshot: authority,
        retry,
        message: this.failureMessage(error, undefined, restoreError),
      };
    }
    return {
      status: "conflict",
      snapshot: authority,
      message: `主题操作未能确认：数据库已发生其他主题更新，已恢复最新权威主题；为避免覆盖并发修改，本次不会自动重试。原始错误：${errorMessage(error)}${
        restoreError ? `；重新应用失败：${restoreError}` : ""
      }`,
    };
  }

  private async reconcileWithoutCommit(
    generation: number,
    previousAuthority: ThemeSnapshot,
    retry: ThemeMutationRetry,
    error: unknown,
  ): Promise<ThemeMutationOutcome> {
    const reloaded = await this.reloadAuthority(generation, previousAuthority, retry, error);
    if ("outcome" in reloaded) return reloaded.outcome;
    const { authority, restoreError } = reloaded;
    if (sameValue(authority, previousAuthority)) {
      return {
        status: "failed",
        snapshot: authority,
        retry,
        message: this.failureMessage(error, undefined, restoreError),
      };
    }
    return {
      status: "conflict",
      snapshot: authority,
      message: `主题导入预检失败，同时数据库已发生其他主题更新；已恢复最新权威主题，本次不会自动重试。原始错误：${errorMessage(error)}${
        restoreError ? `；重新应用失败：${restoreError}` : ""
      }`,
    };
  }

  private async reloadAuthority(
    generation: number,
    previousAuthority: ThemeSnapshot,
    retry: ThemeMutationRetry,
    error: unknown,
  ): Promise<
    | { authority: ThemeSnapshot; restoreError?: string }
    | { outcome: ThemeMutationOutcome }
  > {
    if (generation !== this.generation) return { outcome: { status: "superseded" } };
    let authority: ThemeSnapshot;
    try {
      authority = await this.operations.service.load();
    } catch (readError) {
      if (generation !== this.generation) return { outcome: { status: "superseded" } };
      let restoreError: string | undefined;
      try {
        this.operations.apply(previousAuthority);
      } catch (applyError) {
        restoreError = errorMessage(applyError);
      }
      return {
        outcome: {
          status: "failed",
          snapshot: previousAuthority,
          retry,
          message: this.failureMessage(error, errorMessage(readError), restoreError),
        },
      };
    }
    if (generation !== this.generation) return { outcome: { status: "superseded" } };
    let restoreError: string | undefined;
    try {
      this.operations.apply(authority);
    } catch (applyError) {
      restoreError = errorMessage(applyError);
    }
    return { authority, restoreError };
  }

  private failureMessage(error: unknown, reloadError?: string, restoreError?: string) {
    return `主题操作失败，已恢复数据库权威主题：${errorMessage(error)}${
      reloadError ? `；重新读取失败：${reloadError}` : ""
    }${restoreError ? `；重新应用失败：${restoreError}` : ""}`;
  }
}
