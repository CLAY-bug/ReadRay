export const MAIN_SIDEBAR_DEFAULT_WIDTH = 252;
export const MAIN_SIDEBAR_MIN_WIDTH = 180;
export const MAIN_SIDEBAR_MAX_WIDTH = 360;
export const MAIN_SIDEBAR_WIDTH_STORAGE_KEY = "readray.main.sidebar-width.v1";

type SidebarWidthStorage = Pick<Storage, "getItem" | "setItem">;

type StoredSidebarWidth = {
  version: 1;
  width: number;
};

export function clampMainSidebarWidth(width: number): number {
  return Math.round(
    Math.min(MAIN_SIDEBAR_MAX_WIDTH, Math.max(MAIN_SIDEBAR_MIN_WIDTH, width)),
  );
}

export function parseStoredMainSidebarWidth(value: string | null): number | null {
  if (value === null) return null;
  try {
    const stored = JSON.parse(value) as Partial<StoredSidebarWidth> | null;
    if (
      stored?.version !== 1 ||
      typeof stored.width !== "number" ||
      !Number.isFinite(stored.width)
    ) {
      return null;
    }
    return clampMainSidebarWidth(stored.width);
  } catch {
    return null;
  }
}

function defaultStorage(): SidebarWidthStorage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function loadMainSidebarWidth(
  storage: SidebarWidthStorage | null = defaultStorage(),
): number | null {
  if (!storage) return null;
  try {
    return parseStoredMainSidebarWidth(
      storage.getItem(MAIN_SIDEBAR_WIDTH_STORAGE_KEY),
    );
  } catch {
    return null;
  }
}

export function saveMainSidebarWidth(
  width: number,
  storage: SidebarWidthStorage | null = defaultStorage(),
): void {
  if (!storage || !Number.isFinite(width)) return;
  const stored: StoredSidebarWidth = {
    version: 1,
    width: clampMainSidebarWidth(width),
  };
  try {
    storage.setItem(MAIN_SIDEBAR_WIDTH_STORAGE_KEY, JSON.stringify(stored));
  } catch {
    // WebView 存储不可用时保留当前会话内的宽度，不阻断主界面。
  }
}
