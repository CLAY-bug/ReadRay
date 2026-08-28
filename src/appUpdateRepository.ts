import { check, type Update } from "@tauri-apps/plugin-updater";

export type AppUpdateDownloadEvent =
  | { event: "started"; contentLength: number | null }
  | { event: "progress"; chunkLength: number }
  | { event: "finished" };

export interface AppUpdateHandle {
  readonly version: string;
  readonly notes: string | null;
  download(
    onEvent?: (event: AppUpdateDownloadEvent) => void,
  ): Promise<void>;
  install(): Promise<void>;
}

export interface AppUpdateClient {
  check(): Promise<AppUpdateHandle | null>;
}

class TauriAppUpdate implements AppUpdateHandle {
  readonly version: string;
  readonly notes: string | null;
  private readonly update: Update;

  constructor(update: Update) {
    this.update = update;
    this.version = update.version;
    this.notes = update.body?.trim() ? update.body : null;
  }

  download(onEvent?: (event: AppUpdateDownloadEvent) => void) {
    return this.update.download((event) => {
      if (!onEvent) {
        return;
      }
      switch (event.event) {
        case "Started":
          onEvent({
            event: "started",
            contentLength: event.data.contentLength ?? null,
          });
          break;
        case "Progress":
          onEvent({ event: "progress", chunkLength: event.data.chunkLength });
          break;
        case "Finished":
          onEvent({ event: "finished" });
          break;
      }
    });
  }

  install() {
    return this.update.install();
  }
}

export class TauriAppUpdateRepository implements AppUpdateClient {
  async check(): Promise<AppUpdateHandle | null> {
    const update = await check();
    return update ? new TauriAppUpdate(update) : null;
  }
}
