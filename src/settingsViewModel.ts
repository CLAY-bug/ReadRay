export type ApiKeySource = "credential" | "environment" | "none";

export type SettingsSnapshot = {
  apiKeyConfigured: boolean;
  apiKeySource: ApiKeySource;
  model: string;
  appDataDirectory: string;
  learningRecordCount: number;
  conversationCount: number;
  writingDocumentCount: number;
  appVersion: string;
};

export type DeepSeekCurrencyBalance = {
  currency: string;
  totalBalance: string;
  grantedBalance: string;
  toppedUpBalance: string;
};

export type DeepSeekBalance = {
  isAvailable: boolean;
  balances: DeepSeekCurrencyBalance[];
};

export type DatabaseBackupResult = {
  fileName: string;
  filePath: string;
  byteSize: number;
  createdAtUnixMs: number;
};

export type ModelUsageRange = "today" | "last7Days" | "last30Days" | "all";

export type ModelUsageCategory = "explanation_query" | "quick_ai" | "writing";

export type ModelUsageCategorySummary = {
  category: ModelUsageCategory;
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  requestCount: number;
};

export type ModelUsageSummary = {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  requestCount: number;
  statisticsStartUnixMs: number | null;
  categories: ModelUsageCategorySummary[];
};

export type ModelUsageRangeBounds = {
  startUnixMs: number | null;
  endUnixMs: number | null;
};

function requireValidDate(date: Date) {
  if (!Number.isFinite(date.getTime())) {
    throw new Error("本机日期无效。");
  }
}

function requireValidUtcOffset(utcOffsetMinutes: number) {
  if (
    !Number.isInteger(utcOffsetMinutes) ||
    utcOffsetMinutes < -14 * 60 ||
    utcOffsetMinutes > 14 * 60
  ) {
    throw new Error("本机时区偏移无效。");
  }
}

export function formatLocalCalendarDate(
  date: Date,
  utcOffsetMinutes = -date.getTimezoneOffset(),
) {
  requireValidDate(date);
  requireValidUtcOffset(utcOffsetMinutes);
  const local = new Date(date.getTime() + utcOffsetMinutes * 60_000);
  const year = String(local.getUTCFullYear()).padStart(4, "0");
  const month = String(local.getUTCMonth() + 1).padStart(2, "0");
  const day = String(local.getUTCDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function suggestedBackupFileName(
  date = new Date(),
  utcOffsetMinutes = -date.getTimezoneOffset(),
) {
  return `ReadRay-backup-${formatLocalCalendarDate(date, utcOffsetMinutes)}.sqlite3`;
}

export function modelUsageRangeBounds(
  range: ModelUsageRange,
  now = new Date(),
  utcOffsetMinutes?: number,
): ModelUsageRangeBounds {
  requireValidDate(now);
  if (range === "all") {
    return { startUnixMs: null, endUnixMs: null };
  }
  let daysBeforeToday: number;
  switch (range) {
    case "today":
      daysBeforeToday = 0;
      break;
    case "last7Days":
      daysBeforeToday = 6;
      break;
    case "last30Days":
      daysBeforeToday = 29;
      break;
    default:
      throw new Error("未知的 ReadRay 使用量时间范围。");
  }

  if (utcOffsetMinutes !== undefined) {
    requireValidUtcOffset(utcOffsetMinutes);
    const localNow = new Date(now.getTime() + utcOffsetMinutes * 60_000);
    const year = localNow.getUTCFullYear();
    const month = localNow.getUTCMonth();
    const day = localNow.getUTCDate();
    return {
      startUnixMs:
        Date.UTC(year, month, day - daysBeforeToday) - utcOffsetMinutes * 60_000,
      endUnixMs: Date.UTC(year, month, day + 1) - utcOffsetMinutes * 60_000,
    };
  }

  const start = new Date(now);
  start.setHours(0, 0, 0, 0);
  start.setDate(start.getDate() - daysBeforeToday);
  const end = new Date(now);
  end.setHours(0, 0, 0, 0);
  end.setDate(end.getDate() + 1);
  return { startUnixMs: start.getTime(), endUnixMs: end.getTime() };
}

export function validateApiKeyDraft(value: string): string | undefined {
  const trimmed = value.trim();
  if (!trimmed) {
    return "请输入 DeepSeek API Key。";
  }
  if (trimmed.length > 2_048) {
    return "API Key 长度异常，请检查后重试。";
  }
  if (/\s/.test(trimmed) || /[\u0000-\u001f\u007f]/.test(trimmed)) {
    return "API Key 中不能包含空格、换行或控制字符。";
  }
  return undefined;
}

export function isSettingsOperationCurrent(
  mounted: boolean,
  expectedKey: number,
  currentKey: number,
) {
  return mounted && expectedKey === currentKey;
}
