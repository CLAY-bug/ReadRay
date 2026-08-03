import { invoke } from "@tauri-apps/api/core";
import type { SettingsSnapshot } from "./settingsViewModel";

export type SettingsInvoke = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

export interface SettingsRepository {
  get(): Promise<SettingsSnapshot>;
  validateAndSaveApiKey(apiKey: string): Promise<SettingsSnapshot>;
  clearApiKey(): Promise<SettingsSnapshot>;
}

export class TauriSettingsRepository implements SettingsRepository {
  private readonly invokeCommand: SettingsInvoke;

  constructor(invokeCommand: SettingsInvoke = invoke) {
    this.invokeCommand = invokeCommand;
  }

  get() {
    return this.invokeCommand<SettingsSnapshot>("get_settings_snapshot");
  }

  validateAndSaveApiKey(apiKey: string) {
    return this.invokeCommand<SettingsSnapshot>(
      "validate_and_save_deepseek_api_key",
      { apiKey },
    );
  }

  clearApiKey() {
    return this.invokeCommand<SettingsSnapshot>("clear_deepseek_api_key");
  }
}
