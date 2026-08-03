import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from "react";
import type { SettingsService } from "../settingsService";
import {
  isSettingsOperationCurrent,
  validateApiKeyDraft,
  type SettingsSnapshot,
} from "../settingsViewModel";
import geistLicense from "../assets/fonts/licenses/Geist-OFL.txt?raw";
import newsreaderLicense from "../assets/fonts/licenses/Newsreader-OFL.txt?raw";
import sourceHanSansLicense from "../assets/fonts/licenses/Source-Han-Sans-OFL.txt?raw";
import sourceHanSerifLicense from "../assets/fonts/licenses/Source-Han-Serif-OFL.txt?raw";

type SettingsSection = "general" | "appearance" | "ai" | "data" | "about";
type OperationState = "idle" | "saving" | "clearing";

type SettingsPageProps = {
  service: SettingsService | null;
};

const settingsSections: ReadonlyArray<readonly [SettingsSection, string]> = [
  ["general", "通用"],
  ["appearance", "外观"],
  ["ai", "AI 服务"],
  ["data", "数据"],
  ["about", "关于"],
];

const licenseMaterials = [
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

function InfoIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="12" cy="12" r="9" />
      <path d="M12 10.8v5.2M12 7.7h.01" />
    </svg>
  );
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

function SettingsHeader({
  title,
  meta,
  id,
}: {
  title: string;
  meta: string;
  id: string;
}) {
  return (
    <header className="rr-settings-header">
      <h1 id={id}>{title}</h1>
      <span>{meta}</span>
    </header>
  );
}

