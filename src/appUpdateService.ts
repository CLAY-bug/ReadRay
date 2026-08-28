import { desktopSaveCoordinator } from "./desktopLifecycle.ts";
import {
  TauriAppUpdateRepository,
  type AppUpdateClient,
  type AppUpdateDownloadEvent,
  type AppUpdateHandle,
} from "./appUpdateRepository.ts";

export type AppUpdateProgress = {
  received: number;
  total: number | null;
};

export type AppUpdateState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "upToDate" }
  | { status: "available"; version: string; notes: string | null }
  | {
      status: "downloading";
      version: string;
      notes: string | null;
      progress: AppUpdateProgress | null;
    }
  | { status: "installing"; version: string; phase: "flushing" | "installer" }
  | { status: "failed"; message: string; retry: "check" | "apply" };

export type AppUpdateCheckTrigger = "startup" | "manual";

export const APP_UPDATE_PROGRESS_PUBLISH_INTERVAL_MS = 200;

export type AppUpdateServiceDeps = {
  client: AppUpdateClient;
  flushBeforeInstall: () => Promise<void>;
  isDesktopRuntime: () => boolean;
  now?: () => number;
};

function formatError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function matchesAny(value: string, fragments: readonly string[]) {
  return fragments.some((fragment) => value.includes(fragment));
}

function friendlyNetworkError(error: unknown) {
  const message = formatError(error).toLowerCase();
  if (matchesAny(message, ["signature", "verify", "minisign"])) {
    return "更新包签名校验失败，已停止安装。";
  }
  if (
    matchesAny(message, [
      "network",
      "fetch",
      "timeout",
      "timed out",
      "dns",
      "connect",
      "connection",
      "invalid updater format",
      "unexpected status",
    ])
  ) {
    return "暂时无法连接更新服务器，请稍后重试。";
  }
  return "更新失败，请稍后重试。";
}

/**
 * 应用内更新协调服务：负责检查 latest.json、下载进度状态机、
 * 安装前经 desktopSaveCoordinator flush 全部待落盘写入，
 * 再交由 tauri-plugin-updater 执行 NSIS 安装。
 *
 * Windows 上 install() 会由插件直接结束进程并由 NSIS 安装器
 * （/P /R /UPDATE 参数）在安装完成后自动重启应用，因此
 * "installing/installer" 之后的代码通常没有机会执行。
 */
export class AppUpdateService {
  private state: AppUpdateState = { status: "idle" };
  private readonly listeners = new Set<() => void>();
  private readonly deps: AppUpdateServiceDeps;
  private handle: AppUpdateHandle | null = null;
  private downloaded = false;
  private progress: AppUpdateProgress | null = null;
  private lastProgressPublishAt = 0;
  private readonly now: () => number;

  constructor(deps: AppUpdateServiceDeps) {
    this.deps = deps;
    this.now = deps.now ?? (() => Date.now());
  }

  getState(): AppUpdateState {
    return this.state;
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private setState(next: AppUpdateState) {
    this.state = next;
    for (const listener of [...this.listeners]) {
      listener();
    }
  }

  async checkForUpdates(trigger: AppUpdateCheckTrigger): Promise<void> {
    if (!this.deps.isDesktopRuntime()) {
      return;
    }
    if (this.isBusy()) {
      return;
    }

    this.setState({ status: "checking" });
    let handle: AppUpdateHandle | null;
    try {
      handle = await this.deps.client.check();
    } catch (error) {
      console.error("ReadRay 更新检查失败：", error);
      // 启动静默检查失败不打扰用户；手动检查需要明确反馈。
      this.setState(
        trigger === "manual"
          ? { status: "failed", message: friendlyNetworkError(error), retry: "check" }
          : { status: "idle" },
      );
      return;
    }

    this.handle = handle;
    this.downloaded = false;
    this.progress = null;
    this.lastProgressPublishAt = 0;
    this.setState(
      handle
        ? { status: "available", version: handle.version, notes: handle.notes }
        : { status: "upToDate" },
    );
  }

  async applyUpdate(): Promise<void> {
    if (!this.deps.isDesktopRuntime()) {
      return;
    }
    const handle = this.handle;
    if (!handle || this.isBusy()) {
      return;
    }

    if (!this.downloaded) {
      this.progress = null;
      this.lastProgressPublishAt = 0;
      this.setState({
        status: "downloading",
        version: handle.version,
        notes: handle.notes,
        progress: null,
      });
      try {
        await handle.download((event) => {
          this.onDownloadEvent(event, handle.version, handle.notes);
        });
        this.downloaded = true;
      } catch (error) {
        console.error("ReadRay 更新下载失败：", error);
        this.setState({
          status: "failed",
          message: friendlyNetworkError(error),
          retry: "apply",
        });
        return;
      }
    }

    // 安装进程会终止应用，先等待既有防抖写入（写作草稿、
    // 复习反馈、偏好保存）全部安全落盘。
    this.setState({
      status: "installing",
      version: handle.version,
      phase: "flushing",
    });
    try {
      await this.deps.flushBeforeInstall();
    } catch (error) {
      console.error("ReadRay 更新前保存失败：", error);
      this.setState({
        status: "failed",
        message: `更新前保存失败：${formatError(error)}`,
        retry: "apply",
      });
      return;
    }

    this.setState({
      status: "installing",
      version: handle.version,
      phase: "installer",
    });
    try {
      await handle.install();
    } catch (error) {
      console.error("ReadRay 更新安装失败：", error);
      this.setState({
        status: "failed",
        message: friendlyNetworkError(error),
        retry: "apply",
      });
      return;
    }
    // 正常情况下 Windows 上 install() 不会返回（进程被安装器接管）；
    // 万一返回则诚实提示，允许重试。
    this.setState({
      status: "failed",
      message: "更新安装未完成，请重试。",
      retry: "apply",
    });
  }

  retry(): Promise<void> {
    if (this.state.status !== "failed") {
      return Promise.resolve();
    }
    return this.state.retry === "apply"
      ? this.applyUpdate()
      : this.checkForUpdates("manual");
  }

  /** 失败是瞬态反馈：更新行卸载（切走页面/标签）时回落 idle，避免失败样式长期驻留；已下载内容与更新句柄保持不变。 */
  dismissTransientFailure() {
    if (this.state.status === "failed") {
      this.setState({ status: "idle" });
    }
  }

  private isBusy() {
    return (
      this.state.status === "checking" ||
      this.state.status === "downloading" ||
      this.state.status === "installing"
    );
  }

  private onDownloadEvent(
    event: AppUpdateDownloadEvent,
    version: string,
    notes: string | null,
  ) {
    if (event.event === "started") {
      this.progress = { received: 0, total: event.contentLength };
    } else if (event.event === "progress") {
      if (!this.progress) {
        this.progress = { received: 0, total: null };
      }
      this.progress = {
        received: this.progress.received + event.chunkLength,
        total: this.progress.total,
      };
    }

    const publish =
      event.event !== "progress" ||
      this.now() - this.lastProgressPublishAt >=
        APP_UPDATE_PROGRESS_PUBLISH_INTERVAL_MS;
    if (!publish) {
      return;
    }
    this.lastProgressPublishAt = this.now();
    this.setState({
      status: "downloading",
      version,
      notes,
      progress: this.progress,
    });
  }
}

export const appUpdateService = new AppUpdateService({
  client: new TauriAppUpdateRepository(),
  flushBeforeInstall: () => desktopSaveCoordinator.flushAll(),
  isDesktopRuntime: () => "__TAURI_INTERNALS__" in window,
});
