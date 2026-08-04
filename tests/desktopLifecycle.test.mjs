import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import {
  DesktopSaveCoordinator,
  runForcedExit,
  runSafeExit,
  shortcutFromKeyEvent,
} from "../src/desktopLifecycle.ts";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

test("快捷键录制拒绝裸按键并生成稳定组合键", () => {
  assert.throws(
    () => shortcutFromKeyEvent({
      code: "KeyR",
      ctrlKey: false,
      altKey: false,
      shiftKey: false,
      metaKey: false,
    }),
    /裸按键/,
  );
  assert.equal(
    shortcutFromKeyEvent({
      code: "KeyR",
      ctrlKey: true,
      altKey: true,
      shiftKey: false,
      metaKey: false,
    }),
    "Ctrl+Alt+R",
  );
  assert.equal(
    shortcutFromKeyEvent({
      code: "ControlLeft",
      ctrlKey: true,
      altKey: false,
      shiftKey: false,
      metaKey: false,
    }),
    undefined,
  );
});

test("安全退出等待所有保存并在成功后直接退出", async () => {
  const saves = new DesktopSaveCoordinator();
  const calls = [];
  saves.register({ label: "设置保存", flush: async () => calls.push("settings") });
  saves.register({ label: "写作草稿", flush: async () => calls.push("writing") });
  const outcome = await runSafeExit(7, {
    flush: () => saves.flushAll(),
    complete: async (requestId) => calls.push(`exit:${requestId}`),
    isCurrent: () => true,
  });
  assert.deepEqual(outcome, { status: "exited" });
  assert.deepEqual(calls, ["settings", "writing", "exit:7"]);
});

test("保存失败取消退出并返回具体错误供重试", async () => {
  let completed = false;
  const outcome = await runSafeExit(8, {
    flush: async () => {
      throw new Error("文章 42：磁盘已满");
    },
    complete: async () => {
      completed = true;
    },
    isCurrent: () => true,
  });
  assert.equal(outcome.status, "failed");
  assert.match(outcome.message, /文章 42：磁盘已满/);
  assert.equal(completed, false);
});

test("设置操作切页后仍由应用级协调器等待并报告失败", async () => {
  const saves = new DesktopSaveCoordinator();
  const operation = deferred();
  let pageMounted = true;
  let pageStateUpdates = 0;
  const pageResult = saves
    .runMutation("开机启动操作", () => operation.promise)
    .catch(() => {
      if (pageMounted) pageStateUpdates += 1;
    });

  pageMounted = false;
  let flushSettled = false;
  const flushing = saves.flushAll().finally(() => {
    flushSettled = true;
  });
  await Promise.resolve();
  assert.equal(flushSettled, false, "离开设置页后仍等待进行中的应用级操作");

  operation.reject(new Error("Windows 拒绝修改启动项"));
  await pageResult;
  await assert.rejects(flushing, /开机启动操作：Windows 拒绝修改启动项/);
  assert.equal(pageStateUpdates, 0, "卸载后的旧设置页不得接收操作结果");
});

test("安全退出从请求开始阻止 flush 期间的新编辑", async () => {
  const saves = new DesktopSaveCoordinator();
  const draftFlush = deferred();
  let body = "已进入 flush 的正文";
  saves.register({
    label: "写作草稿",
    flush: () => draftFlush.promise,
  });
  saves.beginExit(21);
  const flushing = saves.flushAll();
  await Promise.resolve();

  const accepted = saves.recordMutation();
  if (accepted) body = "flush 完成前产生的新正文";
  assert.equal(accepted, false);
  assert.equal(body, "已进入 flush 的正文");

  draftFlush.resolve();
  await flushing;
  saves.endExit(21);
  assert.equal(saves.recordMutation(), true, "退出失败或取消后恢复正常编辑");
});

