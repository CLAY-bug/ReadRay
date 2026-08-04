import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import type { AppPreferences } from "./appPreferences";
import type {
  DatabaseBackupResult,
  DeepSeekBalance,
  ModelUsageSummary,
  SettingsSnapshot,
} from "./settingsViewModel";

export type SettingsInvoke = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

export interface SettingsRepository {
  get(): Promise<SettingsSnapshot>;
  getPreferences(): Promise<AppPreferences>;
  updatePreferences(preferences: AppPreferences): Promise<AppPreferences>;
  validateAndSaveApiKey(apiKey: string): Promise<SettingsSnapshot>;
  clearApiKey(): Promise<SettingsSnapshot>;
  getBalance(): Promise<DeepSeekBalance>;
  getUsage(
    startUnixMs: number | null,
    endUnixMs: number | null,
  ): Promise<ModelUsageSummary>;
  openDataDirectory(): Promise<void>;
  backupDatabase(suggestedFileName: string): Promise<DatabaseBackupResult | null>;
}

export type SettingsSaveDialog = (options: {
  title: string;
  defaultPath: string;
  filters: { name: string; extensions: string[] }[];
}) => Promise<string | null>;

export class TauriSettingsRepository implements SettingsRepository {
  private readonly invokeCommand: SettingsInvoke;

  private readonly saveDialog: SettingsSaveDialog;

  constructor(
    invokeCommand: SettingsInvoke = invoke,
    saveDialog: SettingsSaveDialog = save,
  ) {
    this.invokeCommand = invokeCommand;
    this.saveDialog = saveDialog;
  }

  get() {
    return this.invokeCommand<SettingsSnapshot>("get_settings_snapshot");
  }

  getPreferences() {
    return this.invokeCommand<AppPreferences>("get_app_preferences");
  }

  updatePreferences(preferences: AppPreferences) {
    return this.invokeCommand<AppPreferences>("update_app_preferences", {
      preferences,
    });
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

  getBalance() {
    return this.invokeCommand<DeepSeekBalance>("get_deepseek_balance");
  }

  getUsage(startUnixMs: number | null, endUnixMs: number | null) {
    return this.invokeCommand<ModelUsageSummary>("get_model_usage_summary", {
      startUnixMs,
      endUnixMs,
    });
  }

  openDataDirectory() {
    return this.invokeCommand<void>("open_readray_data_directory");
  }

  async backupDatabase(suggestedFileName: string) {
    const filePath = await this.saveDialog({
      title: "备份 ReadRay 数据",
      defaultPath: suggestedFileName,
      filters: [{ name: "SQLite 数据库", extensions: ["sqlite3"] }],
    });
    if (!filePath) {
      return null;
    }
    return this.invokeCommand<DatabaseBackupResult>("backup_readray_database", {
      filePath,
    });
  }
}
