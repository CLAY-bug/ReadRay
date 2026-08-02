import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type CSSProperties,
} from "react";
import { deliverConversationExport } from "../conversationExportDelivery";
import type {
  ConversationOperationIdentity,
  ConversationService,
} from "../conversationViewModel";
import {
  conversationExportUnavailableReason,
  isConversationOperationCurrent,
} from "../conversationViewModel";

export type ConversationManagementTarget = {
  interactionKey: number;
  conversationId: string;
  title: string;
  x: number;
  y: number;
  routeIdentity: ConversationOperationIdentity;
};

type ManagementDialog =
  | {
      kind: "rename";
      target: ConversationManagementTarget;
      draft: string;
    }
  | {
      kind: "delete";
      target: ConversationManagementTarget;
    };

type PendingOperation = {
  kind: "rename" | "delete" | "export";
  identity: ConversationOperationIdentity;
};

type ConversationManagementMenuProps = {
  service: ConversationService;
  target: ConversationManagementTarget | null;
  pageIdentity: string;
  onCloseMenu: () => void;
  onRenamed: (
    conversationId: string,
    title: string,
    routeIdentity: ConversationOperationIdentity,
  ) => void;
  onDeleted: (routeIdentity: ConversationOperationIdentity) => void;
};

function toOperationIdentity(target: ConversationManagementTarget) {
  return {
    requestKey: target.interactionKey,
    conversationId: target.conversationId,
  };
}

