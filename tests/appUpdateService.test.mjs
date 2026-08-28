import assert from "node:assert/strict";
import test from "node:test";
import {
  APP_UPDATE_PROGRESS_PUBLISH_INTERVAL_MS,
  AppUpdateService,
} from "../src/appUpdateService.ts";

function createFakeHandle(overrides = {}) {
  const handle = {
    version: "0.2.0",
    notes: null,
    downloadCalls: 0,
    installCalls: 0,
    downloadError: null,
    installError: null,
    async download(onEvent) {
      handle.downloadCalls += 1;
      if (handle.downloadError) {
        throw handle.downloadError;
      }
      onEvent?.({ event: "started", contentLength: 1000 });
      onEvent?.({ event: "progress", chunkLength: 400 });
      onEvent?.({ event: "progress", chunkLength: 600 });
      onEvent?.({ event: "finished" });
    },
    async install() {
      handle.installCalls += 1;
      if (handle.installError) {
        throw handle.installError;
      }
    },
    ...overrides,
  };
  return handle;
}

function createHarness({ clientCheck, now, isDesktopRuntime = () => true } = {}) {
  let fakeNow = 0;
  const advanceNow = (step) => {
    fakeNow += step;
  };
  let flushCalls = 0;
  let flushError = null;
  const client = {
    check: clientCheck ?? (async () => null),
  };
  const service = new AppUpdateService({
    client,
    flushBeforeInstall: async () => {
      flushCalls += 1;
      if (flushError) {
        throw flushError;
      }
    },
    isDesktopRuntime,
    now: now ?? (() => fakeNow),
  });
  return {
    service,
    client,
    advanceNow,
    getFlushCalls: () => flushCalls,
    setFlushError: (error) => {
      flushError = error;
    },
  };
}

test("手动检查无更新时进入 upToDate", async () => {
  const { service } = createHarness({ clientCheck: async () => null });
  await service.checkForUpdates("manual");
  assert.deepEqual(service.getState(), { status: "upToDate" });
});

test("手动检查发现新版本时进入 available 并保留版本与说明", async () => {
  const handle = createFakeHandle({
    version: "0.2.0",
    notes: "修复若干问题",
  });
  const { service } = createHarness({ clientCheck: async () => handle });
  await service.checkForUpdates("manual");
  assert.deepEqual(service.getState(), {
    status: "available",
    version: "0.2.0",
    notes: "修复若干问题",
  });
});

test("启动静默检查失败回到 idle，不打扰用户", async () => {
  const { service } = createHarness({
    clientCheck: async () => {
      throw new Error("Failed to fetch latest.json");
    },
  });
  await service.checkForUpdates("startup");
  assert.deepEqual(service.getState(), { status: "idle" });
});

test("手动检查失败进入 failed 并给出网络友好文案", async () => {
  const { service } = createHarness({
    clientCheck: async () => {
      throw new Error("Failed to fetch latest.json");
    },
  });
  await service.checkForUpdates("manual");
  const state = service.getState();
  assert.equal(state.status, "failed");
  assert.equal(state.retry, "check");
  assert.match(state.message, /暂时无法连接更新服务器/);
});

test("签名校验失败映射为明确的签名文案", async () => {
  const { service } = createHarness({
    clientCheck: async () => {
      throw new Error("signature verification failed");
    },
  });
  await service.checkForUpdates("manual");
  const state = service.getState();
  assert.equal(state.status, "failed");
  assert.match(state.message, /签名校验失败/);
});

test("applyUpdate 完整链路：下载进度 → flush → install", async () => {
  const handle = createFakeHandle();
  const harness = createHarness({ clientCheck: async () => handle });
  const { service } = harness;
  const observed = [];
  service.subscribe(() => observed.push(service.getState().status));

  await service.checkForUpdates("manual");
  await service.applyUpdate();

  assert.equal(handle.downloadCalls, 1);
  assert.equal(harness.getFlushCalls(), 1);
  assert.equal(handle.installCalls, 1);
  assert.ok(observed.includes("downloading"));
  assert.ok(observed.includes("installing"));
  // 模拟环境中 install 正常返回（真实 Windows 上进程会被安装器终止），
  // 此时诚实进入 failed 允许重试。
  const state = service.getState();
  assert.equal(state.status, "failed");
  assert.match(state.message, /更新安装未完成/);
});

test("下载进度事件按节流发布并累计字节数", async () => {
  const harness = createHarness();
  const { service, advanceNow } = harness;
  const handle = {
    version: "0.2.0",
    notes: null,
    async download(onEvent) {
      onEvent?.({ event: "started", contentLength: 200 });
      advanceNow(APP_UPDATE_PROGRESS_PUBLISH_INTERVAL_MS + 50);
      onEvent?.({ event: "progress", chunkLength: 100 });
      onEvent?.({ event: "progress", chunkLength: 100 });
      onEvent?.({ event: "finished" });
    },
    async install() {},
  };
  harness.client.check = async () => handle;
  const downloadingStates = [];
  service.subscribe(() => {
    const state = service.getState();
    if (state.status === "downloading") {
      downloadingStates.push(state.progress);
    }
  });

  await service.checkForUpdates("manual");
  await service.applyUpdate();

  // applyUpdate 先发布一次 progress:null 的下载中状态，
  // 随后 started、间隔外第一个 progress、finished 各发布一次；
  // 紧随其后的第二个 progress 被节流跳过但其字节累计进最终状态。
  assert.equal(downloadingStates.length, 4);
  assert.equal(downloadingStates[0], null);
  assert.deepEqual(downloadingStates[1], { received: 0, total: 200 });
  assert.deepEqual(downloadingStates[2], { received: 100, total: 200 });
  assert.deepEqual(downloadingStates[3], { received: 200, total: 200 });
});

