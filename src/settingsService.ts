import type { SettingsRepository } from "./settingsRepository";
import {
  validateAppPreferences,
  type AppPreferences,
} from "./appPreferences.ts";
import { modelUsageRangeBounds } from "./settingsViewModel.ts";
import type {
  DatabaseBackupResult,
  DeepSeekBalance,
  DeepSeekCurrencyBalance,
  ModelUsageCategory,
  ModelUsageCategorySummary,
  ModelUsageRange,
  ModelUsageSummary,
  SettingsSnapshot,
} from "./settingsViewModel";

export interface SettingsService {
  loadSettings(): Promise<SettingsSnapshot>;
  loadPreferences(): Promise<AppPreferences>;
  savePreferences(preferences: AppPreferences): Promise<AppPreferences>;
  loadAutostartEnabled(): Promise<boolean>;
  setAutostartEnabled(enabled: boolean): Promise<boolean>;
  validateAndSaveApiKey(apiKey: string): Promise<SettingsSnapshot>;
  clearApiKey(): Promise<SettingsSnapshot>;
  loadBalance(): Promise<DeepSeekBalance>;
  loadUsage(range: ModelUsageRange): Promise<ModelUsageSummary>;
  openDataDirectory(): Promise<void>;
  createDatabaseBackup(
    suggestedFileName: string,
  ): Promise<DatabaseBackupResult | null>;
}

function assertCount(value: number, label: string) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(`${label}不是有效的本地数据计数。`);
  }
}

export function validateSettingsSnapshot(
  snapshot: SettingsSnapshot,
): SettingsSnapshot {
  if (
    !["credential", "environment", "none"].includes(snapshot.apiKeySource)
  ) {
    throw new Error("设置返回了未知的 API Key 来源。");
  }
  if (snapshot.apiKeyConfigured === (snapshot.apiKeySource === "none")) {
    throw new Error("设置返回了不一致的 API Key 状态。");
  }
  if (!snapshot.model.trim() || !snapshot.appDataDirectory.trim()) {
    throw new Error("设置返回的运行时信息不完整。");
  }
  assertCount(snapshot.learningRecordCount, "学习记录数");
  assertCount(snapshot.conversationCount, "对话数");
  assertCount(snapshot.writingDocumentCount, "写作文档数");
  validateAppPreferences(snapshot.preferences);
  if (typeof snapshot.autostartEnabled !== "boolean") {
    throw new Error("设置返回了无效的开机启动状态。");
  }
  if (
    snapshot.shortcutRegistrationError !== null &&
    typeof snapshot.shortcutRegistrationError !== "string"
  ) {
    throw new Error("设置返回了无效的全局快捷键注册状态。");
  }
  return snapshot;
}

function isNonNegativeDecimal(value: string) {
  return value.length <= 64 && /^(?:0|[0-9]+)(?:\.[0-9]+)?$/.test(value);
}

export function validateDeepSeekBalance(balance: DeepSeekBalance): DeepSeekBalance {
  if (typeof balance.isAvailable !== "boolean" || !Array.isArray(balance.balances)) {
    throw new Error("DeepSeek 余额返回结构无效。");
  }
  const currencies = new Set<string>();
  const balances = balance.balances.map((item): DeepSeekCurrencyBalance => {
    if (!/^[A-Z]{3}$/.test(item.currency) || currencies.has(item.currency)) {
      throw new Error("DeepSeek 余额返回了无效或重复的币种。");
    }
    currencies.add(item.currency);
    if (
      !isNonNegativeDecimal(item.totalBalance) ||
      !isNonNegativeDecimal(item.grantedBalance) ||
      !isNonNegativeDecimal(item.toppedUpBalance)
    ) {
      throw new Error(`DeepSeek ${item.currency} 余额金额格式无效。`);
    }
    return { ...item };
  });
  return { isAvailable: balance.isAvailable, balances };
}

export function validateDatabaseBackupResult(
  result: DatabaseBackupResult,
): DatabaseBackupResult {
  if (!result.fileName.trim() || !result.filePath.trim()) {
    throw new Error("ReadRay 备份结果缺少文件信息。");
  }
  if (!Number.isSafeInteger(result.byteSize) || result.byteSize <= 0) {
    throw new Error("ReadRay 备份结果包含无效文件大小。");
  }
  if (!Number.isSafeInteger(result.createdAtUnixMs) || result.createdAtUnixMs <= 0) {
    throw new Error("ReadRay 备份结果包含无效创建时间。");
  }
  return result;
}