function ConversationManagementMenu({
  service,
  target,
  pageIdentity,
  onCloseMenu,
  onRenamed,
  onDeleted,
}: ConversationManagementMenuProps) {
  const [dialog, setDialog] = useState<ManagementDialog | null>(null);
  const [pending, setPending] = useState<PendingOperation | null>(null);
  const [operationError, setOperationError] = useState("");
  const [toast, setToast] = useState("");
  const mountedRef = useRef(true);
  const activeIdentityRef = useRef<ConversationOperationIdentity | null>(null);
  const pageIdentityRef = useRef(pageIdentity);
  const toastTimerRef = useRef<number | undefined>(undefined);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (toastTimerRef.current !== undefined) {
        window.clearTimeout(toastTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (pageIdentityRef.current !== pageIdentity) {
      pageIdentityRef.current = pageIdentity;
      activeIdentityRef.current = null;
      setDialog(null);
      setOperationError("");
    }
  }, [pageIdentity]);

  useEffect(() => {
    if (target) {
      activeIdentityRef.current = toOperationIdentity(target);
      setOperationError("");
    }
  }, [target]);

  useEffect(() => {
    const handlePointerDown = (event: PointerEvent) => {
      if (
        target &&
        !(event.target as HTMLElement).closest(".rr-conversation-context-menu")
      ) {
        onCloseMenu();
      }
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") {
        return;
      }
      if (dialog && !pending) {
        activeIdentityRef.current = null;
        setDialog(null);
        setOperationError("");
      }
      onCloseMenu();
    };
    window.addEventListener("pointerdown", handlePointerDown);
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("pointerdown", handlePointerDown);
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [dialog, onCloseMenu, pending, target]);

  const operationIsCurrent = (identity: ConversationOperationIdentity) => {
    const active = activeIdentityRef.current;
    return isConversationOperationCurrent(
      mountedRef.current,
      identity,
      active?.requestKey ?? -1,
      active?.conversationId,
    );
  };

  const notify = (message: string) => {
    if (toastTimerRef.current !== undefined) {
      window.clearTimeout(toastTimerRef.current);
    }
    setToast(message);
    toastTimerRef.current = window.setTimeout(() => setToast(""), 2300);
  };

  const closeDialog = () => {
    if (pending) {
      return;
    }
    activeIdentityRef.current = null;
    setDialog(null);
    setOperationError("");
  };

  const beginRename = () => {
    if (!target) {
      return;
    }
    activeIdentityRef.current = toOperationIdentity(target);
    setDialog({ kind: "rename", target, draft: target.title });
    setOperationError("");
    onCloseMenu();
  };

  const beginDelete = () => {
    if (!target) {
      return;
    }
    activeIdentityRef.current = toOperationIdentity(target);
    setDialog({ kind: "delete", target });
    setOperationError("");
    onCloseMenu();
  };

  const renameConversation = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (dialog?.kind !== "rename" || pending) {
      return;
    }
    const title = dialog.draft.trim();
    if (!title) {
      return;
    }
    const identity = toOperationIdentity(dialog.target);
    setPending({ kind: "rename", identity });
    setOperationError("");
    try {
      const renamed = await service.renameConversation(
        dialog.target.conversationId,
        title,
      );
      if (!operationIsCurrent(identity)) {
        return;
      }
      setDialog(null);
      onRenamed(
        renamed.id,
        renamed.title,
        dialog.target.routeIdentity,
      );
      notify("会话名称已更新");
    } catch (error) {
      if (operationIsCurrent(identity)) {
        setOperationError(error instanceof Error ? error.message : String(error));
      }
    } finally {
      if (operationIsCurrent(identity)) {
        setPending(null);
      }
    }
  };

  const deleteConversation = async () => {
    if (dialog?.kind !== "delete" || pending) {
      return;
    }
    const identity = toOperationIdentity(dialog.target);
    setPending({ kind: "delete", identity });
    setOperationError("");
    try {
      await service.deleteConversation(dialog.target.conversationId);
      if (!operationIsCurrent(identity)) {
        return;
      }
      setDialog(null);
      onDeleted(dialog.target.routeIdentity);
    } catch (error) {
      if (operationIsCurrent(identity)) {
        setOperationError(error instanceof Error ? error.message : String(error));
      }
    } finally {
      if (operationIsCurrent(identity)) {
        setPending(null);
      }
    }
  };

  const exportConversation = async () => {
    if (!target || pending) {
      return;
    }
    const exportTarget = target;
    const identity = toOperationIdentity(exportTarget);
    activeIdentityRef.current = identity;
    setPending({ kind: "export", identity });
    onCloseMenu();
    try {
      const thread = await service.loadConversation(
        exportTarget.conversationId,
        exportTarget.title,
      );
      if (!operationIsCurrent(identity)) {
        return;
      }
      const unavailableReason = conversationExportUnavailableReason(
        thread,
        false,
        service.capabilities.canExport,
      );
      if (unavailableReason) {
        notify(unavailableReason);
        return;
      }
      const result = await service.exportConversation(thread);
      if (!operationIsCurrent(identity) || !result.exported) {
        return;
      }
      deliverConversationExport(result);
      notify(`已导出 ${result.fileName}`);
    } catch (error) {
      if (operationIsCurrent(identity)) {
        console.error("ReadRay 对话导出失败：", error);
        notify("导出失败，请稍后重试");
      }
    } finally {
      if (operationIsCurrent(identity)) {
        setPending(null);
      }
    }
  };

  const menuStyle = target
    ? ({ left: target.x, top: target.y } satisfies CSSProperties)
    : undefined;

  return (
    <>
      {target ? (
        <div
          className="rr-conversation-more-menu rr-conversation-context-menu"
          role="menu"
          style={menuStyle}
          onContextMenu={(event) => event.preventDefault()}
        >
          <button
            className="rr-conversation-menu-item"
            type="button"
            role="menuitem"
            disabled={pending !== null}
            onClick={beginRename}
          >
            重命名
          </button>
          <button
            className="rr-conversation-menu-item"
            type="button"
            role="menuitem"
            disabled={pending !== null}
            onClick={() => void exportConversation()}
          >
            导出
          </button>
          <button
            className="rr-conversation-menu-item is-danger"
            type="button"
            role="menuitem"
            disabled={pending !== null}
            onClick={beginDelete}
          >
            删除
          </button>
        </div>
      ) : null}

      {dialog?.kind === "rename" ? (
        <div className="rr-conversation-dialog-backdrop">
          <form
            className="rr-conversation-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="rr-list-rename-title"
            onSubmit={renameConversation}
          >
            <h2 id="rr-list-rename-title">重命名会话</h2>
            <input
              autoFocus
              maxLength={80}
              value={dialog.draft}
              aria-label="会话名称"
              onChange={(event) =>
                setDialog({ ...dialog, draft: event.target.value })
              }
            />
            {operationError ? (
              <p className="rr-conversation-dialog-error">{operationError}</p>
            ) : null}
            <div className="rr-conversation-dialog-actions">
              <button
                type="button"
                disabled={pending !== null}
                onClick={closeDialog}
              >
                取消
              </button>
              <button
                className="is-primary"
                type="submit"
                disabled={!dialog.draft.trim() || pending !== null}
              >
                {pending?.kind === "rename" ? "正在保存…" : "保存"}
              </button>
            </div>
          </form>
        </div>
      ) : null}

      {dialog?.kind === "delete" ? (
        <div className="rr-conversation-dialog-backdrop">
          <section
            className="rr-conversation-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="rr-list-delete-title"
          >
            <h2 id="rr-list-delete-title">删除“{dialog.target.title}”？</h2>
            <p>完整消息将从本机 SQLite 中删除，此操作无法撤销。</p>
            {operationError ? (
              <p className="rr-conversation-dialog-error">{operationError}</p>
            ) : null}
            <div className="rr-conversation-dialog-actions">
              <button
                type="button"
                disabled={pending !== null}
                onClick={closeDialog}
              >
                取消
              </button>
              <button
                className="is-danger"
                type="button"
                disabled={pending !== null}
                onClick={() => void deleteConversation()}
              >
                {pending?.kind === "delete" ? "正在删除…" : "删除"}
              </button>
            </div>
          </section>
        </div>
      ) : null}

      <div
        className={`rr-conversation-toast${toast ? " is-visible" : ""}`}
        role="status"
      >
        {toast}
      </div>
    </>
  );
}

export default ConversationManagementMenu;
