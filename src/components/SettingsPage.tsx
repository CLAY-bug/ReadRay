import {
  useEffect,
  useId,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import type { SettingsService } from "../settingsService";
import type { AppPreferenceSaveOutcome } from "../appPreferenceSaveCoordinator";
import type { AppThemeController } from "../useAppTheme";
import type {
  ThemeMutationOutcome,
  ThemeMutationRetry,
} from "../themeMutationCoordinator";
import type { ThemeMode } from "../themeProtocol";
import {
  desktopSaveCoordinator,
  shortcutFromKeyEvent,
  shortcutParts,
} from "../desktopLifecycle";
import {
  DEFAULT_APP_PREFERENCES,
  LEARNING_FONT_SIZE_MAX,
  LEARNING_FONT_SIZE_MIN,
  parseFontSizeCandidate,
  UI_FONT_SIZE_MAX,
  UI_FONT_SIZE_MIN,
  type AppPreferences,
} from "../appPreferences";
import {
  BalanceRefreshController,
  reduceBalanceRefreshState,
  type BalanceRefreshState,
} from "../settingsBalanceRefresh";
import {
  isSettingsOperationCurrent,
  suggestedBackupFileName,
  validateApiKeyDraft,
  type DatabaseBackupResult,
  type DeepSeekBalance,
  type ModelUsageCategory,
  type ModelUsageRange,
  type ModelUsageSummary,
  type SettingsSnapshot,
} from "../settingsViewModel";
import geistLicense from "../assets/fonts/licenses/Geist-OFL.txt?raw";
import newsreaderLicense from "../assets/fonts/licenses/Newsreader-OFL.txt?raw";
import sourceHanSansLicense from "../assets/fonts/licenses/Source-Han-Sans-OFL.txt?raw";
import sourceHanSerifLicense from "../assets/fonts/licenses/Source-Han-Serif-OFL.txt?raw";
import flexokiLicense from "../assets/flexoki-LICENSE.txt?raw";

type SettingsSection = "general" | "appearance" | "ai" | "data" | "about";
type OperationState = "idle" | "saving" | "clearing";
type RequestStatus = "idle" | "loading" | "success" | "error";

type SettingsPageProps = {
  service: SettingsService | null;
  themeController: AppThemeController;
  onPreferencesSave?: (
    candidate: AppPreferences,
    previousAuthority: AppPreferences,
  ) => Promise<AppPreferenceSaveOutcome>;
};

const settingsSections: ReadonlyArray<readonly [SettingsSection, string]> = [
  ["general", "通用"],
  ["appearance", "外观"],
  ["ai", "AI 服务"],
  ["data", "数据"],
  ["about", "关于"],
];

const usageRanges: ReadonlyArray<readonly [ModelUsageRange, string]> = [
  ["today", "今天"],
  ["last7Days", "近 7 天"],
  ["last30Days", "近 30 天"],
  ["all", "全部"],
];

const licenseMaterials = [
  {
    name: "Flexoki（内置主题配色）",
    license: "MIT License",
    text: flexokiLicense,
  },
  {
    name: "Geist 与 Geist Mono",
    license: "SIL Open Font License 1.1",
    text: geistLicense,
  },
  {
    name: "Newsreader",
    license: "SIL Open Font License 1.1",
    text: newsreaderLicense,
  },
  {
    name: "Source Han Sans SC（思源黑体）",
    license: "SIL Open Font License 1.1",
    text: sourceHanSansLicense,
  },
  {
    name: "Source Han Serif SC（思源宋体）",
    license: "SIL Open Font License 1.1",
    text: sourceHanSerifLicense,
  },
] as const;

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function formatByteSize(byteSize: number) {
  if (byteSize >= 1024 * 1024) {
    return `${(byteSize / (1024 * 1024)).toFixed(1)} MB`;
  }
  if (byteSize >= 1024) {
    return `${(byteSize / 1024).toFixed(1)} KB`;
  }
  return `${byteSize} B`;
}

function formatUsageNumber(value: number) {
  return new Intl.NumberFormat("zh-CN").format(value);
}

function formatUsageDate(value: number | null) {
  if (value === null) return "暂无记录";
  const date = new Date(value);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(
    date.getDate(),
  ).padStart(2, "0")}`;
}

function usageCategory(
  summary: ModelUsageSummary,
  category: ModelUsageCategory,
) {
  return summary.categories.find((item) => item.category === category)!;
}

function FolderIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M3 6.5h7l2 2h9v9.5H3z" />
    </svg>
  );
}

function ArrowIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="m9 5 7 7-7 7" />
    </svg>
  );
}

function ChevronDownIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="m6 9 6 6 6-6" />
    </svg>
  );
}

function SettingsHeader({
  title,
  id,
}: {
  title: string;
  id: string;
}) {
  return (
    <header className="rr-settings-header">
      <h1 id={id}>{title}</h1>
    </header>
  );
}

function GroupHeading({ title, meta }: { title: string; meta?: string }) {
  return (
    <div className="rr-settings-group-heading">
      <h2>{title}</h2>
      {meta ? <p>{meta}</p> : null}
    </div>
  );
}

function SettingsCopy({ label, help }: { label: string; help?: string }) {
  return (
    <div className="rr-settings-copy">
      <span className="rr-settings-label">{label}</span>
      {help ? <span className="rr-settings-help">{help}</span> : null}
    </div>
  );
}

type SettingsSelectOption = {
  value: string;
  label: string;
};

function SettingsSelect({
  label,
  value,
  options,
  disabled = false,
  className,
  onChange,
}: {
  label: string;
  value: string;
  options: readonly SettingsSelectOption[];
  disabled?: boolean;
  className?: string;
  onChange: (value: string) => void;
}) {
  const selectId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const selectedIndex = Math.max(
    0,
    options.findIndex((option) => option.value === value),
  );
  const [activeIndex, setActiveIndex] = useState(selectedIndex);
  const selectedOption = options[selectedIndex];

  useEffect(() => {
    if (!open) return;

    const handlePointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const handleDocumentKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
        buttonRef.current?.focus();
      }
    };

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleDocumentKeyDown);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleDocumentKeyDown);
    };
  }, [open]);

  useEffect(() => {
    if (!open) {
      setActiveIndex(selectedIndex);
    }
  }, [open, selectedIndex]);

  const chooseOption = (index: number) => {
    const option = options[index];
    if (!option) return;
    onChange(option.value);
    setOpen(false);
    buttonRef.current?.focus();
  };

  const openMenu = () => {
    if (disabled || options.length === 0) return;
    setActiveIndex(selectedIndex);
    setOpen(true);
  };

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (disabled || options.length === 0) return;

    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!open) {
        openMenu();
        return;
      }
      const delta = event.key === "ArrowDown" ? 1 : -1;
      setActiveIndex((index) =>
        Math.min(Math.max(index + delta, 0), options.length - 1),
      );
      return;
    }

    if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      if (!open) openMenu();
      setActiveIndex(event.key === "Home" ? 0 : options.length - 1);
      return;
    }

    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      if (open) {
        chooseOption(activeIndex);
      } else {
        openMenu();
      }
    }
  };

  return (
    <div
      ref={rootRef}
      className={`rr-settings-select-shell${open ? " is-open" : ""}${
        className ? ` ${className}` : ""
      }`}
    >
      <button
        ref={buttonRef}
        className="rr-settings-select"
        type="button"
        role="combobox"
        aria-label={label}
        aria-controls={`${selectId}-listbox`}
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-activedescendant={
          open ? `${selectId}-option-${activeIndex}` : undefined
        }
        disabled={disabled}
        onClick={() => (open ? setOpen(false) : openMenu())}
        onKeyDown={handleKeyDown}
        title={selectedOption?.label ?? value}
      >
        <span className="rr-settings-select-value">
          {selectedOption?.label ?? value}
        </span>
        <ChevronDownIcon />
      </button>
      {open ? (
        <div
          id={`${selectId}-listbox`}
          className="rr-settings-select-menu"
          role="listbox"
          aria-label={label}
        >
          {options.map((option, index) => (
            <button
              id={`${selectId}-option-${index}`}
              className={`rr-settings-select-option${
                index === activeIndex ? " is-active" : ""
              }${option.value === value ? " is-selected" : ""}`}
              key={option.value}
              type="button"
              role="option"
              aria-selected={option.value === value}
              onMouseEnter={() => setActiveIndex(index)}
              onClick={() => chooseOption(index)}
            >
              {option.label}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function ShortcutRow({
  name,
  value,
  recording,
  disabled,
  isDefault,
  onRecord,
  onRestore,
}: {
  name: string;
  value: string;
  recording: boolean;
  disabled: boolean;
  isDefault: boolean;
  onRecord: () => void;
  onRestore: () => void;
}) {
  return (
    <div className="rr-settings-shortcut-row">
      <div className="rr-settings-shortcut-name">{name}</div>
      <div className="rr-settings-shortcut-actions">
        <div className="rr-settings-shortcut-value" aria-label={value}>
          {shortcutParts(value).map((part) => <kbd key={part}>{part}</kbd>)}
        </div>
        <button
          className="rr-settings-button"
          type="button"
          disabled={disabled}
          aria-pressed={recording}
          onClick={onRecord}
        >
          {recording ? "请按组合键…" : "录制新快捷键"}
        </button>
        <button
          className="rr-settings-restore"
          type="button"
          disabled={disabled || isDefault}
          title={isDefault ? "当前已是默认值" : "恢复默认快捷键"}
          onClick={onRestore}
        >
          恢复默认
        </button>
      </div>
    </div>
  );
}

function SettingsLoading() {
  return (
    <div className="rr-settings-loading" aria-label="正在加载设置">
      <div className="rr-settings-loading-rail">
        <span className="rr-settings-skeleton is-short" />
        <span className="rr-settings-skeleton is-mid" />
        <span className="rr-settings-skeleton is-mid" />
        <span className="rr-settings-skeleton is-short" />
      </div>
      <div className="rr-settings-loading-content">
        <span className="rr-settings-skeleton is-title" />
        <span className="rr-settings-skeleton is-wide" />
        <span className="rr-settings-skeleton is-mid" />
        <span className="rr-settings-skeleton is-wide is-spaced" />
        <span className="rr-settings-skeleton is-wide" />
        <span className="rr-settings-skeleton is-mid" />
      </div>
    </div>
  );
}

function SettingsPage({
  service,
  themeController,
  onPreferencesSave,
}: SettingsPageProps) {
  const [activeSection, setActiveSection] = useState<SettingsSection>("general");
  const [snapshot, setSnapshot] = useState<SettingsSnapshot>();
  const [loadStatus, setLoadStatus] = useState<"loading" | "ready" | "error">(
    "loading",
  );
  const [loadError, setLoadError] = useState<string>();
  const [retryToken, setRetryToken] = useState(0);
  const [editingKey, setEditingKey] = useState(false);
  const [keyDraft, setKeyDraft] = useState("");
  const [operation, setOperation] = useState<OperationState>("idle");
  const [operationMessage, setOperationMessage] = useState<string>();
  const [operationError, setOperationError] = useState<string>();
  const [confirmingClear, setConfirmingClear] = useState(false);
  const [showingLicenses, setShowingLicenses] = useState(false);
  const [balanceState, setBalanceState] = useState<
    BalanceRefreshState<DeepSeekBalance>
  >({ status: "idle" });
  const [documentVisible, setDocumentVisible] = useState(
    () => typeof document === "undefined" || document.visibilityState === "visible",
  );
  const [usageRange, setUsageRange] = useState<ModelUsageRange>("last7Days");
  const [usageStatus, setUsageStatus] = useState<RequestStatus>("idle");
  const [usageSummary, setUsageSummary] = useState<ModelUsageSummary>();
  const [usageError, setUsageError] = useState<string>();
  const [usageRetryToken, setUsageRetryToken] = useState(0);
  const [directoryStatus, setDirectoryStatus] = useState<RequestStatus>("idle");
  const [directoryMessage, setDirectoryMessage] = useState<string>();
  const [backupStatus, setBackupStatus] = useState<RequestStatus>("idle");
  const [backupResult, setBackupResult] = useState<DatabaseBackupResult>();
  const [backupMessage, setBackupMessage] = useState<string>();
  const [preferenceStatus, setPreferenceStatus] = useState<RequestStatus>("idle");
  const [preferenceMessage, setPreferenceMessage] = useState<string>();
  const [failedPreferences, setFailedPreferences] = useState<AppPreferences>();
  const [recordingShortcut, setRecordingShortcut] = useState<
    "quickQueryShortcut" | "selectionExplanationShortcut"
  >();
  const [shortcutError, setShortcutError] = useState<string>();
  const [autostartStatus, setAutostartStatus] = useState<RequestStatus>("idle");
  const [autostartMessage, setAutostartMessage] = useState<string>();
  const [themeStatus, setThemeStatus] = useState<RequestStatus>("idle");
  const [themeMessage, setThemeMessage] = useState<string>();
  const [themeRetry, setThemeRetry] = useState<ThemeMutationRetry>();
  const mountedRef = useRef(false);
  const operationKeyRef = useRef(0);
  const balanceControllerRef = useRef<{
    service: SettingsService;
    controller: BalanceRefreshController<DeepSeekBalance>;
  } | null>(null);
  const balanceCleanupGenerationRef = useRef(0);
  const usageKeyRef = useRef(0);
  const directoryKeyRef = useRef(0);
  const backupKeyRef = useRef(0);
  const preferenceKeyRef = useRef(0);
  const autostartKeyRef = useRef(0);
  const themeKeyRef = useRef(0);
  const scrollRef = useRef<HTMLDivElement>(null);
  function trackSettingsOperation<T>(label: string, start: () => Promise<T>) {
    return desktopSaveCoordinator.runMutation(label, start);
  }

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      operationKeyRef.current += 1;
      usageKeyRef.current += 1;
      directoryKeyRef.current += 1;
      backupKeyRef.current += 1;
      preferenceKeyRef.current += 1;
      autostartKeyRef.current += 1;
      themeKeyRef.current += 1;
    };
  }, []);

  useEffect(() => {
    if (!recordingShortcut || !snapshot) return;
    const activeRecording = recordingShortcut;
    const currentSnapshot = snapshot;
    function record(event: KeyboardEvent) {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") {
        setRecordingShortcut(undefined);
        setShortcutError(undefined);
        return;
      }
      try {
        const shortcut = shortcutFromKeyEvent(event);
        if (!shortcut) return;
        const other = activeRecording === "quickQueryShortcut"
          ? currentSnapshot.preferences.selectionExplanationShortcut
          : currentSnapshot.preferences.quickQueryShortcut;
        if (shortcut === other) {
          throw new Error("快速查询和选区解释不能使用同一个快捷键。");
        }
        setShortcutError(undefined);
        setRecordingShortcut(undefined);
        patchPreferences({ [activeRecording]: shortcut });
      } catch (error) {
        setShortcutError(errorMessage(error));
      }
    }
    window.addEventListener("keydown", record, true);
    return () => window.removeEventListener("keydown", record, true);
  }, [recordingShortcut, snapshot]);

  useEffect(() => {
    if (!service) return;
    const currentService = service;
    let disposed = false;
    async function refreshAutostart() {
      if (document.visibilityState !== "visible") return;
      const requestKey = autostartKeyRef.current + 1;
      autostartKeyRef.current = requestKey;
      try {
        const enabled = await currentService.loadAutostartEnabled();
        if (
          disposed ||
          !isSettingsOperationCurrent(
            mountedRef.current,
            requestKey,
            autostartKeyRef.current,
          )
        ) return;
        setSnapshot((current) => current ? { ...current, autostartEnabled: enabled } : current);
        setAutostartStatus("idle");
        setAutostartMessage(undefined);
      } catch (error) {
        if (!disposed) {
          setAutostartStatus("error");
          setAutostartMessage(`读取失败：${errorMessage(error)}`);
        }
      }
    }
    window.addEventListener("focus", refreshAutostart);
    document.addEventListener("visibilitychange", refreshAutostart);
    return () => {
      disposed = true;
      window.removeEventListener("focus", refreshAutostart);
      document.removeEventListener("visibilitychange", refreshAutostart);
    };
  }, [service]);

  useEffect(() => {
    function syncDocumentVisibility() {
      setDocumentVisible(document.visibilityState === "visible");
    }

    syncDocumentVisibility();
    document.addEventListener("visibilitychange", syncDocumentVisibility);
    return () => {
      document.removeEventListener("visibilitychange", syncDocumentVisibility);
    };
  }, []);

  useEffect(() => {
    const setupGeneration = balanceCleanupGenerationRef.current + 1;
    balanceCleanupGenerationRef.current = setupGeneration;

    if (!service) return;
    if (balanceControllerRef.current?.service !== service) {
      balanceControllerRef.current?.controller.dispose();
      balanceControllerRef.current = {
        service,
        controller: new BalanceRefreshController(
          () => service.loadBalance(),
          (event) => {
            if (!mountedRef.current) return;
            setBalanceState((state) => reduceBalanceRefreshState(state, event));
          },
        ),
      };
    }

    const entry = balanceControllerRef.current;
    return () => {
      const cleanupGeneration = balanceCleanupGenerationRef.current + 1;
      balanceCleanupGenerationRef.current = cleanupGeneration;
      queueMicrotask(() => {
        if (
          balanceCleanupGenerationRef.current === cleanupGeneration &&
          balanceControllerRef.current === entry
        ) {
          entry.controller.dispose();
          balanceControllerRef.current = null;
        }
      });
    };
  }, [service]);

  useEffect(() => {
    balanceControllerRef.current?.controller.updateContext({
      active: activeSection === "ai",
      visible: documentVisible,
      apiKeyConfigured: snapshot?.apiKeyConfigured === true,
    });
  }, [activeSection, documentVisible, service, snapshot?.apiKeyConfigured]);

  useEffect(() => {
    let ignore = false;
    if (!service) {
      setLoadStatus("loading");
      return () => {
        ignore = true;
      };
    }

    setLoadStatus("loading");
    setLoadError(undefined);
    void service.loadSettings().then(
      (nextSnapshot) => {
        if (ignore) return;
        setSnapshot(nextSnapshot);
        setShortcutError(nextSnapshot.shortcutRegistrationError ?? undefined);
        setEditingKey(!nextSnapshot.apiKeyConfigured);
        setLoadStatus("ready");
      },
      (error) => {
        if (ignore) return;
        setLoadError(errorMessage(error));
        setLoadStatus("error");
      },
    );
    return () => {
      ignore = true;
    };
  }, [retryToken, service]);

  useEffect(() => {
    if (!service) {
      setUsageStatus("idle");
      return;
    }
    const requestKey = usageKeyRef.current + 1;
    usageKeyRef.current = requestKey;
    setUsageStatus("loading");
    setUsageSummary(undefined);
    setUsageError(undefined);
    void service.loadUsage(usageRange).then(
      (summary) => {
        if (
          !isSettingsOperationCurrent(
            mountedRef.current,
            requestKey,
            usageKeyRef.current,
          )
        ) {
          return;
        }
        setUsageSummary(summary);
        setUsageStatus("success");
      },
      (error) => {
        if (
          !isSettingsOperationCurrent(
            mountedRef.current,
            requestKey,
            usageKeyRef.current,
          )
        ) {
          return;
        }
        setUsageStatus("error");
        setUsageError(errorMessage(error));
      },
    );
  }, [service, usageRange, usageRetryToken]);

  useEffect(() => {
    if (!confirmingClear) return;
    function handleEscape(event: KeyboardEvent) {
      if (event.key === "Escape" && operation !== "clearing") {
        setConfirmingClear(false);
        setOperationError(undefined);
      }
    }
    window.addEventListener("keydown", handleEscape);
    return () => window.removeEventListener("keydown", handleEscape);
  }, [confirmingClear, operation]);

  useEffect(() => {
    if (!showingLicenses) return;
    function handleEscape(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setShowingLicenses(false);
      }
    }
    window.addEventListener("keydown", handleEscape);
    return () => window.removeEventListener("keydown", handleEscape);
  }, [showingLicenses]);

  function selectSection(section: SettingsSection) {
    setActiveSection(section);
    scrollRef.current?.scrollTo({ top: 0 });
  }

  async function saveApiKey(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!service || operation !== "idle") return;

    const validationError = validateApiKeyDraft(keyDraft);
    if (validationError) {
      setOperationMessage(undefined);
      setOperationError(validationError);
      return;
    }

    const operationKey = operationKeyRef.current + 1;
    operationKeyRef.current = operationKey;
    setOperation("saving");
    setOperationMessage(undefined);
    setOperationError(undefined);
    try {
      const nextSnapshot = await trackSettingsOperation(
        "API Key 操作",
        () => service.validateAndSaveApiKey(keyDraft),
      );
      if (
        !isSettingsOperationCurrent(
          mountedRef.current,
          operationKey,
          operationKeyRef.current,
        )
      ) {
        return;
      }
      setSnapshot(nextSnapshot);
      setKeyDraft("");
      setEditingKey(false);
      balanceControllerRef.current?.controller.replaceCredential({
        active: activeSection === "ai",
        visible: documentVisible,
        apiKeyConfigured: nextSnapshot.apiKeyConfigured,
      });
      setOperation("idle");
      setOperationMessage("验证成功，API Key 已安全保存。");
    } catch (error) {
      if (
        !isSettingsOperationCurrent(
          mountedRef.current,
          operationKey,
          operationKeyRef.current,
        )
      ) {
        return;
      }
      setOperation("idle");
      setOperationError(`验证或保存失败：${errorMessage(error)}`);
    }
  }

  async function clearApiKey() {
    if (!service || operation !== "idle") return;
    const operationKey = operationKeyRef.current + 1;
    operationKeyRef.current = operationKey;
    setOperation("clearing");
    setOperationMessage(undefined);
    setOperationError(undefined);
    try {
      const nextSnapshot = await trackSettingsOperation(
        "API Key 操作",
        () => service.clearApiKey(),
      );
      if (
        !isSettingsOperationCurrent(
          mountedRef.current,
          operationKey,
          operationKeyRef.current,
        )
      ) {
        return;
      }
      setSnapshot(nextSnapshot);
      setKeyDraft("");
      setEditingKey(true);
      balanceControllerRef.current?.controller.replaceCredential({
        active: activeSection === "ai",
        visible: documentVisible,
        apiKeyConfigured: nextSnapshot.apiKeyConfigured,
      });
      setConfirmingClear(false);
      setOperation("idle");
      setOperationMessage("API Key 已清除，AI 功能现已停用。");
    } catch (error) {
      if (
        !isSettingsOperationCurrent(
          mountedRef.current,
          operationKey,
          operationKeyRef.current,
        )
      ) {
        return;
      }
      setOperation("idle");
      setOperationError(`清除失败：${errorMessage(error)}`);
    }
  }

  function refreshBalance() {
    balanceControllerRef.current?.controller.refreshNow();
  }

  async function openDataDirectory() {
    if (!service || directoryStatus === "loading") return;
    const requestKey = directoryKeyRef.current + 1;
    directoryKeyRef.current = requestKey;
    setDirectoryStatus("loading");
    setDirectoryMessage(undefined);
    try {
      await service.openDataDirectory();
      if (
        !isSettingsOperationCurrent(
          mountedRef.current,
          requestKey,
          directoryKeyRef.current,
        )
      ) {
        return;
      }
      setDirectoryStatus("success");
      setDirectoryMessage("已交给 Windows 打开数据目录。");
    } catch (error) {
      if (
        !isSettingsOperationCurrent(
          mountedRef.current,
          requestKey,
          directoryKeyRef.current,
        )
      ) {
        return;
      }
      setDirectoryStatus("error");
      setDirectoryMessage(`打开失败：${errorMessage(error)}`);
    }
  }

  async function createDatabaseBackup() {
    if (!service || backupStatus === "loading") return;
    const requestKey = backupKeyRef.current + 1;
    backupKeyRef.current = requestKey;
    setBackupStatus("loading");
    setBackupResult(undefined);
    setBackupMessage(undefined);
    try {
      const result = await service.createDatabaseBackup(suggestedBackupFileName());
      if (
        !isSettingsOperationCurrent(
          mountedRef.current,
          requestKey,
          backupKeyRef.current,
        )
      ) {
        return;
      }
      if (!result) {
        setBackupStatus("idle");
        return;
      }
      setBackupResult(result);
      setBackupStatus("success");
      setBackupMessage(`备份完成：${result.fileName}（${formatByteSize(result.byteSize)}）`);
    } catch (error) {
      if (
        !isSettingsOperationCurrent(
          mountedRef.current,
          requestKey,
          backupKeyRef.current,
        )
      ) {
        return;
      }
      setBackupStatus("error");
      setBackupMessage(`备份失败：${errorMessage(error)}`);
    }
  }

  function selectUsageRange(nextRange: ModelUsageRange) {
    if (nextRange === usageRange) {
      setUsageRetryToken((token) => token + 1);
      return;
    }
    setUsageRange(nextRange);
  }

  async function savePreferences(next: AppPreferences) {
    if (
      !service ||
      !onPreferencesSave ||
      !snapshot ||
      preferenceStatus === "loading"
    ) {
      return;
    }
    const previous = snapshot.preferences;
    const shortcutsChanged =
      next.quickQueryShortcut !== previous.quickQueryShortcut ||
      next.selectionExplanationShortcut !== previous.selectionExplanationShortcut;
    if (!desktopSaveCoordinator.recordMutation()) return;
    const requestKey = preferenceKeyRef.current + 1;
    preferenceKeyRef.current = requestKey;
    setPreferenceStatus("loading");
    setPreferenceMessage(undefined);
    setFailedPreferences(undefined);
    setSnapshot((current) => (current ? { ...current, preferences: next } : current));

    try {
      const outcome = await onPreferencesSave(next, previous);
      if (
        !isSettingsOperationCurrent(
          mountedRef.current,
          requestKey,
          preferenceKeyRef.current,
        )
      ) {
        return;
      }
      if (outcome.status === "superseded") {
        setPreferenceStatus("idle");
        return;
      }
      if (outcome.status === "failed") {
        setSnapshot((current) =>
          current ? { ...current, preferences: outcome.preferences } : current,
        );
        setFailedPreferences(outcome.retryPreferences);
        setPreferenceStatus("error");
        setPreferenceMessage(outcome.message);
        if (shortcutsChanged) {
          setShortcutError(outcome.message);
        }
        return;
      }
      setSnapshot((current) =>
        current ? { ...current, preferences: outcome.preferences } : current,
      );
      if (shortcutsChanged) {
        try {
          const refreshed = await service.loadSettings();
          if (
            !isSettingsOperationCurrent(
              mountedRef.current,
              requestKey,
              preferenceKeyRef.current,
            )
          ) {
            return;
          }
          setSnapshot((current) => current ? {
            ...current,
            preferences: outcome.preferences,
            shortcutRegistrationError: refreshed.shortcutRegistrationError,
          } : current);
          setShortcutError(refreshed.shortcutRegistrationError ?? undefined);
        } catch (error) {
          if (
            isSettingsOperationCurrent(
              mountedRef.current,
              requestKey,
              preferenceKeyRef.current,
            )
          ) {
            setShortcutError(`快捷键已保存，但无法读取剩余注册状态：${errorMessage(error)}`);
          }
        }
      }
      setPreferenceStatus("success");
      setPreferenceMessage("已保存并应用。");
    } catch (error) {
      if (
        !isSettingsOperationCurrent(
          mountedRef.current,
          requestKey,
          preferenceKeyRef.current,
        )
      ) {
        return;
      }
      setFailedPreferences({ ...next, revision: previous.revision });
      setPreferenceStatus("error");
      setPreferenceMessage(
        `保存失败：${errorMessage(error)}`,
      );
      if (shortcutsChanged) {
        setShortcutError(`快捷键保存失败：${errorMessage(error)}`);
      }
    } finally {
      if (
        isSettingsOperationCurrent(
          mountedRef.current,
          requestKey,
          preferenceKeyRef.current,
        )
      ) {
        setPreferenceStatus((current) =>
          current === "loading" ? "error" : current,
        );
      }
    }
  }

  function patchPreferences(patch: Partial<AppPreferences>) {
    if (!snapshot) return;
    void savePreferences({ ...snapshot.preferences, ...patch });
  }

  async function toggleAutostart() {
    if (!service || !snapshot || autostartStatus === "loading") return;
    const requested = !snapshot.autostartEnabled;
    const requestKey = autostartKeyRef.current + 1;
    autostartKeyRef.current = requestKey;
    setAutostartStatus("loading");
    setAutostartMessage(undefined);
    try {
      const enabled = await trackSettingsOperation(
        "开机启动操作",
        () => service.setAutostartEnabled(requested),
      );
      if (
        !isSettingsOperationCurrent(
          mountedRef.current,
          requestKey,
          autostartKeyRef.current,
        )
      ) return;
      setSnapshot((current) => current ? { ...current, autostartEnabled: enabled } : current);
      setAutostartStatus("success");
      setAutostartMessage(enabled ? "已启用 Windows 开机启动。" : "已关闭 Windows 开机启动。");
    } catch (error) {
      if (
        !isSettingsOperationCurrent(
          mountedRef.current,
          requestKey,
          autostartKeyRef.current,
        )
      ) return;
      let detail = errorMessage(error);
      try {
        const authoritative = await service.loadAutostartEnabled();
        if (
          !isSettingsOperationCurrent(
            mountedRef.current,
            requestKey,
            autostartKeyRef.current,
          )
        ) return;
        setSnapshot((current) => current ? { ...current, autostartEnabled: authoritative } : current);
      } catch (readError) {
        detail += `；重新读取失败：${errorMessage(readError)}`;
      }
      setAutostartStatus("error");
      setAutostartMessage(`开机启动修改失败：${detail}`);
    }
  }

  async function runThemeMutation(
    label: string,
    retry: ThemeMutationRetry,
    start: () => Promise<ThemeMutationOutcome>,
    successMessage: (outcome: Extract<ThemeMutationOutcome, { status: "saved" }>) => string,
  ) {
    if (themeStatus === "loading") return;
    const requestKey = themeKeyRef.current + 1;
    themeKeyRef.current = requestKey;
    setThemeStatus("loading");
    setThemeMessage(undefined);
    setThemeRetry(undefined);
    try {
      const outcome = await trackSettingsOperation(label, start);
      if (!isSettingsOperationCurrent(mountedRef.current, requestKey, themeKeyRef.current)) {
        return;
      }
      switch (outcome.status) {
        case "saved":
          setThemeStatus("success");
          setThemeMessage(successMessage(outcome));
          break;
        case "failed":
          setThemeStatus("error");
          setThemeMessage(outcome.message);
          setThemeRetry(outcome.retry);
          break;
        case "conflict":
          setThemeStatus("error");
          setThemeMessage(outcome.message);
          break;
        case "cancelled":
          setThemeStatus("idle");
          break;
        case "superseded":
          setThemeStatus("idle");
          break;
      }
    } catch (error) {
      if (!isSettingsOperationCurrent(mountedRef.current, requestKey, themeKeyRef.current)) {
        return;
      }
      setThemeStatus("error");
      setThemeMessage(`${label}失败：${errorMessage(error)}`);
      setThemeRetry(retry);
    } finally {
      if (!isSettingsOperationCurrent(mountedRef.current, requestKey, themeKeyRef.current)) {
        return;
      }
      setThemeStatus((current) => current === "loading" ? "error" : current);
    }
  }

  function selectTheme(themeId: string, mode?: ThemeMode) {
    const theme = themeController.snapshot.themes.find(
      (candidate) => candidate.manifest.id === themeId,
    );
    if (!theme) return;
    const selectedMode = mode ?? (
      theme.manifest.modes.includes(themeController.snapshot.currentMode)
        ? themeController.snapshot.currentMode
        : theme.manifest.modes[0]
    );
    if (
      themeId === themeController.snapshot.currentThemeId &&
      selectedMode === themeController.snapshot.currentMode
    ) return;
    void runThemeMutation(
      "主题选择",
      { kind: "select", themeId, mode: selectedMode },
      () => themeController.select(themeId, selectedMode),
      () => `已应用 ${theme.manifest.name} · ${selectedMode === "light" ? "浅色" : "深色"}。`,
    );
  }

  function retryThemeMutation() {
    if (!themeRetry) return;
    void runThemeMutation(
      "主题操作重试",
      themeRetry,
      () => themeController.retry(themeRetry),
      () => "主题操作重试成功。",
    );
  }

  if (!service) {
    return (
      <main className="rr-main-panel rr-settings-page" aria-label="ReadRay 设置">
        <div className="rr-settings-load-error" role="status">
          <strong>设置仅在 ReadRay 桌面应用中可用</strong>
          <p>浏览器预览不会读取或模拟 Windows 凭据与本地数据库。</p>
        </div>
      </main>
    );
  }

  if (loadStatus === "loading") {
    return (
      <main className="rr-main-panel rr-settings-page" aria-label="ReadRay 设置">
        <SettingsLoading />
      </main>
    );
  }

  if (loadStatus === "error" || !snapshot) {
    return (
      <main className="rr-main-panel rr-settings-page" aria-label="ReadRay 设置">
        <div className="rr-settings-load-error" role="alert">
          <strong>暂时无法读取设置</strong>
          <p>{loadError}</p>
          <button type="button" onClick={() => setRetryToken((token) => token + 1)}>
            重试
          </button>
        </div>
      </main>
    );
  }

  const balanceStatus = balanceState.status;
  const balance = balanceState.value;
  const validationMessage =
    operation === "saving"
      ? "正在验证 DeepSeek 连接，成功后才会替换现有配置…"
      : operationError ?? operationMessage;
  const selectedTheme = themeController.snapshot.themes.find(
    (theme) => theme.manifest.id === themeController.snapshot.currentThemeId,
  );
  const themeBusy =
    themeStatus === "loading" || themeController.status === "loading";

  return (
    <main className="rr-main-panel rr-settings-page" aria-label="ReadRay 设置">
      <div className="rr-settings-shell">
        <aside className="rr-settings-nav" aria-label="设置分类">
          <div className="rr-settings-nav-inner">
            {settingsSections.map(([id, label]) => (
              <button
                type="button"
                className={activeSection === id ? "is-active" : undefined}
                aria-current={activeSection === id ? "page" : undefined}
                key={id}
                onClick={() => selectSection(id)}
              >
                {label}
              </button>
            ))}
          </div>
        </aside>

        <div className="rr-settings-scroll" ref={scrollRef}>
          <div className="rr-settings-content">
            {activeSection === "general" ? (
              <section className="rr-settings-section" aria-labelledby="rr-settings-general-heading">
                <SettingsHeader
                  id="rr-settings-general-heading"
                  title="通用"
                />

                <div className="rr-settings-group">
                  <GroupHeading title="语言与输入" meta="单行解释查询固定使用 Enter" />
                  <div className="rr-settings-panel">
                    <div className="rr-settings-row">
                      <SettingsCopy label="界面语言" />
                      <div className="rr-settings-stack-control">
                        <SettingsSelect
                          className="rr-settings-language-select"
                          label="界面语言"
                          value="zh-CN"
                          options={[{ value: "zh-CN", label: "简体中文" }]}
                          disabled
                          onChange={() => undefined}
                        />
                      </div>
                    </div>
                    <div className="rr-settings-row">
                      <SettingsCopy
                        label="发送快捷键"
                      />
                      <div className="rr-settings-stack-control">
                        <SettingsSelect
                          className="rr-settings-send-select"
                          label="发送快捷键"
                          value={snapshot.preferences.sendShortcut}
                          options={[
                            { value: "enter", label: "Enter 发送" },
                            { value: "ctrlEnter", label: "Ctrl+Enter 发送" },
                          ]}
                          disabled={preferenceStatus === "loading"}
                          onChange={(value) =>
                            patchPreferences({
                              sendShortcut: value as AppPreferences["sendShortcut"],
                            })
                          }
                        />
                        <div className="rr-settings-status-line">
                          {snapshot.preferences.sendShortcut === "enter"
                            ? "Enter 发送，Shift+Enter 换行"
                            : "Ctrl+Enter 发送，Enter 换行"}
                        </div>
                      </div>
                    </div>
                  </div>
                  {preferenceMessage ? (
                    <div
                      className={`rr-settings-inline-message ${
                        preferenceStatus === "error" ? "is-error" : "is-success"
                      }`}
                      role={preferenceStatus === "error" ? "alert" : "status"}
                    >
                      <span>{preferenceMessage}</span>
                      {preferenceStatus === "error" && failedPreferences ? (
                        <button
                          type="button"
                          onClick={() => void savePreferences(failedPreferences)}
                        >
                          重试
                        </button>
                      ) : null}
                    </div>
                  ) : null}
                </div>

                <div className="rr-settings-group">
                  <GroupHeading title="全局快捷键" meta="按下 Esc 可取消录制" />
                  <div className="rr-settings-panel">
                    <ShortcutRow
                      name="快速查询"
                      value={snapshot.preferences.quickQueryShortcut}
                      recording={recordingShortcut === "quickQueryShortcut"}
                      disabled={preferenceStatus === "loading"}
                      isDefault={snapshot.preferences.quickQueryShortcut === DEFAULT_APP_PREFERENCES.quickQueryShortcut}
                      onRecord={() => {
                        setShortcutError(undefined);
                        setRecordingShortcut("quickQueryShortcut");
                      }}
                      onRestore={() => patchPreferences({
                        quickQueryShortcut: DEFAULT_APP_PREFERENCES.quickQueryShortcut,
                      })}
                    />
                    <ShortcutRow
                      name="选区解释"
                      value={snapshot.preferences.selectionExplanationShortcut}
                      recording={recordingShortcut === "selectionExplanationShortcut"}
                      disabled={preferenceStatus === "loading"}
                      isDefault={snapshot.preferences.selectionExplanationShortcut === DEFAULT_APP_PREFERENCES.selectionExplanationShortcut}
                      onRecord={() => {
                        setShortcutError(undefined);
                        setRecordingShortcut("selectionExplanationShortcut");
                      }}
                      onRestore={() => patchPreferences({
                        selectionExplanationShortcut: DEFAULT_APP_PREFERENCES.selectionExplanationShortcut,
                      })}
                    />
                  </div>
                  {shortcutError ? (
                    <div className="rr-settings-inline-message is-error" role="alert">
                      <span>{shortcutError}</span>
                    </div>
                  ) : null}
                </div>

                <div className="rr-settings-group">
                  <GroupHeading title="启动与关闭" />
                  <div className="rr-settings-panel">
                    <div className="rr-settings-row">
                      <SettingsCopy label="开机启动" />
                      <div className="rr-settings-stack-control">
                        <div className="rr-settings-control">
                          <button
                            className={`rr-settings-switch${snapshot.autostartEnabled ? " is-on" : ""}`}
                            type="button"
                            role="switch"
                            aria-checked={snapshot.autostartEnabled}
                            aria-label="开机启动"
                            disabled={autostartStatus === "loading"}
                            onClick={() => void toggleAutostart()}
                          />
                          <span>{snapshot.autostartEnabled ? "已开启" : "已关闭"}</span>
                        </div>
                        {autostartMessage ? (
                          <div
                            className={`rr-settings-action-status ${autostartStatus === "error" ? "is-error" : "is-success"}`}
                            role={autostartStatus === "error" ? "alert" : "status"}
                          >
                            {autostartMessage}
                          </div>
                        ) : null}
                      </div>
                    </div>
                    <div className="rr-settings-row">
                      <SettingsCopy label="关闭主窗口时" />
                      <div className="rr-settings-stack-control">
                        <SettingsSelect
                          className="rr-settings-close-select"
                          label="关闭主窗口时"
                          value={snapshot.preferences.closeBehavior}
                          options={[
                            { value: "hideToTray", label: "隐藏到托盘" },
                            { value: "exit", label: "退出 ReadRay" },
                          ]}
                          disabled={preferenceStatus === "loading"}
                          onChange={(value) => patchPreferences({
                            closeBehavior: value as AppPreferences["closeBehavior"],
                          })}
                        />
                      </div>
                    </div>
                  </div>
                </div>

              </section>
            ) : null}

            {activeSection === "appearance" ? (
              <section className="rr-settings-section" aria-labelledby="rr-settings-appearance-heading">
                <SettingsHeader
                  id="rr-settings-appearance-heading"
                  title="外观"
                />

                <div className="rr-settings-group">
                  <GroupHeading title="主题" />
                  <div className="rr-settings-panel">
                    <div className="rr-settings-row">
                      <SettingsCopy
                        label="当前主题"
                        help={selectedTheme
                          ? `${selectedTheme.manifest.author} · ${selectedTheme.manifest.version}`
                          : "数据库主题暂不可用"}
                      />
                      <div className="rr-settings-stack-control">
                        <div className="rr-settings-appearance-actions">
                          <SettingsSelect
                            className="rr-settings-theme-select"
                            label="主题"
                            value={themeController.snapshot.currentThemeId}
                            options={themeController.snapshot.themes.map((theme) => ({
                              value: theme.manifest.id,
                              label: theme.manifest.name,
                            }))}
                            disabled={themeBusy || themeController.status === "error"}
                            onChange={(value) => selectTheme(value)}
                          />
                          <SettingsSelect
                            className="rr-settings-theme-mode-select"
                            label="主题模式"
                            value={themeController.snapshot.currentMode}
                            options={
                              selectedTheme?.manifest.modes.map((mode) => ({
                                value: mode,
                                label: mode === "light" ? "浅色" : "深色",
                              })) ?? []
                            }
                            disabled={
                              themeBusy ||
                              themeController.status === "error" ||
                              (selectedTheme?.manifest.modes.length ?? 0) <= 1
                            }
                            onChange={(value) => selectTheme(
                              themeController.snapshot.currentThemeId,
                              value as ThemeMode,
                            )}
                          />
                        </div>
                      </div>
                    </div>
                  </div>
                  {themeController.status === "error" ? (
                    <div className="rr-settings-inline-message is-error" role="alert">
                      <span>主题读取失败，已保持当前配色：{themeController.error}</span>
                      <button type="button" onClick={() => void themeController.reload()}>
                        重试读取
                      </button>
                    </div>
                  ) : null}
                  {themeMessage ? (
                    <div
                      className={`rr-settings-inline-message ${
                        themeStatus === "error" ? "is-error" : "is-success"
                      }`}
                      role={themeStatus === "error" ? "alert" : "status"}
                    >
                      <span>{themeMessage}</span>
                      {themeStatus === "error" && themeRetry ? (
                        <button type="button" onClick={retryThemeMutation}>
                          重试
                        </button>
                      ) : null}
                    </div>
                  ) : null}
                </div>

                <div className="rr-settings-group">
                  <GroupHeading title="字体" meta="使用具体字号" />
                  <div className="rr-settings-panel">
                    <div className="rr-settings-row">
                      <SettingsCopy label="界面字体" />
                      <SettingsSelect
                        className="rr-settings-font-field rr-settings-ui-font"
                        label="界面字体"
                        value={snapshot.preferences.uiFont}
                        options={[
                          { value: "geistSourceHanSans", label: "Geist + 思源黑体" },
                          { value: "sourceHanSans", label: "思源黑体" },
                        ]}
                        disabled={preferenceStatus === "loading"}
                        onChange={(value) =>
                          patchPreferences({
                            uiFont: value as AppPreferences["uiFont"],
                          })
                        }
                      />
                    </div>
                    <div className="rr-settings-row">
                      <SettingsCopy label="界面字号" />
                      <div className="rr-settings-font-size-control">
                        <label className="rr-settings-number-field">
                          <input
                            aria-label="界面字号"
                            type="number"
                            min={UI_FONT_SIZE_MIN}
                            max={UI_FONT_SIZE_MAX}
                            step={1}
                            value={snapshot.preferences.uiFontSize}
                            disabled={preferenceStatus === "loading"}
                            onChange={(event) => {
                              const value = parseFontSizeCandidate(
                                event.currentTarget.value,
                                UI_FONT_SIZE_MIN,
                                UI_FONT_SIZE_MAX,
                              );
                              if (value !== undefined) {
                                patchPreferences({ uiFontSize: value });
                              } else {
                                setFailedPreferences(undefined);
                                setPreferenceStatus("error");
                                setPreferenceMessage(
                                  `界面字号必须是 ${UI_FONT_SIZE_MIN}–${UI_FONT_SIZE_MAX} px 之间的整数。`,
                                );
                              }
                            }}
                          />
                          <span>px</span>
                        </label>
                        <button
                          className="rr-settings-restore"
                          type="button"
                          disabled={
                            preferenceStatus === "loading" ||
                            snapshot.preferences.uiFontSize ===
                              DEFAULT_APP_PREFERENCES.uiFontSize
                          }
                          onClick={() =>
                            patchPreferences({
                              uiFontSize: DEFAULT_APP_PREFERENCES.uiFontSize,
                            })
                          }
                        >
                          恢复默认
                        </button>
                      </div>
                    </div>
                    <div className="rr-settings-row">
                      <SettingsCopy label="学习内容字体" />
                      <SettingsSelect
                        className="rr-settings-font-field rr-settings-learning-font"
                        label="学习内容字体"
                        value={snapshot.preferences.learningFont}
                        options={[
                          {
                            value: "newsreaderSourceHanSerif",
                            label: "Newsreader + 思源宋体",
                          },
                          { value: "sourceHanSerif", label: "思源宋体" },
                        ]}
                        disabled={preferenceStatus === "loading"}
                        onChange={(value) =>
                          patchPreferences({
                            learningFont: value as AppPreferences["learningFont"],
                          })
                        }
                      />
                    </div>
                    <div className="rr-settings-row">
                      <SettingsCopy label="学习内容字号" />
                      <div className="rr-settings-font-size-control">
                        <label className="rr-settings-number-field">
                          <input
                            aria-label="学习内容字号"
                            type="number"
                            min={LEARNING_FONT_SIZE_MIN}
                            max={LEARNING_FONT_SIZE_MAX}
                            step={1}
                            value={snapshot.preferences.learningFontSize}
                            disabled={preferenceStatus === "loading"}
                            onChange={(event) => {
                              const value = parseFontSizeCandidate(
                                event.currentTarget.value,
                                LEARNING_FONT_SIZE_MIN,
                                LEARNING_FONT_SIZE_MAX,
                              );
                              if (value !== undefined) {
                                patchPreferences({ learningFontSize: value });
                              } else {
                                setFailedPreferences(undefined);
                                setPreferenceStatus("error");
                                setPreferenceMessage(
                                  `学习内容字号必须是 ${LEARNING_FONT_SIZE_MIN}–${LEARNING_FONT_SIZE_MAX} px 之间的整数。`,
                                );
                              }
                            }}
                          />
                          <span>px</span>
                        </label>
                        <button
                          className="rr-settings-restore"
                          type="button"
                          disabled={
                            preferenceStatus === "loading" ||
                            snapshot.preferences.learningFontSize ===
                              DEFAULT_APP_PREFERENCES.learningFontSize
                          }
                          onClick={() =>
                            patchPreferences({
                              learningFontSize:
                                DEFAULT_APP_PREFERENCES.learningFontSize,
                            })
                          }
                        >
                          恢复默认
                        </button>
                      </div>
                    </div>
                  </div>
                  {preferenceMessage ? (
                    <div
                      className={`rr-settings-inline-message ${
                        preferenceStatus === "error" ? "is-error" : "is-success"
                      }`}
                      role={preferenceStatus === "error" ? "alert" : "status"}
                    >
                      <span>{preferenceMessage}</span>
                      {preferenceStatus === "error" && failedPreferences ? (
                        <button
                          type="button"
                          onClick={() => void savePreferences(failedPreferences)}
                        >
                          重试
                        </button>
                      ) : null}
                    </div>
                  ) : null}
                </div>
              </section>
            ) : null}

            {activeSection === "ai" ? (
              <section className="rr-settings-section" aria-labelledby="rr-settings-ai-heading">
                <SettingsHeader
                  id="rr-settings-ai-heading"
                  title="AI 服务"
                />

                <div className="rr-settings-provider-head">
                  <div className="rr-settings-provider-title">
                    <span className="rr-settings-provider-logo">D</span>
                    <div>
                      <strong>DeepSeek</strong>
                    </div>
                  </div>
                  <span
                    className={`rr-settings-pill ${
                      snapshot.apiKeyConfigured ? "is-success" : "is-warn"
                    }`}
                  >
                    <span />
                    {snapshot.apiKeyConfigured ? "API Key 已配置" : "API Key 未配置"}
                  </span>
                </div>

                <div className="rr-settings-key-panel">
                  <div className="rr-settings-key-form">
                    <div className="rr-settings-key-copy">
                      <strong>API Key</strong>
                    </div>
                    {editingKey ? (
                      <form className="rr-settings-key-actions" onSubmit={saveApiKey}>
                        <input
                          className="rr-settings-text-input"
                          type="password"
                          value={keyDraft}
                          autoComplete="off"
                          spellCheck={false}
                          aria-label="DeepSeek API Key"
                          placeholder="填写 DeepSeek API Key"
                          disabled={operation !== "idle"}
                          onChange={(event) => {
                            setKeyDraft(event.target.value);
                            setOperationError(undefined);
                            setOperationMessage(undefined);
                          }}
                        />
                        <button
                          className="rr-settings-button is-primary"
                          type="submit"
                          disabled={operation !== "idle"}
                        >
                          {operation === "saving" ? "正在验证…" : "验证并保存"}
                        </button>
                        {snapshot.apiKeyConfigured ? (
                          <button
                            className="rr-settings-button is-ghost"
                            type="button"
                            disabled={operation !== "idle"}
                            onClick={() => {
                              setEditingKey(false);
                              setKeyDraft("");
                              setOperationError(undefined);
                            }}
                          >
                            取消
                          </button>
                        ) : null}
                      </form>
                    ) : (
                      <div className="rr-settings-key-actions">
                        <span className="rr-settings-masked-key" aria-label="API Key 已隐藏">
                          已配置 · 不返回明文
                        </span>
                        <button
                          className="rr-settings-button"
                          type="button"
                          onClick={() => {
                            setEditingKey(true);
                            setOperationError(undefined);
                            setOperationMessage(undefined);
                          }}
                        >
                          更新
                        </button>
                        <button
                          className="rr-settings-button is-danger"
                          type="button"
                          onClick={() => {
                            setConfirmingClear(true);
                            setOperationError(undefined);
                          }}
                        >
                          清除
                        </button>
                      </div>
                    )}
                  </div>
                  {validationMessage ? (
                    <div
                      className={`rr-settings-validation${
                        operation === "saving"
                          ? " is-loading"
                          : operationError
                            ? " is-error"
                            : operationMessage
                              ? " is-success"
                              : ""
                      }`}
                      aria-live="polite"
                    >
                      {validationMessage}
                    </div>
                  ) : null}
                </div>

                <div className="rr-settings-row rr-settings-model-row">
                  <SettingsCopy label="模型" />
                  <SettingsSelect
                    className="rr-settings-model-select"
                    label="DeepSeek 模型"
                    value={snapshot.model}
                    options={[{ value: snapshot.model, label: snapshot.model }]}
                    disabled
                    onChange={() => undefined}
                  />
                </div>

                <div className="rr-settings-balance-card">
                  <div>
                    <div className="rr-settings-balance-label">DEEPSEEK 账户余额</div>
                    {balance?.balances.length ? (
                      <div className="rr-settings-balance-list">
                        {balance.balances.map((item) => (
                          <div className="rr-settings-balance-item" key={item.currency}>
                            <div className="rr-settings-balance-value">
                              {item.totalBalance}<span>{item.currency}</span>
                            </div>
                          </div>
                        ))}
                      </div>
                    ) : (
                      <div className="rr-settings-balance-value">
                        {!snapshot.apiKeyConfigured
                          ? "配置 API Key 后可查询"
                          : balanceStatus === "loading"
                            ? "正在查询…"
                            : balanceStatus === "error"
                              ? "余额查询失败"
                              : balanceStatus === "success"
                                ? "未返回余额"
                                : "正在准备查询…"}
                      </div>
                    )}
                  </div>
                  <button
                    className="rr-settings-button"
                    type="button"
                    disabled={!snapshot.apiKeyConfigured || balanceStatus === "loading"}
                    onClick={refreshBalance}
                  >
                    {balanceStatus === "loading"
                      ? "正在刷新…"
                      : balanceStatus === "error"
                        ? "重试查询"
                        : "刷新余额"}
                  </button>
                </div>

                <div className="rr-settings-usage-card">
                  <div className="rr-settings-usage-head">
                    <div>
                      <strong>ReadRay 使用量</strong>
                      <span>仅统计 ReadRay 收到的真实 DeepSeek usage</span>
                    </div>
                    <div className="rr-settings-segmented" aria-label="使用量时间范围">
                      {usageRanges.map(([range, label]) => (
                        <button
                          className={range === usageRange ? "is-active" : undefined}
                          type="button"
                          aria-pressed={range === usageRange}
                          onClick={() => selectUsageRange(range)}
                          key={range}
                        >
                          {label}
                        </button>
                      ))}
                    </div>
                  </div>
                  <div className="rr-settings-usage-body">
                    <div className="rr-settings-usage-total-label">READRAY 使用 TOKEN</div>
                    <div className="rr-settings-usage-total">
                      {usageSummary ? formatUsageNumber(usageSummary.totalTokens) : "—"}
                      <span className="rr-settings-usage-unit">Token</span>
                    </div>
                    <div className="rr-settings-usage-meta-grid">
                      <div className="rr-settings-usage-meta-item">
                        <span>AI 请求次数</span>
                        <strong>
                          {usageSummary ? formatUsageNumber(usageSummary.requestCount) : "—"}
                        </strong>
                      </div>
                      <div className="rr-settings-usage-meta-item">
                        <span>统计开始日期</span>
                        <strong>
                          {usageSummary
                            ? formatUsageDate(usageSummary.statisticsStartUnixMs)
                            : "—"}
                        </strong>
                      </div>
                    </div>
                    <div className="rr-settings-usage-breakdown">
                      {(
                        [
                          ["explanation_query", "解释查询"],
                          ["quick_ai", "Quick AI"],
                          ["writing", "写作"],
                          ["review_card", "复习制卡"],
                        ] as const
                      ).map(([category, label]) => {
                        const item = usageSummary
                          ? usageCategory(usageSummary, category)
                          : undefined;
                        return (
                          <div key={category}>
                            <span>{label}</span>
                            <strong>
                              {item ? formatUsageNumber(item.totalTokens) : "—"}
                            </strong>
                            <small>
                              {item
                                ? `${formatUsageNumber(item.requestCount)} 次 · 输入 ${formatUsageNumber(
                                    item.promptTokens,
                                  )} / 输出 ${formatUsageNumber(item.completionTokens)}`
                                : "—"}
                            </small>
                          </div>
                        );
                      })}
                    </div>
                    {usageStatus === "loading" ? (
                      <div className="rr-settings-usage-status" role="status">
                        正在读取本地使用量…
                      </div>
                    ) : null}
                    {usageStatus === "error" ? (
                      <div className="rr-settings-usage-status is-error" role="alert">
                        <span>读取失败：{usageError}</span>
                        <button
                          className="rr-settings-button"
                          type="button"
                          onClick={() => setUsageRetryToken((token) => token + 1)}
                        >
                          重试读取
                        </button>
                      </div>
                    ) : null}
                  </div>
                </div>
              </section>
            ) : null}

            {activeSection === "data" ? (
              <section className="rr-settings-section" aria-labelledby="rr-settings-data-heading">
                <SettingsHeader
                  id="rr-settings-data-heading"
                  title="本地数据"
                />

                <div className="rr-settings-group">
                  <GroupHeading title="数据目录" />
                  <div className="rr-settings-panel">
                    <div className="rr-settings-row">
                      <SettingsCopy label="ReadRay 数据目录" />
                      <div className="rr-settings-stack-control">
                        <div className="rr-settings-path-actions">
                          <div className="rr-settings-path-box" title={snapshot.appDataDirectory}>
                            <FolderIcon />
                            <span>{snapshot.appDataDirectory}</span>
                          </div>
                          <button
                            className="rr-settings-button"
                            type="button"
                            disabled={directoryStatus === "loading"}
                            onClick={() => void openDataDirectory()}
                          >
                            {directoryStatus === "loading"
                              ? "正在打开…"
                              : directoryStatus === "error"
                                ? "重试打开"
                              : "打开数据目录"}
                          </button>
                        </div>
                        {directoryMessage ? (
                          <div
                            className={`rr-settings-action-status ${
                              directoryStatus === "error" ? "is-error" : "is-success"
                            }`}
                            role={directoryStatus === "error" ? "alert" : "status"}
                          >
                            {directoryMessage}
                          </div>
                        ) : null}
                      </div>
                    </div>
                    <div className="rr-settings-row">
                      <SettingsCopy label="数据概览" />
                      <div className="rr-settings-counts">
                        <div><span>学习记录</span><strong>{snapshot.learningRecordCount}</strong></div>
                        <div><span>对话</span><strong>{snapshot.conversationCount}</strong></div>
                        <div><span>写作</span><strong>{snapshot.writingDocumentCount}</strong></div>
                      </div>
                    </div>
                  </div>
                </div>

                <div className="rr-settings-group">
                  <GroupHeading title="备份" />
                  <div className="rr-settings-panel">
                    <div className="rr-settings-backup-panel">
                      <div className="rr-settings-backup-head">
                        <div>
                          <strong>备份全部 ReadRay 数据</strong>
                          <p>不包含 API Key。</p>
                        </div>
                        <button
                          className="rr-settings-button is-primary"
                          type="button"
                          disabled={backupStatus === "loading"}
                          onClick={() => void createDatabaseBackup()}
                        >
                          {backupStatus === "loading"
                            ? "正在备份…"
                            : backupStatus === "error"
                              ? "重试备份"
                              : "开始备份"}
                        </button>
                      </div>
                      {backupMessage ? (
                        <div
                          className={`rr-settings-action-status ${
                            backupStatus === "error" ? "is-error" : "is-success"
                          }`}
                          role={backupStatus === "error" ? "alert" : "status"}
                          title={backupResult?.filePath}
                        >
                          {backupMessage}
                        </div>
                      ) : null}
                    </div>
                  </div>
                </div>

              </section>
            ) : null}

            {activeSection === "about" ? (
              <section className="rr-settings-section" aria-labelledby="rr-settings-about-heading">
                <SettingsHeader
                  id="rr-settings-about-heading"
                  title="关于"
                />
                <div className="rr-settings-about">
                  <span className="rr-settings-about-mark">R</span>
                  <div>
                    <h2>ReadRay</h2>
                    <p>Windows-first、本地数据优先的英语学习 Agent。</p>
                  </div>
                  <code>{snapshot.appVersion}</code>
                </div>
                <button
                  className="rr-settings-link-row"
                  type="button"
                  onClick={() => setShowingLicenses(true)}
                >
                  <strong>许可证与第三方材料</strong>
                  <ArrowIcon />
                </button>
                <div className="rr-settings-link-row is-static">
                  <strong>更新</strong>
                  <span className="rr-settings-pill is-warn">未来位置</span>
                </div>
              </section>
            ) : null}
          </div>
        </div>
      </div>

      {confirmingClear ? (
        <div className="rr-settings-dialog-backdrop">
          <section
            className="rr-settings-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="rr-settings-clear-title"
          >
            <h2 id="rr-settings-clear-title">清除 DeepSeek API Key？</h2>
            <p>
              清除后本地内容仍可查看，但解释、Quick AI 与写作 AI 功能将不可用。
            </p>
            {operationError ? <div className="rr-settings-dialog-error">{operationError}</div> : null}
            <div className="rr-settings-dialog-actions">
              <button
                className="rr-settings-button"
                type="button"
                disabled={operation === "clearing"}
                onClick={() => {
                  setConfirmingClear(false);
                  setOperationError(undefined);
                }}
              >
                取消
              </button>
              <button
                className="rr-settings-button is-danger"
                type="button"
                disabled={operation === "clearing"}
                onClick={() => void clearApiKey()}
              >
                {operation === "clearing" ? "正在清除…" : operationError ? "重试清除" : "确认清除"}
              </button>
            </div>
          </section>
        </div>
      ) : null}

      {showingLicenses ? (
        <div className="rr-settings-dialog-backdrop">
          <section
            className="rr-settings-dialog rr-settings-license-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="rr-settings-license-title"
          >
            <div className="rr-settings-license-head">
              <div>
                <h2 id="rr-settings-license-title">许可证与第三方材料</h2>
                <p>以下材料随 ReadRay 字体与主题资源一起打包，可展开查看完整许可证文本。</p>
              </div>
              <button
                className="rr-settings-button"
                type="button"
                onClick={() => setShowingLicenses(false)}
              >
                关闭
              </button>
            </div>
            <div className="rr-settings-license-list">
              {licenseMaterials.map((material) => (
                <details key={material.name}>
                  <summary>
                    <strong>{material.name}</strong>
                    <span>{material.license}</span>
                  </summary>
                  <pre>{material.text}</pre>
                </details>
              ))}
            </div>
          </section>
        </div>
      ) : null}
    </main>
  );
}

export default SettingsPage;
