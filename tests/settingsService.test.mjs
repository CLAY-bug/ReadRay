import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { TauriSettingsRepository } from "../src/settingsRepository.ts";
import {
  RepositorySettingsService,
  validateDatabaseBackupResult,
  validateDeepSeekBalance,
  validateModelUsageSummary,
  validateSettingsSnapshot,
} from "../src/settingsService.ts";
import {
  formatLocalCalendarDate,
  isSettingsOperationCurrent,
  modelUsageRangeBounds,
  suggestedBackupFileName,
  validateApiKeyDraft,
} from "../src/settingsViewModel.ts";
import {
  BALANCE_RETRY_INTERVAL_MS,
  BALANCE_REFRESH_INTERVAL_MS,
  BalanceRefreshController,
  reduceBalanceRefreshState,
} from "../src/settingsBalanceRefresh.ts";
import {
  DEFAULT_APP_PREFERENCES,
  appPreferenceCssVariables,
  parseFontSizeCandidate,
  shortcutBindingIdentity,
  shortcutBindingParts,
  shouldSendMultilineMessage,
  validateAppPreferences,
  validateShortcutBinding,
} from "../src/appPreferences.ts";
import { AppPreferenceSaveCoordinator } from "../src/appPreferenceSaveCoordinator.ts";

function preferences(overrides = {}) {
  return { ...DEFAULT_APP_PREFERENCES, ...overrides };
}

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
    preferences: preferences(),
    autostartEnabled: false,
    shortcutRegistrationError: null,
    ...overrides,
  };
}

function usageSummary(overrides = {}) {
  return {
    promptTokens: 67,
    completionTokens: 28,
    totalTokens: 95,
    requestCount: 4,
    statisticsStartUnixMs: 1_700_000_000_000,
    categories: [
      {
        category: "explanation_query",
        promptTokens: 10,
        completionTokens: 5,
        totalTokens: 15,
        requestCount: 1,
      },
      {
        category: "quick_ai",
        promptTokens: 20,
        completionTokens: 8,
        totalTokens: 28,
        requestCount: 1,
      },
      {
        category: "writing",
        promptTokens: 30,
        completionTokens: 12,
        totalTokens: 42,
        requestCount: 1,
      },
      {
        category: "review_card",
        promptTokens: 7,
        completionTokens: 3,
        totalTokens: 10,
        requestCount: 1,
      },
    ],
    ...overrides,
  };
}

class FakeBalanceScheduler {
  now = 0;
  nextHandle = 1;
  tasks = new Map();

  set(callback, delayMs) {
    const handle = this.nextHandle;
    this.nextHandle += 1;
    this.tasks.set(handle, { callback, dueAt: this.now + delayMs });
    return handle;
  }

  clear(handle) {
    this.tasks.delete(handle);
  }

  advance(delayMs) {
    this.now += delayMs;
    while (true) {
      const next = [...this.tasks.entries()]
        .filter(([, task]) => task.dueAt <= this.now)
        .sort((left, right) => left[1].dueAt - right[1].dueAt)[0];
      if (!next) return;
      this.tasks.delete(next[0]);
      next[1].callback();
    }
  }
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function flushAsyncWork() {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
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
      if (command === "get_model_usage_summary") {
        return usageSummary();
      }
      if (command === "backup_readray_database") {
        return {
          fileName: "ReadRay.sqlite3",
          filePath: "D:\\Backups\\ReadRay.sqlite3",
          byteSize: 4096,
          createdAtUnixMs: 1_700_000_000_000,
        };
      }
      if (command === "get_app_preferences") {
        return preferences();
      }
      if (command === "update_app_preferences") {
        return preferences({ ...args.preferences, revision: args.preferences.revision + 1 });
      }
      if (command === "get_autostart_enabled") return false;
      if (command === "set_autostart_enabled") return args.enabled;
      return snapshot();
    },
    async (options) => {
      dialogCalls.push(options);
      return "D:\\Backups\\ReadRay.sqlite3";
    },
  );

  await repository.get();
  await repository.getPreferences();
  await repository.updatePreferences(preferences({ uiFontSize: 16 }));
  await repository.beginShortcutRecording("quickQuery");
  await repository.submitShortcutRecordingKeyEvent("ControlLeft", true);
  await repository.cancelShortcutRecording();
  await repository.getAutostartEnabled();
  await repository.setAutostartEnabled(true);
  await repository.validateAndSaveApiKey("candidate-secret");
  await repository.clearApiKey();
  await repository.getBalance();
  await repository.getUsage(1_700_000_000_000, 1_700_604_800_000);
  await repository.openDataDirectory();
  await repository.backupDatabase("ReadRay-backup.sqlite3");

  assert.deepEqual(calls, [
    { command: "get_settings_snapshot", args: undefined },
    { command: "get_app_preferences", args: undefined },
    {
      command: "update_app_preferences",
      args: { preferences: preferences({ uiFontSize: 16 }) },
    },
    { command: "begin_shortcut_recording", args: { action: "quickQuery" } },
    {
      command: "submit_shortcut_recording_key_event",
      args: { code: "ControlLeft", keyDown: true },
    },
    { command: "cancel_shortcut_recording", args: undefined },
    { command: "get_autostart_enabled", args: undefined },
    { command: "set_autostart_enabled", args: { enabled: true } },
    {
      command: "validate_and_save_deepseek_api_key",
      args: { apiKey: "candidate-secret" },
    },
    { command: "clear_deepseek_api_key", args: undefined },
    { command: "get_deepseek_balance", args: undefined },
    {
      command: "get_model_usage_summary",
      args: {
        startUnixMs: 1_700_000_000_000,
        endUnixMs: 1_700_604_800_000,
      },
    },
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
    getPreferences: async () => preferences(),
    updatePreferences: async (next) => ({ ...next, revision: next.revision + 1 }),
    getAutostartEnabled: async () => false,
    setAutostartEnabled: async (enabled) => enabled,
    validateAndSaveApiKey: async (apiKey) => {
      saved.push(apiKey);
      return snapshot({ apiKeyConfigured: true, apiKeySource: "credential" });
    },
    clearApiKey: async () => snapshot(),
    getBalance: async () => ({ isAvailable: true, balances: [] }),
    getUsage: async () => usageSummary(),
    openDataDirectory: async () => {},
    backupDatabase: async () => null,
  });

  assert.equal((await service.loadSettings()).learningRecordCount, 3);
  assert.equal((await service.loadPreferences()).uiFontSize, 14);
  assert.equal(await service.loadAutostartEnabled(), false);
  assert.equal(await service.setAutostartEnabled(true), true);
  assert.equal(
    (await service.savePreferences(preferences({ learningFontSize: 19 }))).revision,
    1,
  );
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

