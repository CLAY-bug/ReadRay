import MainAppIcon from "./MainAppIcon";

type ApiKeySetupCardProps = {
  onOpenSettings: () => void;
  onDismiss: () => void;
};

function ApiKeySetupCard({
  onOpenSettings,
  onDismiss,
}: ApiKeySetupCardProps) {
  return (
    <aside
      className="rr-main-api-key-setup-card"
      aria-labelledby="rr-main-api-key-setup-title"
      aria-live="polite"
    >
      <div className="rr-main-api-key-setup-head">
        <span className="rr-main-api-key-setup-logo" aria-hidden="true">
          D
        </span>
        <div className="rr-main-api-key-setup-copy">
          <strong id="rr-main-api-key-setup-title">先配置 AI 服务</strong>
          <p>填写并验证 DeepSeek API Key，才能使用解释、对话和写作辅助。</p>
        </div>
        <button
          className="rr-main-api-key-setup-close"
          type="button"
          aria-label="稍后再配置"
          title="稍后再配置"
          onClick={onDismiss}
        >
          ×
        </button>
      </div>
      <div className="rr-main-api-key-setup-actions">
        <button
          className="rr-main-api-key-setup-primary"
          type="button"
          onClick={onOpenSettings}
        >
          <span>配置 API Key</span>
          <MainAppIcon name="arrow" />
        </button>
        <button
          className="rr-main-api-key-setup-secondary"
          type="button"
          onClick={onDismiss}
        >
          稍后再说
        </button>
      </div>
    </aside>
  );
}

export default ApiKeySetupCard;
