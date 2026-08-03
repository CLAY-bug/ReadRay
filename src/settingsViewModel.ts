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