test("明确确认后允许仍然退出，取消确认不会调用强制退出", async () => {
  const calls = [];
  assert.equal(await runForcedExit(9, false, async (id) => calls.push(id)), "cancelled");
  assert.equal(await runForcedExit(9, true, async (id) => calls.push(id)), "forced");
  assert.deepEqual(calls, [9]);
});

test("过期安全退出结果不会完成新的退出请求", async () => {
  const save = deferred();
  let current = true;
  let completed = false;
  const running = runSafeExit(10, {
    flush: () => save.promise,
    complete: async () => {
      completed = true;
    },
    isCurrent: () => current,
  });
  current = false;
  save.resolve();
  assert.deepEqual(await running, { status: "stale" });
  assert.equal(completed, false);
});

test("桌面正式路径保持单实例优先、三项托盘菜单和隐藏式开机启动", async () => {
  const rust = await readFile("src-tauri/src/lib.rs", "utf8");
  const lifecycle = await readFile("src-tauri/src/desktop_lifecycle.rs", "utf8");
  const settings = await readFile("src-tauri/src/settings.rs", "utf8");
  const migrations = await readFile("src-tauri/src/learning_records.rs", "utf8");
  const config = JSON.parse(await readFile("src-tauri/tauri.conf.json", "utf8"));
  const singleInstance = rust.indexOf("tauri_plugin_single_instance::init");
  const clipboard = rust.indexOf("tauri_plugin_clipboard_manager::init");
  assert.ok(singleInstance >= 0 && singleInstance < clipboard);
  assert.match(rust, /single_instance::init\(\|app, argv,[\s\S]*?AUTOSTART_ARGUMENT[\s\S]*?show_main_window\(app\)/);
  assert.match(lifecycle, /"打开 ReadRay"/);
  assert.match(lifecycle, /"快速查询"/);
  assert.match(lifecycle, /"退出 ReadRay"/);
  assert.match(lifecycle, /show_menu_on_left_click\(false\)/);
  assert.match(lifecycle, /TrayIconEvent::Click[\s\S]*?MouseButton::Left/);
  assert.match(lifecycle, /AUTOSTART_ARGUMENT/);
  assert.match(settings, /autolaunch\(\)\.is_enabled/);
  assert.doesNotMatch(migrations, /autostart/i);
  assert.equal(config.app.windows.find((window) => window.label === "main").visible, false);
  assert.equal(config.app.windows.find((window) => window.label === "overlay").visible, false);
});

test("安全退出只从应用装配层调用 typed Rust command，页面不直接 invoke", async () => {
  const app = await readFile("src/App.tsx", "utf8");
  const settingsPage = await readFile("src/components/SettingsPage.tsx", "utf8");
  const writingPage = await readFile("src/components/WritingPage.tsx", "utf8");
  assert.match(app, /readray:\/\/safe-exit-requested/);
  assert.match(app, /desktopSaveCoordinator\.flushAll/);
  assert.match(app, /complete_app_exit/);
  assert.match(app, /force_app_exit/);
  assert.match(app, /cancel_app_exit/);
  assert.match(app, /重试保存/);
  assert.match(app, /取消退出，继续使用/);
  assert.match(app, /仍然退出/);
  assert.match(app, /interactionBlocked=\{safeExitFailure\?\.retrying === true\}/);
  assert.match(app, /正在保存并退出/);
  assert.match(settingsPage, /desktopSaveCoordinator\.runMutation/);
  assert.match(settingsPage, /desktopSaveCoordinator\.recordMutation/);
  assert.match(
    settingsPage,
    /shortcutsChanged[\s\S]*?await service\.loadSettings\(\)[\s\S]*?shortcutRegistrationError/,
  );
  assert.match(
    writingPage,
    /desktopSaveCoordinator\.recordMutation\(\)[\s\S]*?saveCoordinatorRef\.current\?\.schedule/,
  );
  assert.doesNotMatch(settingsPage, /pendingSettingsOperationsRef/);
  assert.doesNotMatch(settingsPage, /\binvoke\s*\(/);
});
