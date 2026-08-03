import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { TauriSettingsRepository } from "../src/settingsRepository.ts";
import {
  RepositorySettingsService,
  validateDatabaseBackupResult,
  validateDeepSeekBalance,
  validateSettingsSnapshot,
} from "../src/settingsService.ts";
import {
  isSettingsOperationCurrent,
  validateApiKeyDraft,
} from "../src/settingsViewModel.ts";

function snapshot(overrides = {}) {
  return {
    apiKeyConfigured: false,
    apiKeySource: "none",
    model: "deepseek-v4-flash",
    appDataDirectory: "C:\\Users\\tester\\AppData\\Roaming\\com.readray.app",
    learningRecordCount: 3,
    conversationCount: 2,
    writingDocumentCount: 1,
    appVersion: "0.1.0",
    ...overrides,
  };
}

test("Tauri 设置 repository 通过有类型 command 和原生保存对话框完成正式操作", async () => {
  const calls = [];
  const dialogCalls = [];
  const repository = new TauriSettingsRepository(
    async (command, args) => {
      calls.push({ command, args });
      if (command === "validate_and_save_deepseek_api_key") {
        return snapshot({ apiKeyConfigured: true, apiKeySource: "credential" });
      }
      if (command === "get_deepseek_balance") {
        return { isAvailable: true, balances: [] };
      }
      if (command === "backup_readray_database") {
        return {
          fileName: "ReadRay.sqlite3",
          filePath: "D:\\Backups\\ReadRay.sqlite3",
          byteSize: 4096,
          createdAtUnixMs: 1_700_000_000_000,
        };
      }
      return snapshot();
    },
    async (options) => {
      dialogCalls.push(options);
      return "D:\\Backups\\ReadRay.sqlite3";
    },
  );

  await repository.get();
  await repository.validateAndSaveApiKey("candidate-secret");
  await repository.clearApiKey();
  await repository.getBalance();
  await repository.openDataDirectory();
  await repository.backupDatabase("ReadRay-backup.sqlite3");

  assert.deepEqual(calls, [
    { command: "get_settings_snapshot", args: undefined },
    {
      command: "validate_and_save_deepseek_api_key",
      args: { apiKey: "candidate-secret" },
    },
    { command: "clear_deepseek_api_key", args: undefined },
    { command: "get_deepseek_balance", args: undefined },
    { command: "open_readray_data_directory", args: undefined },
    {
      command: "backup_readray_database",
      args: { filePath: "D:\\Backups\\ReadRay.sqlite3" },
    },
  ]);
  assert.deepEqual(dialogCalls, [
    {
      title: "备份 ReadRay 数据",
      defaultPath: "ReadRay-backup.sqlite3",
      filters: [{ name: "SQLite 数据库", extensions: ["sqlite3"] }],
    },
  ]);
});

test("service 规范化输入并拒绝不一致的运行时快照", async () => {
  const saved = [];
  const service = new RepositorySettingsService({
    get: async () => snapshot(),
    validateAndSaveApiKey: async (apiKey) => {
      saved.push(apiKey);
      return snapshot({ apiKeyConfigured: true, apiKeySource: "credential" });
    },
    clearApiKey: async () => snapshot(),
    getBalance: async () => ({ isAvailable: true, balances: [] }),
    openDataDirectory: async () => {},
    backupDatabase: async () => null,
  });

  assert.equal((await service.loadSettings()).learningRecordCount, 3);
  assert.equal((await service.validateAndSaveApiKey("  candidate-secret  ")).apiKeySource, "credential");
  assert.deepEqual(saved, ["candidate-secret"]);
  assert.throws(() =>
    validateSettingsSnapshot(
      snapshot({ apiKeyConfigured: true, apiKeySource: "none" }),
    ),
  );
  assert.throws(() =>
    validateSettingsSnapshot(snapshot({ conversationCount: -1 })),
  );
});

test("余额 service 严格映射多币种，并允许失败后重新请求", async () => {
  let attempts = 0;
  const service = new RepositorySettingsService({
    get: async () => snapshot(),
    validateAndSaveApiKey: async () => snapshot(),
    clearApiKey: async () => snapshot(),
    getBalance: async () => {
      attempts += 1;
      if (attempts === 1) throw new Error("temporary network failure");
      return {
        isAvailable: true,
        balances: [
          {
            currency: "CNY",
            totalBalance: "110.00",
            grantedBalance: "10.00",
            toppedUpBalance: "100.00",
          },
          {
            currency: "USD",
            totalBalance: "3.25",
            grantedBalance: "0.25",
            toppedUpBalance: "3.00",
          },
        ],
      };
    },
    openDataDirectory: async () => {},
    backupDatabase: async () => null,
  });

  await assert.rejects(() => service.loadBalance(), /temporary network failure/);
  const balance = await service.loadBalance();
  assert.equal(attempts, 2);
  assert.deepEqual(balance.balances.map((item) => item.currency), ["CNY", "USD"]);
  assert.throws(() =>
    validateDeepSeekBalance({
      isAvailable: true,
      balances: [
        {
          currency: "CNY",
          totalBalance: "-1.00",
          grantedBalance: "0.00",
          toppedUpBalance: "0.00",
        },
      ],
    }),
  );
  assert.throws(() =>
    validateDeepSeekBalance({
      isAvailable: true,
      balances: [
        {
          currency: "USD",
          totalBalance: "1.00",
          grantedBalance: "0.00",
          toppedUpBalance: "1.00",
        },
        {
          currency: "USD",
          totalBalance: "2.00",
          grantedBalance: "0.00",
          toppedUpBalance: "2.00",
        },
      ],
    }),
  );
});

