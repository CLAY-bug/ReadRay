import type { SettingsRepository } from "./settingsRepository";
import type { SettingsSnapshot } from "./settingsViewModel";

export interface SettingsService {
  loadSettings(): Promise<SettingsSnapshot>;
  validateAndSaveApiKey(apiKey: string): Promise<SettingsSnapshot>;
  clearApiKey(): Promise<SettingsSnapshot>;
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
}
