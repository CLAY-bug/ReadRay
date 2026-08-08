import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { listen } from "@tauri-apps/api/event";
import { desktopSaveCoordinator } from "./desktopLifecycle";
import { getPrefetchedThemeSnapshot } from "./themePrefetch.ts";
import type { ThemeService } from "./themeService.ts";
import {
  ThemeMutationCoordinator,
  type ThemeMutationOutcome,
  type ThemeMutationRetry,
} from "./themeMutationCoordinator.ts";
import {
  applyThemeVariables,
  DEFAULT_THEME_SNAPSHOT,
  validateThemeSnapshot,
  type ThemeMode,
  type ThemeSnapshot,
} from "./themeProtocol.ts";

export type AppThemeController = {
  snapshot: ThemeSnapshot;
  status: "loading" | "ready" | "error";
  error?: string;
  reload(): Promise<void>;
  importPackage(): Promise<ThemeMutationOutcome>;
  select(themeId: string, mode: ThemeMode): Promise<ThemeMutationOutcome>;
  delete(themeId: string): Promise<ThemeMutationOutcome>;
  retry(retry: ThemeMutationRetry): Promise<ThemeMutationOutcome>;
};

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export function useAppTheme(service: ThemeService | null): AppThemeController {
  const [snapshot, setSnapshot] = useState<ThemeSnapshot>(DEFAULT_THEME_SNAPSHOT);
  const [status, setStatus] = useState<"loading" | "ready" | "error">(
    service ? "loading" : "ready",
  );
  const [error, setError] = useState<string>();
  const requestKeyRef = useRef(0);
  const mountedRef = useRef(false);
  const reloadPendingRef = useRef(false);
  const snapshotRef = useRef(snapshot);

  const apply = useCallback((next: ThemeSnapshot) => {
    const validated = validateThemeSnapshot(next);
    const mainApp = document.querySelector<HTMLElement>(".rr-main-app");
    if (!mainApp) throw new Error("ReadRay 主应用尚未就绪，无法应用主题。");
    applyThemeVariables(mainApp.style, validated);
    snapshotRef.current = validated;
    setSnapshot(validated);
    setStatus("ready");
    setError(undefined);
  }, []);

  const coordinator = useState(() =>
    service ? new ThemeMutationCoordinator({ service, apply }) : null,
  )[0];

  const reload = useCallback(async () => {
    if (!service) return;
    if (coordinator?.isSaving) {
      reloadPendingRef.current = true;
      return;
    }
    const requestKey = requestKeyRef.current + 1;
    requestKeyRef.current = requestKey;
    setStatus("loading");
    try {
      const next = await service.load();
      if (!mountedRef.current || requestKey !== requestKeyRef.current) return;
      apply(next);
    } catch (loadError) {
      if (!mountedRef.current || requestKey !== requestKeyRef.current) return;
      setStatus("error");
      setError(errorMessage(loadError));
    }
  }, [apply, coordinator, service]);

  const run = useCallback(async (
    start: (authority: ThemeSnapshot) => Promise<ThemeMutationOutcome>,
  ) => {
    if (!coordinator) {
      return {
        status: "failed",
        snapshot: snapshotRef.current,
        retry: { kind: "import" },
        message: "主题仅能在 ReadRay 桌面应用中管理。",
      } satisfies ThemeMutationOutcome;
    }
    try {
      return await start(snapshotRef.current);
    } finally {
      if (!coordinator.isSaving && reloadPendingRef.current) {
        reloadPendingRef.current = false;
        void reload();
      }
    }
  }, [coordinator, reload]);

  // 用 useLayoutEffect：在浏览器首次绘制前应用预取主题，
  // 使 `.rr-main-app` 首帧即为已选主题（不再先绘制 CSS 硬编码默认值）。
  useLayoutEffect(() => {
    mountedRef.current = true;
    const prefetched = getPrefetchedThemeSnapshot();
    if (service) {
      if (prefetched) {
        apply(prefetched);
        void reload();
      } else {
        void reload();
      }
    } else {
      apply(DEFAULT_THEME_SNAPSHOT);
    }
    return () => {
      mountedRef.current = false;
      requestKeyRef.current += 1;
      coordinator?.dispose();
    };
  }, [apply, coordinator, reload, service]);

  useEffect(() => {
    if (!coordinator) return;
    return desktopSaveCoordinator.register({
      label: "主题保存",
      flush: () => coordinator.flush(),
    });
  }, [coordinator]);

  useEffect(() => {
    if (!service) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen("readray://theme-updated", () => void reload())
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch((listenError) => {
        if (!disposed) console.error("ReadRay 主题更新监听失败：", listenError);
      });
    function reloadWhenVisible() {
      if (document.visibilityState === "visible") void reload();
    }
    window.addEventListener("focus", reloadWhenVisible);
    document.addEventListener("visibilitychange", reloadWhenVisible);
    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener("focus", reloadWhenVisible);
      document.removeEventListener("visibilitychange", reloadWhenVisible);
    };
  }, [reload, service]);

  return {
    snapshot,
    status,
    error,
    reload,
    importPackage: () => run((authority) => coordinator!.importPackage(authority)),
    select: (themeId, mode) => run((authority) => coordinator!.select(authority, themeId, mode)),
    delete: (themeId) => run((authority) => coordinator!.delete(authority, themeId)),
    retry: (retry) => run((authority) => coordinator!.retry(authority, retry)),
  };
}