test("偏好设置校验、字体作用域与两种发送方式保持确定语义", () => {
  assert.equal(parseFontSizeCandidate("12", 12, 20), 12);
  assert.equal(parseFontSizeCandidate("20", 12, 20), 20);
  assert.equal(parseFontSizeCandidate("11", 12, 20), undefined);
  assert.equal(parseFontSizeCandidate("21", 12, 20), undefined);
  assert.equal(parseFontSizeCandidate("14.5", 12, 20), undefined);
  assert.equal(parseFontSizeCandidate("1e1", 12, 20), undefined);
  assert.equal(parseFontSizeCandidate("", 12, 20), undefined);
  assert.equal(parseFontSizeCandidate("not-a-size", 12, 20), undefined);
  assert.throws(
    () => validateAppPreferences(preferences({ uiFontSize: 21 })),
    /12–20/,
  );
  assert.throws(
    () => validateAppPreferences(preferences({ learningFontSize: 13 })),
    /14–24/,
  );
  assert.equal(
    shortcutBindingIdentity(DEFAULT_APP_PREFERENCES.quickQueryBinding),
    "chord:Alt+Super+Space",
  );
  assert.deepEqual(
    shortcutBindingParts(DEFAULT_APP_PREFERENCES.quickQueryBinding),
    ["Alt", "Win", "Space"],
  );
  assert.deepEqual(
    shortcutBindingParts(DEFAULT_APP_PREFERENCES.selectionExplanationBinding),
    ["左 Alt", "×2"],
  );
  assert.throws(
    () => validateShortcutBinding(
      { version: 2, kind: "chord", accelerator: "Space" },
      "快速查询",
    ),
    /必须包含修饰键/,
  );
  assert.throws(
    () => validateAppPreferences(preferences({
      quickQueryBinding: DEFAULT_APP_PREFERENCES.selectionExplanationBinding,
    })),
    /不能使用同一个快捷键/,
  );
  const variables = appPreferenceCssVariables(
    preferences({
      uiFont: "sourceHanSans",
      uiFontSize: 16,
      learningFont: "sourceHanSerif",
      learningFontSize: 19,
    }),
  );
  assert.match(variables["--rr-ui-font-family"], /Source Han Sans/);
  assert.doesNotMatch(variables["--rr-ui-font-family"], /Geist/);
  assert.match(variables["--rr-learning-font-family"], /Source Han Serif/);
  assert.doesNotMatch(variables["--rr-learning-font-family"], /Newsreader/);
  assert.equal(variables["--rr-ui-font-size"], "16px");
  assert.equal(variables["--rr-learning-font-size"], "19px");
  assert.equal(variables["--rr-ui-font-scale"], String(16 / 14));
  assert.equal(variables["--rr-learning-font-scale"], String(19 / 17));

  const key = (overrides = {}) => ({
    key: "Enter",
    shiftKey: false,
    ctrlKey: false,
    isComposing: false,
    ...overrides,
  });
  assert.equal(shouldSendMultilineMessage(key(), "enter"), true);
  assert.equal(
    shouldSendMultilineMessage(key({ shiftKey: true }), "enter"),
    false,
  );
  assert.equal(shouldSendMultilineMessage(key(), "ctrlEnter"), false);
  assert.equal(
    shouldSendMultilineMessage(key({ ctrlKey: true }), "ctrlEnter"),
    true,
  );
  assert.equal(
    shouldSendMultilineMessage(key({ ctrlKey: true, isComposing: true }), "ctrlEnter"),
    false,
  );
});

test("设置页卸载后保存失败仍由持久协调器恢复全局数据库偏好", async () => {
  const pendingSave = deferred();
  const authority = preferences({ revision: 4, uiFontSize: 14 });
  const candidate = preferences({ revision: 4, uiFontSize: 18 });
  const applied = [];
  const coordinator = new AppPreferenceSaveCoordinator({
    save: () => pendingSave.promise,
    load: async () => authority,
    apply: (next) => applied.push(next),
  });
  let pageMounted = true;
  let pageStateUpdates = 0;
  const outcomePromise = coordinator.save(candidate, authority).then((outcome) => {
    if (pageMounted) pageStateUpdates += 1;
    return outcome;
  });

  assert.equal(applied.at(-1).uiFontSize, 18, "候选值先全局乐观应用");
  pageMounted = false;
  pendingSave.reject(new Error("database is locked"));
  const outcome = await outcomePromise;

  assert.equal(outcome.status, "failed");
  assert.equal(applied.at(-1).uiFontSize, 14, "页面卸载后仍恢复 SQLite 权威值");
  assert.equal(pageStateUpdates, 0, "卸载页面不接收保存结果状态");
});

test("旧保存迟到失败不得覆盖较新保存成功后的全局偏好", async () => {
  const oldSave = deferred();
  const authority = preferences({ revision: 8, uiFontSize: 14 });
  const oldCandidate = preferences({ revision: 8, uiFontSize: 15 });
  const newCandidate = preferences({ revision: 8, uiFontSize: 19 });
  const savedNew = preferences({ revision: 9, uiFontSize: 19 });
  const applied = [];
  let reloads = 0;
  const coordinator = new AppPreferenceSaveCoordinator({
    save: (next) =>
      next.uiFontSize === oldCandidate.uiFontSize
        ? oldSave.promise
        : Promise.resolve(savedNew),
    load: async () => {
      reloads += 1;
      return authority;
    },
    apply: (next) => applied.push(next),
  });

  const oldOutcomePromise = coordinator.save(oldCandidate, authority);
  const newOutcome = await coordinator.save(newCandidate, authority);
  assert.equal(newOutcome.status, "saved");
  oldSave.reject(new Error("late failure"));
  const oldOutcome = await oldOutcomePromise;

  assert.equal(oldOutcome.status, "superseded");
  assert.equal(reloads, 0, "被取代的失败不得再读取回滚快照");
  assert.equal(applied.at(-1).uiFontSize, 19);
  assert.equal(applied.at(-1).revision, 9);
});