test("目录打开失败可重试，备份取消不会调用 Rust 或报告成功", async () => {
  const invokeCalls = [];
  const cancelledRepository = new TauriSettingsRepository(
    async (command, args) => {
      invokeCalls.push({ command, args });
      return snapshot();
    },
    async () => null,
  );
  assert.equal(await cancelledRepository.backupDatabase("ReadRay.sqlite3"), null);
  assert.deepEqual(invokeCalls, []);

  let openAttempts = 0;
  const service = new RepositorySettingsService({
    get: async () => snapshot(),
    validateAndSaveApiKey: async () => snapshot(),
    clearApiKey: async () => snapshot(),
    getBalance: async () => ({ isAvailable: true, balances: [] }),
    openDataDirectory: async () => {
      openAttempts += 1;
      if (openAttempts === 1) throw new Error("Explorer unavailable");
    },
    backupDatabase: async () => null,
  });
  await assert.rejects(() => service.openDataDirectory(), /Explorer unavailable/);
  await service.openDataDirectory();
  assert.equal(openAttempts, 2);

  assert.throws(() =>
    validateDatabaseBackupResult({
      fileName: "partial.sqlite3",
      filePath: "D:\\partial.sqlite3",
      byteSize: 0,
      createdAtUnixMs: 1_700_000_000_000,
    }),
  );
});

test("API Key 页面校验与迟到操作守卫覆盖失败重试边界", () => {
  assert.match(validateApiKeyDraft(""), /请输入/);
  assert.match(validateApiKeyDraft("key with space"), /不能包含/);
  assert.equal(validateApiKeyDraft("candidate-secret"), undefined);
  assert.equal(isSettingsOperationCurrent(true, 4, 4), true);
  assert.equal(isSettingsOperationCurrent(false, 4, 4), false);
  assert.equal(isSettingsOperationCurrent(true, 4, 5), false);
});

test("正式设置页保持五类设计结构，确定性操作已接线且不直接 invoke", async () => {
  const page = await readFile("src/components/SettingsPage.tsx", "utf8");
  const styles = await readFile("src/styles/settings-page.css", "utf8");
  const repository = await readFile("src/settingsRepository.ts", "utf8");
  const shell = await readFile("src/components/MainAppShell.tsx", "utf8");
  const rustSettings = await readFile("src-tauri/src/settings.rs", "utf8");

  assert.doesNotMatch(page, /\binvoke\s*\(|localStorage|sessionStorage/);
  assert.match(repository, /validate_and_save_deepseek_api_key/);
  assert.match(repository, /clear_deepseek_api_key/);
  assert.match(repository, /get_deepseek_balance/);
  assert.match(repository, /open_readray_data_directory/);
  assert.match(repository, /backup_readray_database/);
  assert.match(repository, /if \(!filePath\) \{[\s\S]*?return null/);
  assert.match(shell, /<SettingsPage service=\{settingsService\}/);
  assert.match(page, /\["general", "通用"\]/);
  assert.match(page, /\["appearance", "外观"\]/);
  assert.match(page, /\["ai", "AI 服务"\]/);
  assert.match(page, /\["data", "数据"\]/);
  assert.match(page, /\["about", "关于"\]/);
  assert.match(page, /function UnavailableButton[\s\S]*?disabled/);
  assert.match(page, /<UnavailableButton>录制新快捷键<\/UnavailableButton>/);
  assert.match(page, /onClick=\{\(\) => void refreshBalance\(\)\}/);
  assert.match(page, /onClick=\{\(\) => void openDataDirectory\(\)\}/);
  assert.match(page, /onClick=\{\(\) => void createDatabaseBackup\(\)\}/);
  assert.match(page, /重试查询/);
  assert.match(page, /重试打开/);
  assert.match(page, /重试备份/);
  assert.match(page, /不包含 API Key/);
  assert.doesNotMatch(page, /余额查询尚未接线|本轮尚未形成可验证闭环/);
  assert.match(page, /onClick=\{\(\) => setShowingLicenses\(true\)\}/);
  assert.match(page, /Geist-OFL\.txt\?raw/);
  assert.match(page, /Source-Han-Serif-OFL\.txt\?raw/);
  assert.doesNotMatch(page, /role="switch"|隐藏到托盘|autostart|closeBehavior/i);
  assert.match(styles, /\.rr-settings-nav\s*\{[\s\S]*?width:\s*192px/);
  assert.match(styles, /\.rr-settings-content\s*\{[\s\S]*?width:\s*min\(820px/);
  assert.match(styles, /\.rr-settings-row\s*\{[\s\S]*?min-height:\s*82px/);
  assert.match(styles, /\.rr-settings-header h1\s*\{[\s\S]*?font-size:\s*30px/);
  assert.match(styles, /\.rr-settings-link-row\s*\{[\s\S]*?border:\s*0 !important/);
  assert.match(styles, /\.rr-settings-link-row\s*\{[\s\S]*?border-radius:\s*0 !important/);
  assert.match(styles, /\.rr-settings-link-row\s*\{[\s\S]*?min-height:\s*54px !important/);
  assert.match(styles, /@container \(max-width:\s*1100px\)/);
  assert.match(styles, /@container \(max-width:\s*900px\)/);
  const snapshotStruct = rustSettings.slice(
    rustSettings.indexOf("pub struct SettingsSnapshot"),
    rustSettings.indexOf("struct DataCounts"),
  );
  assert.doesNotMatch(snapshotStruct, /api_key:\s*&'static str|api_key:\s*String/);
});