test("flush 失败进入 failed，重试复用已下载内容不再重复下载", async () => {
  const handle = createFakeHandle();
  const harness = createHarness({ clientCheck: async () => handle });
  const { service } = harness;

  await service.checkForUpdates("manual");
  harness.setFlushError(new Error("写作草稿：保存失败"));
  await service.applyUpdate();

  assert.equal(handle.downloadCalls, 1);
  assert.equal(harness.getFlushCalls(), 1);
  assert.equal(handle.installCalls, 0);
  const failedState = service.getState();
  assert.equal(failedState.status, "failed");
  assert.match(failedState.message, /更新前保存失败/);

  harness.setFlushError(null);
  await service.retry();

  assert.equal(handle.downloadCalls, 1);
  assert.equal(handle.installCalls, 1);
});

test("下载失败进入 failed，重试重新下载", async () => {
  const handle = createFakeHandle();
  const harness = createHarness({ clientCheck: async () => handle });
  const { service } = harness;

  await service.checkForUpdates("manual");
  handle.downloadError = new Error("Network error");
  await service.applyUpdate();

  assert.equal(handle.downloadCalls, 1);
  assert.equal(harness.getFlushCalls(), 0);
  const failedState = service.getState();
  assert.equal(failedState.status, "failed");
  assert.equal(failedState.retry, "apply");
  assert.match(failedState.message, /暂时无法连接更新服务器/);

  handle.downloadError = null;
  await service.retry();

  assert.equal(handle.downloadCalls, 2);
  assert.equal(handle.installCalls, 1);
});

test("install 抛错进入 failed 且可重试", async () => {
  const handle = createFakeHandle();
  const harness = createHarness({ clientCheck: async () => handle });
  const { service } = harness;

  await service.checkForUpdates("manual");
  handle.installError = new Error("Invalid updater format");
  await service.applyUpdate();

  assert.equal(handle.installCalls, 1);
  assert.equal(service.getState().status, "failed");

  handle.installError = null;
  await service.retry();
  assert.equal(handle.installCalls, 2);
});

test("下载进行中忽略新的检查请求", async () => {
  let resolveDownload;
  const handle = createFakeHandle({
    download: () =>
      new Promise((resolve) => {
        resolveDownload = resolve;
      }),
  });
  const { service } = createHarness({ clientCheck: async () => handle });

  await service.checkForUpdates("manual");
  const applying = service.applyUpdate();
  await service.checkForUpdates("manual");

  assert.equal(service.getState().status, "downloading");

  resolveDownload?.();
  await applying;
  assert.equal(handle.installCalls, 1);
});

test("没有可用更新句柄时 applyUpdate 保持原状态", async () => {
  const { service } = createHarness({ clientCheck: async () => null });
  await service.checkForUpdates("manual");
  await service.applyUpdate();
  assert.deepEqual(service.getState(), { status: "upToDate" });
});

test("非桌面运行时 checkForUpdates 与 applyUpdate 均不执行", async () => {
  const handle = createFakeHandle();
  const harness = createHarness({
    clientCheck: async () => handle,
    isDesktopRuntime: () => false,
  });
  const { service } = harness;

  await service.checkForUpdates("manual");
  assert.deepEqual(service.getState(), { status: "idle" });

  await service.applyUpdate();
  assert.deepEqual(service.getState(), { status: "idle" });
  assert.equal(handle.downloadCalls, 0);
  assert.equal(harness.getFlushCalls(), 0);
});

test("订阅在状态变化时收到通知，退订后不再收到", async () => {
  const { service } = createHarness({ clientCheck: async () => null });
  let notifications = 0;
  const unsubscribe = service.subscribe(() => {
    notifications += 1;
  });

  // checking → upToDate 两次状态变化。
  await service.checkForUpdates("manual");
  assert.equal(notifications, 2);

  unsubscribe();
  await service.checkForUpdates("manual");
  assert.equal(notifications, 2);
});

test("dismissTransientFailure 将 failed 回落 idle", async () => {
  const { service } = createHarness({
    clientCheck: async () => {
      throw new Error("Failed to fetch latest.json");
    },
  });
  await service.checkForUpdates("manual");
  assert.equal(service.getState().status, "failed");

  service.dismissTransientFailure();
  assert.deepEqual(service.getState(), { status: "idle" });
});

test("dismissTransientFailure 不影响 available 与已下载内容复用", async () => {
  const handle = createFakeHandle();
  const { service } = createHarness({ clientCheck: async () => handle });
  await service.checkForUpdates("manual");
  assert.equal(service.getState().status, "available");

  service.dismissTransientFailure();
  assert.equal(service.getState().status, "available");
});