function GroupHeading({ title, meta }: { title: string; meta: string }) {
  return (
    <div className="rr-settings-group-heading">
      <h2>{title}</h2>
      <p>{meta}</p>
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

function FutureNote({ children }: { children: ReactNode }) {
  return (
    <div className="rr-settings-future-note">
      <InfoIcon />
      <span>{children}</span>
    </div>
  );
}

function UnavailableButton({
  children,
  className = "",
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <button
      className={`rr-settings-button rr-settings-unavailable ${className}`.trim()}
      type="button"
      disabled
      title="留给后续独立任务，本轮不可操作"
    >
      {children}
    </button>
  );
}

function ShortcutRow({ name, keyName }: { name: string; keyName: "R" | "U" }) {
  return (
    <div className="rr-settings-shortcut-row">
      <div className="rr-settings-shortcut-name">{name}</div>
      <div className="rr-settings-shortcut-actions">
        <div className="rr-settings-shortcut-value" aria-label={`Ctrl Alt ${keyName}`}>
          <kbd>Ctrl</kbd>
          <kbd>Alt</kbd>
          <kbd>{keyName}</kbd>
        </div>
        <UnavailableButton>录制新快捷键</UnavailableButton>
        <UnavailableButton className="is-ghost">禁用</UnavailableButton>
        <button
          className="rr-settings-restore"
          type="button"
          disabled
          title="当前已是默认值"
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

function SettingsPage({ service }: SettingsPageProps) {
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
  const mountedRef = useRef(false);
  const operationKeyRef = useRef(0);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      operationKeyRef.current += 1;
    };
  }, []);

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
      const nextSnapshot = await service.validateAndSaveApiKey(keyDraft);
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
      const nextSnapshot = await service.clearApiKey();
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

  const sourceCopy =
    snapshot.apiKeySource === "credential"
      ? "已安全保存在当前 Windows 用户的凭据管理器中。"
      : snapshot.apiKeySource === "environment"
        ? "当前由开发环境配置提供；更新后会改用 Windows 凭据管理器。"
        : "未配置时仍可查看本地内容，AI 功能会引导回到这里。";

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
                  meta="语言、快捷操作与桌面行为"
                />

                <div className="rr-settings-group">
                  <GroupHeading title="语言与输入" meta="单行解释查询固定使用 Enter" />
                  <div className="rr-settings-panel">
                    <div className="rr-settings-row">
                      <SettingsCopy label="界面语言" />
                      <div className="rr-settings-stack-control">
                        <select
                          className="rr-settings-select rr-settings-language-select"
                          aria-label="界面语言"
                          value="zh-CN"
                          disabled
                        >
                          <option value="zh-CN">简体中文</option>
                        </select>
                      </div>
                    </div>
                    <div className="rr-settings-row">
                      <SettingsCopy
                        label="发送快捷键"
                        help="适用于“今天”、完整对话、overlay Quick AI 和写作辅导。"
                      />
                      <div className="rr-settings-stack-control">
                        <select
                          className="rr-settings-select rr-settings-send-select"
                          aria-label="发送快捷键"
                          value="enter"
                          disabled
                        >
                          <option value="enter">Enter 发送</option>
                        </select>
                        <div className="rr-settings-status-line">
                          默认：Enter 发送，Shift+Enter 换行
                        </div>
                      </div>
                    </div>
                  </div>
                </div>

                <div className="rr-settings-group">
                  <GroupHeading title="全局快捷键" meta="当前只读展示；桌面生命周期任务后开放编辑" />
                  <div className="rr-settings-panel">
                    <ShortcutRow name="快速查询" keyName="R" />
                    <ShortcutRow name="选区解释" keyName="U" />
                  </div>
                </div>

                <div className="rr-settings-group">
                  <GroupHeading title="启动与关闭" meta="桌面生命周期能力留给后续独立任务" />
                  <div className="rr-settings-panel">
                    <div className="rr-settings-row">
                      <SettingsCopy label="开机启动" />
                      <div className="rr-settings-stack-control">
                        <div className="rr-settings-control">
                          <button
                            className="rr-settings-switch"
                            type="button"
                            aria-label="开机启动尚未开放"
                            disabled
                          />
                          <span>尚未开放</span>
                        </div>
                      </div>
                    </div>
                    <div className="rr-settings-row">
                      <SettingsCopy label="关闭主窗口时" />
                      <div className="rr-settings-stack-control">
                        <select
                          className="rr-settings-select rr-settings-close-select"
                          aria-label="关闭主窗口时"
                          value="current"
                          disabled
                        >
                          <option value="current">当前窗口行为</option>
                        </select>
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
                  meta="主题、字体与字号"
                />

                <div className="rr-settings-group">
                  <GroupHeading title="主题" meta="当前主题只读显示" />
                  <div className="rr-settings-panel">
                    <div className="rr-settings-row">
                      <SettingsCopy label="当前主题" />
                      <div className="rr-settings-stack-control">
                        <div className="rr-settings-appearance-actions">
                          <select
                            className="rr-settings-select rr-settings-theme-select"
                            aria-label="主题"
                            value="light"
                            disabled
                          >
                            <option value="light">ReadRay 浅色</option>
                          </select>
                          <UnavailableButton>导入主题</UnavailableButton>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>

                <div className="rr-settings-group">
                  <GroupHeading title="字体" meta="使用具体字号" />
                  <div className="rr-settings-panel">
                    <div className="rr-settings-row">
                      <SettingsCopy label="界面字体" />
                      <input
                        className="rr-settings-select rr-settings-font-field rr-settings-ui-font"
                        aria-label="界面字体"
                        value="ReadRay Geist"
                        readOnly
                      />
                    </div>
                    <div className="rr-settings-row">
                      <SettingsCopy label="界面字号" />
                      <div className="rr-settings-font-size-control">
                        <label className="rr-settings-number-field">
                          <input aria-label="界面字号" value="14" readOnly />
                          <span>px</span>
                        </label>
                        <button className="rr-settings-restore" type="button" disabled>
                          恢复默认
                        </button>
                      </div>
                    </div>
                    <div className="rr-settings-row">
                      <SettingsCopy label="学习内容字体" />
                      <input
                        className="rr-settings-select rr-settings-font-field rr-settings-learning-font"
                        aria-label="学习内容字体"
                        value="ReadRay Newsreader + 思源宋体"
                        readOnly
                      />
                    </div>
                    <div className="rr-settings-row">
                      <SettingsCopy label="学习内容字号" />
                      <div className="rr-settings-font-size-control">
                        <label className="rr-settings-number-field">
                          <input aria-label="学习内容字号" value="17" readOnly />
                          <span>px</span>
                        </label>
                        <button className="rr-settings-restore" type="button" disabled>
                          恢复默认
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              </section>
            ) : null}

            {activeSection === "ai" ? (
              <section className="rr-settings-section" aria-labelledby="rr-settings-ai-heading">
                <SettingsHeader
                  id="rr-settings-ai-heading"
                  title="AI 服务"
                  meta="当前 provider · DeepSeek"
                />

                <div className="rr-settings-provider-head">
                  <div className="rr-settings-provider-title">
                    <span className="rr-settings-provider-logo">D</span>
                    <div>
                      <strong>DeepSeek</strong>
                      <span>ReadRay 当前正式 AI 服务</span>
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
                      <span>{sourceCopy}</span>
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
                    {operation === "saving"
                      ? "正在验证 DeepSeek 连接，成功后才会替换现有配置…"
                      : operationError ?? operationMessage ??
                        "Key 只在本机安全存储；不会写入前端持久化或普通日志。"}
                  </div>
                </div>

                <div className="rr-settings-row rr-settings-model-row">
                  <SettingsCopy
                    label="模型"
                    help="只显示已完成 ReadRay 兼容验证的 DeepSeek 模型；当前验证模型为默认值。"
                  />
                  <select
                    className="rr-settings-select rr-settings-model-select"
                    aria-label="DeepSeek 模型"
                    value={snapshot.model}
                    disabled
                  >
                    <option value={snapshot.model}>{snapshot.model}</option>
                  </select>
                </div>

                <div className="rr-settings-balance-card">
                  <div>
                    <div className="rr-settings-balance-label">DEEPSEEK 账户余额</div>
                    <div className="rr-settings-balance-value">连接后显示实时余额</div>
                    <div className="rr-settings-balance-meta">余额查询尚未接线</div>
                  </div>
                  <UnavailableButton>刷新余额</UnavailableButton>
                </div>

                <div className="rr-settings-usage-card">
                  <div className="rr-settings-usage-head">
                    <div>
                      <strong>ReadRay 使用量</strong>
                      <span>尚未接入真实 usage 持久化</span>
                    </div>
                    <div className="rr-settings-segmented" aria-label="使用量时间范围">
                      <button type="button" disabled>今天</button>
                      <button className="is-active" type="button" disabled>近 7 天</button>
                      <button type="button" disabled>近 30 天</button>
                      <button type="button" disabled>全部</button>
                    </div>
                  </div>
                  <div className="rr-settings-usage-body">
                    <div className="rr-settings-usage-total-label">READRAY 使用 TOKEN</div>
                    <div className="rr-settings-usage-total">
                      —<span className="rr-settings-usage-unit">Token</span>
                    </div>
                    <div className="rr-settings-usage-meta-grid">
                      <div className="rr-settings-usage-meta-item">
                        <span>AI 请求次数</span><strong>—</strong>
                      </div>
                      <div className="rr-settings-usage-meta-item">
                        <span>统计开始日期</span><strong>—</strong>
                      </div>
                    </div>
                    <div className="rr-settings-usage-breakdown">
                      <div><span>解释查询</span><strong>—</strong></div>
                      <div><span>Quick AI</span><strong>—</strong></div>
                      <div><span>写作</span><strong>—</strong></div>
                    </div>
                    <p className="rr-settings-usage-note">
                      使用量持久化尚未开放；这里不展示推算值，也不统计同一 API Key
                      在其他应用中的使用。
                    </p>
                  </div>
                </div>

                <FutureNote>
                  <strong>更多 AI 服务后续支持。</strong> 当前不展示其他 provider
                  名称，也不提供无效的“添加服务”按钮。
                </FutureNote>
              </section>
            ) : null}

            {activeSection === "data" ? (
              <section className="rr-settings-section" aria-labelledby="rr-settings-data-heading">
                <SettingsHeader
                  id="rr-settings-data-heading"
                  title="本地数据"
                  meta="本地数据优先"
                />

                <div className="rr-settings-group">
                  <GroupHeading title="数据目录" meta="运行时显示完整路径" />
                  <div className="rr-settings-panel">
                    <div className="rr-settings-row">
                      <SettingsCopy
                        label="ReadRay 数据目录"
                        help="包含学习记录、对话、写作和应用设置。"
                      />
                      <div className="rr-settings-stack-control">
                        <div className="rr-settings-path-box" title={snapshot.appDataDirectory}>
                          <FolderIcon />
                          <span>{snapshot.appDataDirectory}</span>
                        </div>
                        <div className="rr-settings-control">
                          <UnavailableButton>打开数据目录</UnavailableButton>
                        </div>
                      </div>
                    </div>
                    <div className="rr-settings-row">
                      <SettingsCopy
                        label="数据概览"
                        help="正式页面从本地数据库读取，不从前端推算。"
                      />
                      <div className="rr-settings-counts">
                        <div><span>学习记录</span><strong>{snapshot.learningRecordCount}</strong></div>
                        <div><span>对话</span><strong>{snapshot.conversationCount}</strong></div>
                        <div><span>写作</span><strong>{snapshot.writingDocumentCount}</strong></div>
                      </div>
                    </div>
                  </div>
                </div>

                <div className="rr-settings-group">
                  <GroupHeading title="备份" meta="包含全部 ReadRay 数据" />
                  <div className="rr-settings-panel">
                    <div className="rr-settings-backup-panel">
                      <div className="rr-settings-backup-head">
                        <div>
                          <strong>备份全部 ReadRay 数据</strong>
                          <p>创建可保存到本地的完整备份；本轮尚未形成可验证闭环。</p>
                        </div>
                        <UnavailableButton className="is-primary">开始备份</UnavailableButton>
                      </div>
                    </div>
                  </div>
                </div>

                <FutureNote>
                  <strong>本阶段不开放：</strong>恢复备份、清空全部数据和全量结构化导出。解释自动保存到记忆是固定能力，不提供关闭开关。
                </FutureNote>

              </section>
            ) : null}

            {activeSection === "about" ? (
              <section className="rr-settings-section" aria-labelledby="rr-settings-about-heading">
                <SettingsHeader
                  id="rr-settings-about-heading"
                  title="关于"
                  meta="版本信息"
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
                <p>以下材料随 ReadRay 字体资源一起打包，可展开查看完整许可证文本。</p>
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