const usageCategoryOrder: ModelUsageCategory[] = [
  "explanation_query",
  "quick_ai",
  "writing",
];

function validateUsageCounts(
  value: Pick<
    ModelUsageCategorySummary,
    "promptTokens" | "completionTokens" | "totalTokens" | "requestCount"
  >,
  label: string,
) {
  assertCount(value.promptTokens, `${label}输入 Token`);
  assertCount(value.completionTokens, `${label}输出 Token`);
  assertCount(value.totalTokens, `${label}总 Token`);
  assertCount(value.requestCount, `${label}请求次数`);
  if (value.promptTokens + value.completionTokens !== value.totalTokens) {
    throw new Error(`${label}Token 合计不一致。`);
  }
}

export function validateModelUsageSummary(summary: ModelUsageSummary): ModelUsageSummary {
  validateUsageCounts(summary, "ReadRay 使用量");
  if (
    summary.statisticsStartUnixMs !== null &&
    (!Number.isSafeInteger(summary.statisticsStartUnixMs) ||
      summary.statisticsStartUnixMs < 0)
  ) {
    throw new Error("ReadRay 使用量统计开始时间无效。");
  }
  if (!Array.isArray(summary.categories) || summary.categories.length !== 3) {
    throw new Error("ReadRay 使用量必须包含三类业务明细。");
  }

  const categories = new Map<ModelUsageCategory, ModelUsageCategorySummary>();
  for (const item of summary.categories) {
    if (!usageCategoryOrder.includes(item.category) || categories.has(item.category)) {
      throw new Error("ReadRay 使用量包含未知或重复的业务分类。");
    }
    validateUsageCounts(item, `${item.category} `);
    categories.set(item.category, { ...item });
  }
  const ordered = usageCategoryOrder.map((category) => categories.get(category)!);
  const categoryTotals = ordered.reduce(
    (totals, item) => ({
      promptTokens: totals.promptTokens + item.promptTokens,
      completionTokens: totals.completionTokens + item.completionTokens,
      totalTokens: totals.totalTokens + item.totalTokens,
      requestCount: totals.requestCount + item.requestCount,
    }),
    { promptTokens: 0, completionTokens: 0, totalTokens: 0, requestCount: 0 },
  );
  if (
    categoryTotals.promptTokens !== summary.promptTokens ||
    categoryTotals.completionTokens !== summary.completionTokens ||
    categoryTotals.totalTokens !== summary.totalTokens ||
    categoryTotals.requestCount !== summary.requestCount
  ) {
    throw new Error("ReadRay 使用量总览与分类明细不一致。");
  }
  return { ...summary, categories: ordered };
}

export class RepositorySettingsService implements SettingsService {
  private readonly repository: SettingsRepository;

  constructor(repository: SettingsRepository) {
    this.repository = repository;
  }

  async loadSettings() {
    return validateSettingsSnapshot(await this.repository.get());
  }

  async loadPreferences() {
    return validateAppPreferences(await this.repository.getPreferences());
  }

  async savePreferences(preferences: AppPreferences) {
    return validateAppPreferences(
      await this.repository.updatePreferences(validateAppPreferences(preferences)),
    );
  }

  async loadAutostartEnabled() {
    const enabled = await this.repository.getAutostartEnabled();
    if (typeof enabled !== "boolean") {
      throw new Error("Windows 返回了无效的开机启动状态。");
    }
    return enabled;
  }

  async setAutostartEnabled(enabled: boolean) {
    const authoritative = await this.repository.setAutostartEnabled(enabled);
    if (typeof authoritative !== "boolean" || authoritative !== enabled) {
      throw new Error("Windows 开机启动状态与请求不一致。");
    }
    return authoritative;
  }

  async validateAndSaveApiKey(apiKey: string) {
    return validateSettingsSnapshot(
      await this.repository.validateAndSaveApiKey(apiKey.trim()),
    );
  }

  async clearApiKey() {
    return validateSettingsSnapshot(await this.repository.clearApiKey());
  }

  async loadBalance() {
    return validateDeepSeekBalance(await this.repository.getBalance());
  }

  async loadUsage(range: ModelUsageRange) {
    const bounds = modelUsageRangeBounds(range);
    return validateModelUsageSummary(
      await this.repository.getUsage(bounds.startUnixMs, bounds.endUnixMs),
    );
  }

  async openDataDirectory() {
    await this.repository.openDataDirectory();
  }

  async createDatabaseBackup(suggestedFileName: string) {
    const result = await this.repository.backupDatabase(suggestedFileName);
    return result ? validateDatabaseBackupResult(result) : null;
  }
}
