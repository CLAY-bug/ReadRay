import type { SettingsRepository } from "./settingsRepository";
import type {
  DatabaseBackupResult,
  DeepSeekBalance,
  DeepSeekCurrencyBalance,
  SettingsSnapshot,
} from "./settingsViewModel";

export interface SettingsService {
  loadSettings(): Promise<SettingsSnapshot>;
  validateAndSaveApiKey(apiKey: string): Promise<SettingsSnapshot>;
  clearApiKey(): Promise<SettingsSnapshot>;
  loadBalance(): Promise<DeepSeekBalance>;
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

export class RepositorySettingsService implements SettingsService {
  private readonly repository: SettingsRepository;

  constructor(repository: SettingsRepository) {
    this.repository = repository;
  }

  async loadSettings() {
    return validateSettingsSnapshot(await this.repository.get());
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

  async openDataDirectory() {
    await this.repository.openDataDirectory();
  }

  async createDatabaseBackup(suggestedFileName: string) {
    const result = await this.repository.backupDatabase(suggestedFileName);
    return result ? validateDatabaseBackupResult(result) : null;
  }
}
