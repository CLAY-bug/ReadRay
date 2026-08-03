import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { TauriSettingsRepository } from "../src/settingsRepository.ts";
import {
  RepositorySettingsService,
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

test("Tauri 设置 repository 只通过三个有类型 command 传递 Key", async () => {
  const calls = [];
  const repository = new TauriSettingsRepository(async (command, args) => {
    calls.push({ command, args });
    if (command === "validate_and_save_deepseek_api_key") {
      return snapshot({ apiKeyConfigured: true, apiKeySource: "credential" });
    }
    return snapshot();
  });

  await repository.get();
  await repository.validateAndSaveApiKey("candidate-secret");
  await repository.clearApiKey();

  assert.deepEqual(calls, [
    { command: "get_settings_snapshot", args: undefined },
    {
      command: "validate_and_save_deepseek_api_key",
      args: { apiKey: "candidate-secret" },
    },
    { command: "clear_deepseek_api_key", args: undefined },
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

test("API Key 页面校验与迟到操作守卫覆盖失败重试边界", () => {
  assert.match(validateApiKeyDraft(""), /请输入/);
  assert.match(validateApiKeyDraft("key with space"), /不能包含/);
  assert.equal(validateApiKeyDraft("candidate-secret"), undefined);
  assert.equal(isSettingsOperationCurrent(true, 4, 4), true);
  assert.equal(isSettingsOperationCurrent(false, 4, 4), false);
  assert.equal(isSettingsOperationCurrent(true, 4, 5), false);
});

test("正式设置页保持五类设计结构，未接线操作明确禁用且不直接 invoke", async () => {
  const page = await readFile("src/components/SettingsPage.tsx", "utf8");
  const styles = await readFile("src/styles/settings-page.css", "utf8");
  const repository = await readFile("src/settingsRepository.ts", "utf8");
  const shell = await readFile("src/components/MainAppShell.tsx", "utf8");
  const rustSettings = await readFile("src-tauri/src/settings.rs", "utf8");

  assert.doesNotMatch(page, /\binvoke\s*\(|localStorage|sessionStorage/);
  assert.match(repository, /validate_and_save_deepseek_api_key/);
  assert.match(repository, /clear_deepseek_api_key/);
  assert.match(shell, /<SettingsPage service=\{settingsService\}/);
  assert.match(page, /\["general", "通用"\]/);
  assert.match(page, /\["appearance", "外观"\]/);
  assert.match(page, /\["ai", "AI 服务"\]/);
  assert.match(page, /\["data", "数据"\]/);
  assert.match(page, /\["about", "关于"\]/);
  assert.match(page, /function UnavailableButton[\s\S]*?disabled/);
  assert.match(page, /<UnavailableButton>录制新快捷键<\/UnavailableButton>/);
  assert.match(page, /<UnavailableButton>刷新余额<\/UnavailableButton>/);
  assert.match(page, /<UnavailableButton className="is-primary">开始备份<\/UnavailableButton>/);
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
