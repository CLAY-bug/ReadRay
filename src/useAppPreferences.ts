import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  DEFAULT_APP_PREFERENCES,
  appPreferenceCssVariables,
  validateAppPreferences,
  type AppPreferences,
} from "./appPreferences";
import type { SettingsService } from "./settingsService";
import {
  AppPreferenceSaveCoordinator,
  type AppPreferenceSaveOutcome,
} from "./appPreferenceSaveCoordinator";

export function useAppPreferences(service: SettingsService | null) {
  const [preferences, setPreferences] = useState<AppPreferences>(
    DEFAULT_APP_PREFERENCES,
  );
  const requestKeyRef = useRef(0);
  const mountedRef = useRef(false);
  const reloadPendingRef = useRef(false);

  const applyPreferences = useCallback((next: AppPreferences) => {
    setPreferences(validateAppPreferences(next));
  }, []);

  const saveCoordinator = useState(() =>
    service
      ? new AppPreferenceSaveCoordinator({
          save: (next) => service.savePreferences(next),
          load: () => service.loadPreferences(),
          apply: (next) => {
            requestKeyRef.current += 1;
            applyPreferences(next);
          },
        })
      : null,
  )[0];

  const reload = useCallback(async () => {
    if (!service) return;
    if (saveCoordinator?.isSaving) {
      reloadPendingRef.current = true;
      return;
    }
    const requestKey = requestKeyRef.current + 1;
    requestKeyRef.current = requestKey;
    try {
      const next = await service.loadPreferences();
      if (!mountedRef.current || requestKey !== requestKeyRef.current) return;
      applyPreferences(next);
    } catch (error) {
      if (!mountedRef.current || requestKey !== requestKeyRef.current) return;
      console.error("ReadRay 偏好设置读取失败：", error);
    }
  }, [applyPreferences, saveCoordinator, service]);

  const savePreferences = useCallback(
    async (
      candidate: AppPreferences,
      previousAuthority: AppPreferences,
    ): Promise<AppPreferenceSaveOutcome> => {
      if (!saveCoordinator) {
        return {
          status: "failed",
          preferences: previousAuthority,
          retryPreferences: candidate,
          message: "设置仅在 ReadRay 桌面应用中可保存。",
        };
      }
      try {
        return await saveCoordinator.save(candidate, previousAuthority);
      } finally {
        if (!saveCoordinator.isSaving && reloadPendingRef.current) {
          reloadPendingRef.current = false;
          void reload();
        }
      }
    },
    [reload, saveCoordinator],
  );

  useEffect(() => {
    mountedRef.current = true;
    void reload();
    return () => {
      mountedRef.current = false;
      requestKeyRef.current += 1;
      saveCoordinator?.dispose();
    };
  }, [reload, saveCoordinator]);

  useEffect(() => {
    if (!service) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen("readray://app-preferences-updated", () => {
      void reload();
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch((error) => {
      if (!disposed) {
        console.error("ReadRay 偏好设置更新监听失败：", error);
      }
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

  useEffect(() => {
    const style = document.documentElement.style;
    const variables = appPreferenceCssVariables(preferences);
    for (const [name, value] of Object.entries(variables)) {
      style.setProperty(name, value);
    }
  }, [preferences]);

  return { preferences, reload, savePreferences };
}
