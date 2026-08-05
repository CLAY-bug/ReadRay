import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { ReadRayThemeV1, ThemeMode, ThemeSnapshot } from "./themeProtocol.ts";

export type ThemeInvoke = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

export type ThemeDirectoryDialog = (options: {
  title: string;
  directory: true;
  multiple: false;
}) => Promise<string | string[] | null>;

export type ThemePackageSelection = {
  directoryPath: string;
  theme: ReadRayThemeV1;
};

export interface ThemeRepository {
  getSnapshot(): Promise<ThemeSnapshot>;
  prepareImport(): Promise<ThemePackageSelection | null>;
  importPreparedPackage(
    selection: ThemePackageSelection,
    expectedRevision: number,
  ): Promise<ThemeSnapshot>;
  select(themeId: string, mode: ThemeMode, expectedRevision: number): Promise<ThemeSnapshot>;
  delete(themeId: string, expectedRevision: number): Promise<ThemeSnapshot>;
}

export class TauriThemeRepository implements ThemeRepository {
  private readonly invokeCommand: ThemeInvoke;
  private readonly openDirectoryDialog: ThemeDirectoryDialog;

  constructor(
    invokeCommand: ThemeInvoke = invoke,
    openDirectoryDialog: ThemeDirectoryDialog = open,
  ) {
    this.invokeCommand = invokeCommand;
    this.openDirectoryDialog = openDirectoryDialog;
  }

  getSnapshot() {
    return this.invokeCommand<ThemeSnapshot>("get_theme_snapshot");
  }

  async prepareImport() {
    const selected = await this.openDirectoryDialog({
      title: "选择 ReadRay 主题包目录",
      directory: true,
      multiple: false,
    });
    if (selected === null) return null;
    if (Array.isArray(selected)) {
      throw new Error("主题导入只允许选择一个目录。");
    }
    const theme = await this.invokeCommand<ReadRayThemeV1>("inspect_theme_package", {
      directoryPath: selected,
    });
    return { directoryPath: selected, theme };
  }

  importPreparedPackage(selection: ThemePackageSelection, expectedRevision: number) {
    return this.invokeCommand<ThemeSnapshot>("import_theme_package", {
      directoryPath: selection.directoryPath,
      expectedThemeId: selection.theme.manifest.id,
      expectedRevision,
    });
  }

  select(themeId: string, mode: ThemeMode, expectedRevision: number) {
    return this.invokeCommand<ThemeSnapshot>("select_theme", {
      themeId,
      mode,
      expectedRevision,
    });
  }

  delete(themeId: string, expectedRevision: number) {
    return this.invokeCommand<ThemeSnapshot>("delete_custom_theme", {
      themeId,
      expectedRevision,
    });
  }
}
