import type { ThemePackageSelection, ThemeRepository } from "./themeRepository.ts";
import {
  validateCustomTheme,
  validateThemeSnapshot,
  type ThemeMode,
  type ThemeSnapshot,
} from "./themeProtocol.ts";

export interface ThemeService {
  load(): Promise<ThemeSnapshot>;
  prepareImport(): Promise<ThemePackageSelection | null>;
  importPreparedPackage(
    selection: ThemePackageSelection,
    expectedRevision: number,
  ): Promise<ThemeSnapshot>;
  select(themeId: string, mode: ThemeMode, expectedRevision: number): Promise<ThemeSnapshot>;
  delete(themeId: string, expectedRevision: number): Promise<ThemeSnapshot>;
}

export class RepositoryThemeService implements ThemeService {
  private readonly repository: ThemeRepository;

  constructor(repository: ThemeRepository) {
    this.repository = repository;
  }

  async load() {
    return validateThemeSnapshot(await this.repository.getSnapshot());
  }

  async prepareImport() {
    const selection = await this.repository.prepareImport();
    return selection === null
      ? null
      : { ...selection, theme: validateCustomTheme(selection.theme) };
  }

  async importPreparedPackage(selection: ThemePackageSelection, expectedRevision: number) {
    const validatedSelection = {
      ...selection,
      theme: validateCustomTheme(selection.theme),
    };
    return validateThemeSnapshot(
      await this.repository.importPreparedPackage(validatedSelection, expectedRevision),
    );
  }

  async select(themeId: string, mode: ThemeMode, expectedRevision: number) {
    return validateThemeSnapshot(
      await this.repository.select(themeId, mode, expectedRevision),
    );
  }

  async delete(themeId: string, expectedRevision: number) {
    return validateThemeSnapshot(
      await this.repository.delete(themeId, expectedRevision),
    );
  }
}