test("安全退出会等待进行中的设置保存并传播保存失败", async () => {
  const pendingSave = deferred();
  const coordinator = new AppPreferenceSaveCoordinator({
    save: () => pendingSave.promise,
    load: async () => preferences(),
    apply: () => {},
  });
  const saving = coordinator.save(
    preferences({ uiFontSize: 16 }),
    preferences(),
  );
  let flushed = false;
  const flushing = coordinator.flush().then(() => {
    flushed = true;
  });
  await flushAsyncWork();
  assert.equal(flushed, false);
  pendingSave.resolve(preferences({ revision: 1, uiFontSize: 16 }));
  await saving;
  await flushing;
  assert.equal(flushed, true);

  const failing = new AppPreferenceSaveCoordinator({
    save: async () => { throw new Error("SQLite 写入失败"); },
    load: async () => preferences(),
    apply: () => {},
  });
  void failing.save(preferences({ uiFontSize: 18 }), preferences());
  await assert.rejects(() => failing.flush(), /SQLite 写入失败/);
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
    getUsage: async () => usageSummary(),
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

test("余额进入栏目自动查询、五分钟刷新，手动刷新会重新计时", async () => {
  const scheduler = new FakeBalanceScheduler();
  const events = [];
  let calls = 0;
  const controller = new BalanceRefreshController(
    async () => ({ request: ++calls }),
    (event) => events.push(event),
    scheduler,
  );

  controller.updateContext({ active: false, visible: true, apiKeyConfigured: true });
  controller.updateContext({ active: true, visible: true, apiKeyConfigured: false });
  assert.equal(calls, 0, "未进入栏目或未配置 Key 时不得查询");

  controller.updateContext({ active: true, visible: true, apiKeyConfigured: true });
  assert.equal(calls, 1, "进入已配置 Key 的栏目立即查询");
  await flushAsyncWork();
  assert.deepEqual(events.at(-1), { type: "success", value: { request: 1 } });

  scheduler.advance(BALANCE_REFRESH_INTERVAL_MS - 1);
  assert.equal(calls, 1);
  scheduler.advance(1);
  assert.equal(calls, 2, "成功后五分钟自动刷新");
  await flushAsyncWork();

  scheduler.advance(BALANCE_REFRESH_INTERVAL_MS / 2);
  assert.equal(controller.refreshNow(), true);
  assert.equal(calls, 3, "手动刷新立即查询");
  await flushAsyncWork();
  scheduler.advance(BALANCE_REFRESH_INTERVAL_MS - 1);
  assert.equal(calls, 3, "手动刷新后从完成时重新计时");
  scheduler.advance(1);
  assert.equal(calls, 4);
  controller.dispose();
});

test("余额首次查询失败后短暂自动重试，成功后恢复五分钟刷新", async () => {
  const scheduler = new FakeBalanceScheduler();
  const events = [];
  let calls = 0;
  const controller = new BalanceRefreshController(
    async () => {
      calls += 1;
      if (calls === 1) throw new Error("temporary balance failure");
      return { request: calls };
    },
    (event) => events.push(event),
    scheduler,
  );

  controller.updateContext({ active: true, visible: true, apiKeyConfigured: true });
  await flushAsyncWork();
  assert.equal(calls, 1);
  assert.equal(events.at(-1).type, "error");

  scheduler.advance(BALANCE_RETRY_INTERVAL_MS - 1);
  assert.equal(calls, 1, "首次失败的短暂重试不应提前触发");
  scheduler.advance(1);
  assert.equal(calls, 2, "首次失败后应在短暂延迟后自动重试");
  await flushAsyncWork();
  assert.deepEqual(events.at(-1), { type: "success", value: { request: 2 } });

  scheduler.advance(BALANCE_REFRESH_INTERVAL_MS - 1);
  assert.equal(calls, 2, "成功后的刷新仍按五分钟计时");
  controller.dispose();
});

test("已有 Key 更新新 Key 后立即查询，并拒绝旧 Key 的迟到余额", async () => {
  const scheduler = new FakeBalanceScheduler();
  const requests = [];
  const events = [];
  let calls = 0;
  let activeRequests = 0;
  let maxActiveRequests = 0;
  let state = { status: "idle" };
  const controller = new BalanceRefreshController(
    () => {
      calls += 1;
      activeRequests += 1;
      maxActiveRequests = Math.max(maxActiveRequests, activeRequests);
      const request = deferred();
      requests.push(request);
      return request.promise.finally(() => {
        activeRequests -= 1;
      });
    },
    (event) => {
      events.push(event);
      state = reduceBalanceRefreshState(state, event);
    },
    scheduler,
  );
  const configuredContext = {
    active: true,
    visible: true,
    apiKeyConfigured: true,
  };

  assert.equal(controller.replaceCredential(configuredContext), true);
  assert.equal(calls, 1, "首次配置立即查询");
  requests[0].resolve({ account: "old-key" });
  await flushAsyncWork();
  assert.deepEqual(state, { status: "success", value: { account: "old-key" } });

  assert.equal(controller.refreshNow(), true);
  assert.equal(calls, 2);
  assert.deepEqual(state.value, { account: "old-key" });
  assert.equal(controller.replaceCredential(configuredContext), true);
  assert.deepEqual(state, { status: "idle" }, "更新成功后立即清除旧账户余额");
  assert.equal(calls, 2, "旧请求仍在途时不得与新 Key 查询重叠");

  requests[1].resolve({ account: "late-old-key" });
  await flushAsyncWork();
  assert.equal(calls, 3, "旧请求结束后立即使用新 Key 查询");
  assert.equal(
    events.some(
      (event) => event.type === "success" && event.value.account === "late-old-key",
    ),
    false,
  );
  assert.equal(state.value, undefined, "新 Key 返回前不得恢复旧账户余额");
  requests[2].resolve({ account: "new-key" });
  await flushAsyncWork();
  assert.deepEqual(state, { status: "success", value: { account: "new-key" } });

  assert.equal(
    controller.replaceCredential({ ...configuredContext, apiKeyConfigured: false }),
    false,
  );
  assert.deepEqual(state, { status: "idle" }, "清除 Key 后清空余额并停止查询");
  scheduler.advance(BALANCE_REFRESH_INTERVAL_MS);
  assert.equal(calls, 3);
  assert.equal(maxActiveRequests, 1);
  controller.dispose();
});

test("余额隐藏和卸载清理计时，失败保留旧值且迟到请求不覆盖", async () => {
  const scheduler = new FakeBalanceScheduler();
  const requests = [];
  let calls = 0;
  let activeRequests = 0;
  let maxActiveRequests = 0;
  let state = { status: "idle" };
  const events = [];
  const controller = new BalanceRefreshController(
    () => {
      calls += 1;
      activeRequests += 1;
      maxActiveRequests = Math.max(maxActiveRequests, activeRequests);
      const request = deferred();
      requests.push(request);
      return request.promise.finally(() => {
        activeRequests -= 1;
      });
    },
    (event) => {
      events.push(event);
      state = reduceBalanceRefreshState(state, event);
    },
    scheduler,
  );

  controller.updateContext({ active: true, visible: true, apiKeyConfigured: true });
  assert.equal(calls, 1);
  controller.updateContext({ active: true, visible: false, apiKeyConfigured: true });
  controller.updateContext({ active: true, visible: true, apiKeyConfigured: true });
  assert.equal(calls, 1, "窗口重新可见时等待仍在途的请求，不能重叠");

  requests[0].resolve({ id: "stale" });
  await flushAsyncWork();
  assert.equal(calls, 2, "旧请求结束后才发起新的可见窗口请求");
  assert.equal(events.some((event) => event.value?.id === "stale"), false);
  requests[1].resolve({ id: "fresh" });
  await flushAsyncWork();
  assert.deepEqual(state, { status: "success", value: { id: "fresh" } });

  scheduler.advance(BALANCE_REFRESH_INTERVAL_MS);
  assert.equal(calls, 3);
  requests[2].reject(new Error("temporary balance failure"));
  await flushAsyncWork();
  assert.equal(state.status, "error");
  assert.deepEqual(state.value, { id: "fresh" });
  assert.match(String(state.error), /temporary balance failure/);

  controller.updateContext({ active: true, visible: false, apiKeyConfigured: true });
  scheduler.advance(BALANCE_REFRESH_INTERVAL_MS);
  assert.equal(calls, 3, "窗口隐藏后清除失败重试计时器");
  controller.updateContext({ active: true, visible: true, apiKeyConfigured: true });
  assert.equal(calls, 4, "窗口重新可见时立即更新");
  requests[3].resolve({ id: "after-visible" });
  await flushAsyncWork();

  controller.updateContext({ active: false, visible: true, apiKeyConfigured: true });
  scheduler.advance(BALANCE_REFRESH_INTERVAL_MS);
  assert.equal(calls, 4, "离开栏目后停止定时器");
  controller.updateContext({ active: true, visible: true, apiKeyConfigured: true });
  assert.equal(calls, 5);
  const eventCountBeforeDispose = events.length;
  controller.dispose();
  requests[4].resolve({ id: "after-unmount" });
  await flushAsyncWork();
  scheduler.advance(BALANCE_REFRESH_INTERVAL_MS);
  assert.equal(events.length, eventCountBeforeDispose, "卸载后忽略迟到结果");
  assert.equal(calls, 5, "卸载后不再自动查询");
  assert.equal(maxActiveRequests, 1, "任意时刻最多一个余额请求");
});

test("使用量 service 映射四类真实统计，并允许读取失败后重试", async () => {
  let attempts = 0;
  const bounds = [];
  const service = new RepositorySettingsService({
    get: async () => snapshot(),
    validateAndSaveApiKey: async () => snapshot(),
    clearApiKey: async () => snapshot(),
    getBalance: async () => ({ isAvailable: true, balances: [] }),
    getUsage: async (startUnixMs, endUnixMs) => {
      bounds.push({ startUnixMs, endUnixMs });
      attempts += 1;
      if (attempts === 1) throw new Error("database temporarily locked");
      return usageSummary({
        categories: usageSummary().categories.slice().reverse(),
      });
    },
    openDataDirectory: async () => {},
    backupDatabase: async () => null,
  });

  await assert.rejects(() => service.loadUsage("last7Days"), /temporarily locked/);
  const summary = await service.loadUsage("last7Days");
  assert.equal(attempts, 2);
  assert.deepEqual(
    summary.categories.map((item) => item.category),
    ["explanation_query", "quick_ai", "writing", "review_card"],
  );
  assert.equal(summary.totalTokens, 95);
  assert.equal(bounds.length, 2);
  assert.ok(Number.isSafeInteger(bounds[0].startUnixMs));
  assert.ok(Number.isSafeInteger(bounds[0].endUnixMs));

  assert.throws(() =>
    validateModelUsageSummary(
      usageSummary({ totalTokens: 86 }),
    ),
  );
  assert.throws(() =>
    validateModelUsageSummary(
      usageSummary({ categories: usageSummary().categories.slice(0, 2) }),
    ),
  );
});

test("本机日历范围和备份名在中国时区零点附近不会使用 UTC 日期", () => {
  const beforeMidnight = new Date("2026-08-03T15:59:59.999Z");
  const atMidnight = new Date("2026-08-03T16:00:00.000Z");

  assert.equal(formatLocalCalendarDate(beforeMidnight, 480), "2026-08-03");
  assert.equal(formatLocalCalendarDate(atMidnight, 480), "2026-08-04");
  assert.equal(
    suggestedBackupFileName(beforeMidnight, 480),
    "ReadRay-backup-2026-08-03.sqlite3",
  );
  assert.equal(
    suggestedBackupFileName(atMidnight, 480),
    "ReadRay-backup-2026-08-04.sqlite3",
  );

  assert.deepEqual(modelUsageRangeBounds("today", beforeMidnight, 480), {
    startUnixMs: Date.parse("2026-08-02T16:00:00.000Z"),
    endUnixMs: Date.parse("2026-08-03T16:00:00.000Z"),
  });
  assert.deepEqual(modelUsageRangeBounds("today", atMidnight, 480), {
    startUnixMs: Date.parse("2026-08-03T16:00:00.000Z"),
    endUnixMs: Date.parse("2026-08-04T16:00:00.000Z"),
  });
  assert.deepEqual(modelUsageRangeBounds("last7Days", atMidnight, 480), {
    startUnixMs: Date.parse("2026-07-28T16:00:00.000Z"),
    endUnixMs: Date.parse("2026-08-04T16:00:00.000Z"),
  });
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
    getUsage: async () => usageSummary(),
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

test("四处多行输入共享发送偏好与 IME 守卫，单行解释仍固定 Enter", async () => {
  const multilineFiles = [
    "src/components/TodayPage.tsx",
    "src/components/ConversationPage.tsx",
    "src/components/QuickAiPanel.tsx",
    "src/components/WritingCoach.tsx",
  ];
  for (const file of multilineFiles) {
    const content = await readFile(file, "utf8");
    assert.match(content, /shouldSendMultilineMessage/);
    assert.match(content, /nativeEvent\.isComposing/);
    assert.match(content, /sendShortcut/);
  }
  const singleLine = await readFile("src/components/CenteredCommandInput.tsx", "utf8");
  assert.doesNotMatch(singleLine, /sendShortcut/);
  assert.match(singleLine, /event\.key === "Enter"/);
});

test("正式设置页保持四类设置结构，确定性操作已接线且不直接 invoke", async () => {
  const page = await readFile("src/components/SettingsPage.tsx", "utf8");
  const balanceRefresh = await readFile("src/settingsBalanceRefresh.ts", "utf8");
  const styles = await readFile("src/styles/settings-page.css", "utf8");
  const overlayStyles = await readFile("src/App.css", "utf8");
  const mainStyles = await readFile("src/styles/main-app.css", "utf8");
  const writingStyles = await readFile("src/styles/writing-page.css", "utf8");
  const repository = await readFile("src/settingsRepository.ts", "utf8");
  const preferenceHook = await readFile("src/useAppPreferences.ts", "utf8");
  const preferenceModel = await readFile("src/appPreferences.ts", "utf8");
  const preferenceCoordinator = await readFile(
    "src/appPreferenceSaveCoordinator.ts",
    "utf8",
  );
  const sidebar = await readFile("src/components/MainSidebar.tsx", "utf8");
  const shell = await readFile("src/components/MainAppShell.tsx", "utf8");
  const mainAppIcon = await readFile("src/components/MainAppIcon.tsx", "utf8");
  const rustSettings = await readFile("src-tauri/src/settings.rs", "utf8");
  const deepSeekClient = await readFile("src-tauri/src/deepseek_client.rs", "utf8");
  const explanation = await readFile("src-tauri/src/deepseek_explanation.rs", "utf8");
  const quickAi = await readFile("src-tauri/src/quick_ai.rs", "utf8");
  const writing = await readFile("src-tauri/src/writing.rs", "utf8");
  const review = await readFile("src-tauri/src/review.rs", "utf8");
  const migrations = await readFile("src-tauri/src/learning_records.rs", "utf8");

  assert.doesNotMatch(page, /\binvoke\s*\(|localStorage|sessionStorage/);
  assert.doesNotMatch(preferenceHook, /\binvoke\s*\(|localStorage|sessionStorage/);
  assert.match(preferenceHook, /service\.loadPreferences/);
  assert.match(preferenceHook, /app-preferences-updated/);
  assert.doesNotMatch(preferenceModel, /queryLocalFonts|navigator\.fonts|localStorage/);
  assert.doesNotMatch(page, /service\.savePreferences|service\.loadPreferences/);
  assert.match(preferenceHook, /new AppPreferenceSaveCoordinator/);
  assert.match(preferenceHook, /service\.savePreferences/);
  assert.match(preferenceHook, /service\.loadPreferences/);
  assert.match(preferenceCoordinator, /generation !== this\.generation/);
  assert.match(preferenceCoordinator, /保存失败，已恢复数据库设置/);
  assert.doesNotMatch(page, /parseFontSizeCandidate/);
  assert.doesNotMatch(page, /界面字体|界面字号|学习内容字体|学习内容字号/);
  assert.doesNotMatch(page, /已保存并应用/);
  assert.doesNotMatch(page, /已启用 Windows 开机启动|已关闭 Windows 开机启动/);
  assert.match(page, /preferenceStatus === "error" && preferenceMessage/);
  assert.match(page, /autostartStatus === "error" && autostartMessage/);
  assert.match(
    page,
    /await onPreferencesSave\(next, previous\)[\s\S]*?isSettingsOperationCurrent\([\s\S]*?setSnapshot/,
  );
  assert.match(page, /finally\s*\{[\s\S]*?current === "loading" \? "error"/);
  assert.match(repository, /validate_and_save_deepseek_api_key/);
  assert.match(repository, /get_app_preferences/);
  assert.match(repository, /update_app_preferences/);
  assert.match(repository, /clear_deepseek_api_key/);
  assert.match(repository, /get_deepseek_balance/);
  assert.match(repository, /get_model_usage_summary/);
  assert.match(repository, /open_readray_data_directory/);
  assert.match(repository, /backup_readray_database/);
  assert.match(repository, /if \(!filePath\) \{[\s\S]*?return null/);
  assert.match(shell, /<SettingsPage[\s\S]*?service=\{settingsService\}[\s\S]*?onPreferencesSave=\{onPreferencesSave\}/);
  assert.match(page, /\["general", "通用"\]/);
  assert.doesNotMatch(page, /\["appearance", "外观"\]/);
  assert.doesNotMatch(page, /activeSection === "appearance"/);
  assert.match(page, /activeSection === "general"[\s\S]*?GroupHeading title="主题"/);
  assert.match(page, /\["ai", "AI 服务"\]/);
  assert.match(page, /\["data", "数据"\]/);
  assert.match(page, /\["about", "关于"\]/);
  assert.match(page, /themeController\.select/);
  assert.doesNotMatch(page, /themeController\.importPackage/);
  assert.doesNotMatch(page, /themeController\.delete/);
  assert.doesNotMatch(page, /ReadRay 内置主题不能删除/);
  assert.match(page, /录制新快捷键/);
  assert.match(page, /listenShortcutRecording/);
  assert.match(page, /beginShortcutRecording/);
  assert.match(page, /submitShortcutRecordingKeyEvent/);
  assert.match(page, /cancelShortcutRecording/);
  assert.match(page, /addEventListener\("keydown", handleKeyDown, true\)/);
  assert.match(page, /addEventListener\("keyup", handleKeyUp, true\)/);
  assert.match(page, /recordButtonRef\.current\?\.focus\(\{ preventScroll: true \}\)/);
  assert.match(page, /disabled=\{disabled\}[\s\S]*?aria-pressed=\{recording\}/);
  assert.doesNotMatch(page, /disabled=\{disabled \|\| recording\}[\s\S]*?aria-pressed/);
  assert.doesNotMatch(page, /shortcutFromKeyEvent|onKeyDown=\{recordShortcut\}/);
  assert.match(repository, /begin_shortcut_recording/);
  assert.match(repository, /submit_shortcut_recording_key_event/);
  assert.match(repository, /readray:\/\/shortcut-recorded/);
  assert.match(page, /恢复默认快捷键/);
  assert.match(page, /onClick=\{refreshBalance\}/);
  assert.match(page, /new BalanceRefreshController/);
  assert.match(page, /rr-settings-balance-error/);
  assert.doesNotMatch(page, /rr-settings-balance-meta/);
  assert.equal([...page.matchAll(/\.replaceCredential\(/g)].length, 2);
  assert.match(page, /visibilitychange/);
  assert.match(balanceRefresh, /5 \* 60 \* 1_000/);
  assert.match(balanceRefresh, /3 \* 1_000/);
  assert.doesNotMatch(page, /赠送|充值|尚未查询/);
  assert.match(page, /onClick=\{\(\) => void openDataDirectory\(\)\}/);
  assert.match(page, /onClick=\{\(\) => void createDatabaseBackup\(\)\}/);
  assert.match(page, /重试查询/);
  assert.match(page, /重试打开/);
  assert.match(page, /重试备份/);
  assert.match(page, /重试读取/);
  assert.match(page, /\["today", "今天"\]/);
  assert.match(page, /\["last7Days", "近 7 天"\]/);
  assert.doesNotMatch(page, /toISOString\(\)/);
  assert.match(page, /不包含 API Key/);
  assert.doesNotMatch(page, /余额查询尚未接线|本轮尚未形成可验证闭环/);
  assert.match(page, /onClick=\{\(\) => setShowingLicenses\(true\)\}/);
  assert.match(page, /Geist-OFL\.txt\?raw/);
  assert.match(page, /Source-Han-Serif-OFL\.txt\?raw/);
  assert.match(page, /role="switch"/);
  assert.match(page, /隐藏到托盘/);
  assert.match(page, /autostart/i);
  assert.match(page, /closeBehavior/);
  assert.match(styles, /\.rr-settings-nav\s*\{[\s\S]*?width:\s*auto/);
  assert.doesNotMatch(styles, /rr-settings-font-field|rr-settings-font-size-control|rr-settings-number-field/);
  assert.match(styles, /\.rr-settings-content\s*\{[\s\S]*?width:\s*min\(720px/);
  assert.match(styles, /\.rr-settings-page\s*\{[\s\S]*?--rr-settings-font-scale:\s*calc\(var\(--rr-ui-font-scale\) \* 0\.94\)/);
  assert.match(styles, /\.rr-settings-row\s*\{[\s\S]*?grid-template-columns:\s*minmax\(0, 1fr\) auto[\s\S]*?min-height:\s*64px/);
  assert.match(styles, /\.rr-settings-label\s*\{[\s\S]*?font-weight:\s*570/);
  assert.match(styles, /\.rr-settings-shortcut-name\s*\{[\s\S]*?font-weight:\s*570/);
  assert.doesNotMatch(styles, /\.rr-settings-stack-control\s*\{[\s\S]*?min-width:\s*280px/);
  assert.match(styles, /\.rr-settings-path-actions\s*\{/);
  assert.match(styles, /\.rr-settings-header h1\s*\{[\s\S]*?font-size:\s*calc\(30px \* var\(--rr-settings-font-scale\)\)/);
  assert.match(styles, /\.rr-settings-button:hover:not\(:disabled\)\s*\{[\s\S]*?box-shadow:\s*none/);
  assert.doesNotMatch(styles, /\.rr-settings-button:hover:not\(:disabled\)\s*\{[^}]*color:\s*var\(--rr-main-danger\)/);
  assert.match(page, /function SettingsSelect/);
  assert.doesNotMatch(page, /<select\b/);
  assert.match(styles, /\.rr-settings-select-menu\s*\{/);
  assert.match(
    styles,
    /\.rr-settings-select-menu\s*\{[\s\S]*?box-shadow:\s*0 12px 28px color-mix\(in oklab, var\(--rr-main-shadow\), transparent 48%\)/,
  );
  assert.match(styles, /\.rr-settings-link-row\s*\{[\s\S]*?border:\s*0 !important/);
  assert.match(styles, /\.rr-settings-link-row\s*\{[\s\S]*?border-radius:\s*0 !important/);
  assert.match(styles, /\.rr-settings-link-row\s*\{[\s\S]*?min-height:\s*54px !important/);
  assert.match(styles, /@container \(max-width:\s*1100px\)/);
  assert.match(styles, /@container \(max-width:\s*900px\)/);
  assert.match(overlayStyles, /\.app-shell\s*\{[\s\S]*?font-size:\s*calc\(16px \* var\(--rr-ui-font-scale\)\)/);
  assert.match(mainStyles, /\.rr-main-app\s*\{[\s\S]*?font-size:\s*calc\(16px \* var\(--rr-ui-font-scale\)\)/);
  assert.match(mainStyles, /\.rr-main-app\s*\{[\s\S]*?min-width:\s*0/);
  assert.match(mainStyles, /\.rr-main-app\s*\{[^}]*?user-select:\s*none/);
  assert.match(mainStyles, /\.rr-main-app\s+:where\(input,\s*textarea,\s*select,\s*\[contenteditable\]\)\s*\{[^}]*?user-select:\s*text/);
  assert.match(sidebar, /className="rr-main-settings-footer"/);
  assert.match(mainStyles, /\.rr-main-settings-footer\s*\{[\s\S]*?border-top:/);
  assert.match(mainStyles, /\.rr-main-settings-footer\s*\{[\s\S]*?padding-top:\s*calc\(6px/);
  assert.match(mainStyles, /\.rr-main-settings-footer\s*\{[\s\S]*?transform:\s*translateY\(calc\(3px[\s\S]*?border-top:/);
  assert.match(mainStyles, /\.rr-main-settings\s*\{[\s\S]*?min-height:\s*calc\(34px[\s\S]*?border:\s*0 !important[\s\S]*?border-radius:\s*calc\(9px/);
  assert.match(mainStyles, /\.rr-main-settings\.is-active\s*\{[\s\S]*?background:\s*var\(--rr-main-surface-subtle\)/);
  assert.doesNotMatch(mainStyles, /\.rr-main-settings\.is-active::before/);
  assert.match(shell, /className="rr-main-titlebar-leading"[\s\S]*?className="rr-main-brand-zone"[\s\S]*?className="rr-main-brand-icon"[\s\S]*?src="\/branding\/readray-startup-icon\.png"[\s\S]*?className="rr-main-brand-name">ReadRay<[\s\S]*?\{collapseButton\}/);
  assert.match(mainStyles, /\.rr-main-brand-icon\s*\{[\s\S]*?width:\s*calc\(24px[\s\S]*?border-radius:\s*calc\(7px/);
  assert.match(mainStyles, /\.rr-main-collapse\s*\{[\s\S]*?right:\s*calc\(3px/);
  assert.match(mainStyles, /\.rr-main-app\.is-sidebar-collapsed \.rr-main-brand-zone\s*\{[\s\S]*?visibility:\s*hidden[\s\S]*?opacity:\s*0/);
  assert.match(shell, /is-sidebar-peeking/);
  assert.match(shell, /className="rr-main-collapse"[\s\S]*?onPointerEnter=\{handleSidebarPeekEnter\}[\s\S]*?onPointerLeave=\{handleSidebarPeekLeave\}/);
  assert.match(shell, /sidebarEffectiveCollapsed \? "panel-closed" : "panel-open"/);
  assert.match(mainAppIcon, /case "panel-open"[\s\S]*?case "panel-closed"/);
  assert.doesNotMatch(shell, /rr-main-sidebar-peek-trigger/);
  assert.match(sidebar, /onPointerEnter=\{onPeekEnter\}/);
  assert.match(sidebar, /onPointerLeave=\{onPeekLeave\}/);
  assert.match(shell, /className="rr-main-sidebar-slot"[\s\S]*?<MainSidebar/);
  assert.match(mainStyles, /\.rr-main-sidebar-slot\s*\{[\s\S]*?width:\s*var\(--rr-main-sidebar-layout-width\)[\s\S]*?flex:\s*0 0 var\(--rr-main-sidebar-layout-width\)/);
  assert.match(mainStyles, /\.rr-main-app\.is-sidebar-collapsed\s*\{[\s\S]*?--rr-main-sidebar-layout-width:\s*0px/);
  assert.match(mainStyles, /\.rr-main-app\.is-sidebar-collapsed \.rr-main-titlebar-leading\s*\{[\s\S]*?width:\s*calc\(36px/);
  assert.match(mainStyles, /\.rr-main-sidebar\s*\{[\s\S]*?position:\s*absolute[\s\S]*?width:\s*var\(--rr-main-sidebar-width\)[\s\S]*?transform:\s*translateX\(0\)/);
  assert.match(mainStyles, /\.rr-main-app\.is-sidebar-collapsed \.rr-main-sidebar\s*\{[\s\S]*?visibility:\s*hidden[\s\S]*?pointer-events:\s*none[\s\S]*?transform:\s*translateX\(-100%\)/);
  assert.doesNotMatch(mainStyles, /\.rr-main-sidebar-peek-trigger\s*\{/);
  assert.match(mainStyles, /\.rr-main-app\.is-sidebar-collapsed\.is-sidebar-peeking \.rr-main-sidebar\s*\{[\s\S]*?pointer-events:\s*auto[\s\S]*?transform:\s*translateX\(0\)/);
  assert.doesNotMatch(mainStyles, /\.rr-main-app\.is-sidebar-collapsed \.rr-main-collapse svg[\s\S]*?rotate\(180deg\)/);
  assert.match(mainStyles, /\.rr-main-home-content,\s*\.rr-main-composer-inner\s*\{[\s\S]*?width:\s*var\(--rr-main-dialogue-width\)[\s\S]*?margin-inline:\s*auto/);
  assert.match(styles, /@container \(max-width:\s*1100px\)[\s\S]*?\.rr-settings-content\s*\{[\s\S]*?width:\s*min\(720px,\s*calc\(100% - 40px\)\)/);
  assert.match(styles, /@container \(max-width:\s*900px\)[\s\S]*?\.rr-settings-content\s*\{[\s\S]*?width:\s*min\(720px,\s*calc\(100% - 28px\)\)/);
  assert.match(mainStyles, /\.rr-main-sidebar\s*\{[\s\S]*?--rr-main-sidebar-nav-font-size:\s*max\(12px, calc\(var\(--rr-ui-font-size\) - 1px\)\)/);
  assert.match(mainStyles, /\.rr-main-nav-item\.is-active,[\s\S]*?font-weight:\s*500/);
  assert.match(mainStyles, /\.rr-main-recent-item\.is-active\s*\{[\s\S]*?font-weight:\s*450/);
  assert.match(mainStyles, /\.rr-main-recent-item\.is-active::before\s*\{/);
  assert.match(mainStyles, /\.rr-main-section-label\s*\{[\s\S]*?font-family:\s*var\(--rr-main-sidebar-font-display\)[\s\S]*?letter-spacing:\s*0/);
  assert.match(writingStyles, /\.rr-writing-editor-title\s*\{[\s\S]*?font-family:\s*var\(--rr-learning-font-family\)[\s\S]*?font-size:\s*calc\(32px \* var\(--rr-learning-font-scale\)\)/);
  assert.match(writingStyles, /\.rr-writing-article-editor\s*\{[\s\S]*?font-family:\s*var\(--rr-learning-font-family\)/);
  assert.match(writingStyles, /\.rr-writing-pattern-grid p\s*\{[\s\S]*?font-family:\s*var\(--rr-learning-font-family\)[\s\S]*?var\(--rr-learning-font-scale\)/);
  assert.match(writingStyles, /\.rr-writing-coach-head h2\s*\{[\s\S]*?font-family:\s*var\(--rr-ui-font-family\)/);
  assert.match(writingStyles, /\.rr-writing-agent-empty button\s*\{[\s\S]*?font-family:\s*var\(--rr-ui-font-family\)[\s\S]*?var\(--rr-ui-font-scale\)/);
  assert.doesNotMatch(writingStyles, /ReadRay Source Han (?:Serif|Sans)/);
  const snapshotStruct = rustSettings.slice(
    rustSettings.indexOf("pub struct SettingsSnapshot"),
    rustSettings.indexOf("struct DataCounts"),
  );
  assert.doesNotMatch(snapshotStruct, /api_key:\s*&'static str|api_key:\s*String/);
  assert.match(deepSeekClient, /decode_tracked_chat_completion_value/);
  assert.match(explanation, /ModelUsageCategory::ExplanationQuery/);
  assert.match(quickAi, /ModelUsageCategory::QuickAi/);
  assert.match(writing, /ModelUsageCategory::Writing/);
  assert.match(review, /ModelUsageCategory::ReviewCard/);
  assert.match(migrations, /CREATE TABLE model_usage_records/);
  assert.doesNotMatch(
    migrations.slice(
      migrations.indexOf("CREATE TABLE model_usage_records"),
      migrations.indexOf("const MIGRATIONS"),
    ),
    /prompt_text|answer|api_key/i,
  );
  const candidateKeyBoundary = deepSeekClient.slice(
    deepSeekClient.indexOf("pub(crate) async fn post_chat_completion_with_api_key"),
    deepSeekClient.indexOf("async fn send_chat_completion_value"),
  );
  assert.doesNotMatch(candidateKeyBoundary, /record_for_app|ModelUsageCategory/);
});

test("主窗口检测未配置 API Key 并提供直达 AI 设置的非模态入口", async () => {
  const shell = await readFile("src/components/MainAppShell.tsx", "utf8");
  const settingsPage = await readFile("src/components/SettingsPage.tsx", "utf8");
  const setupCard = await readFile("src/components/ApiKeySetupCard.tsx", "utf8");
  const themeHook = await readFile("src/useAppTheme.ts", "utf8");
  const styles = await readFile("src/styles/main-app.css", "utf8");
  const settingsStyles = await readFile("src/styles/settings-page.css", "utf8");

  assert.match(shell, /settingsService\.loadSettings\(\)/);
  assert.match(shell, /setApiKeyConfigured\(snapshot\.apiKeyConfigured\)/);
  assert.match(shell, /activePageId !== "settings"/);
  assert.match(shell, /handleOpenApiKeySettings/);
  assert.match(shell, /setSettingsInitialSection\("ai"\)/);
  assert.match(shell, /initialSection=\{settingsInitialSection\}/);
  assert.match(shell, /onApiKeyConfiguredChange=\{handleApiKeyConfiguredChange\}/);
  assert.match(settingsPage, /initialSection\?: SettingsSection/);
  assert.match(settingsPage, /onApiKeyConfiguredChange\?: \(configured: boolean\)/);
  assert.match(settingsPage, /onApiKeyConfiguredChange\?\.\(nextSnapshot\.apiKeyConfigured\)/);
  assert.match(setupCard, /先配置 AI 服务/);
  assert.match(setupCard, /配置 API Key/);
  assert.match(setupCard, /稍后再说/);
  assert.match(styles, /\.rr-main-api-key-setup-card\s*\{/);
  assert.match(styles, /bottom: calc\(68px \* var\(--rr-main-design-scale\)\)/);
  assert.match(styles, /left: calc\(12px \* var\(--rr-main-design-scale\)\)/);
  assert.match(styles, /width: min\(360px, calc\(100% - 24px\)\)/);
  assert.match(styles, /flex-direction: column/);
  assert.match(styles, /padding-left: 38px/);
  assert.match(styles, /background: color-mix\(in oklab, var\(--rr-main-bg\), var\(--rr-main-surface\) 34%\)/);
  assert.match(settingsStyles, /\.rr-settings-text-input:focus-visible[\s\S]*box-shadow: 0 0 0 2px/);
  assert.match(settingsPage, /useState<ModelUsageRange>\("today"\)/);
});

test("主题下拉导航只预览配色，确认或取消后分别保存或恢复权威主题", async () => {
  const page = await readFile("src/components/SettingsPage.tsx", "utf8");
  const themeHook = await readFile("src/useAppTheme.ts", "utf8");

  assert.match(page, /onPreview=\{\(value\) => previewTheme\(value\)\}/);
  assert.match(page, /onPreviewCancel=\{restoreThemePreview\}/);
  assert.match(page, /const committed = onChange\(option\.value\)/);
  assert.match(page, /if \(committed === false\) onPreviewCancel\?\.\(\)/);
  assert.match(page, /event\.key === "ArrowDown" \|\| event\.key === "ArrowUp"/);
  assert.match(page, /scrollIntoView\(\{ block: "nearest" \}\)/);
  assert.match(themeHook, /preview\(themeId: string, mode: ThemeMode\): void/);
  assert.match(themeHook, /restorePreview\(\): void/);
  assert.match(themeHook, /不推进 snapshotRef 或 SQLite 权威状态/);
});
