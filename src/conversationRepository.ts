import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import type {
  QuickAiConversation,
  QuickAiConversationExport,
  RecentQuickAiConversation,
} from "./types/quickAi";

export interface ConversationRepository {
  create(): Promise<QuickAiConversation>;
  get(conversationId: number): Promise<QuickAiConversation | null>;
  list(): Promise<RecentQuickAiConversation[]>;
  rename(conversationId: number, title: string): Promise<QuickAiConversation>;
  delete(conversationId: number): Promise<boolean>;
  export(
    conversationId: number,
    suggestedFileName: string,
  ): Promise<QuickAiConversationExport | null>;
  send(
    conversationId: number,
    expectedUserSequence: number,
    content: string,
  ): Promise<QuickAiConversation>;
}

export type ConversationInvoke = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

export type ConversationSaveDialog = (options: {
  title: string;
  defaultPath: string;
  filters: { name: string; extensions: string[] }[];
}) => Promise<string | null>;

export class TauriConversationRepository implements ConversationRepository {
  private readonly invokeCommand: ConversationInvoke;

  private readonly saveDialog: ConversationSaveDialog;

  constructor(
    invokeCommand: ConversationInvoke = invoke,
    saveDialog: ConversationSaveDialog = save,
  ) {
    this.invokeCommand = invokeCommand;
    this.saveDialog = saveDialog;
  }

  create() {
    return this.invokeCommand<QuickAiConversation>(
      "create_quick_ai_conversation",
    );
  }

  get(conversationId: number) {
    return this.invokeCommand<QuickAiConversation | null>(
      "get_quick_ai_conversation",
      { conversationId },
    );
  }

  list() {
    return this.invokeCommand<RecentQuickAiConversation[]>(
      "list_all_quick_ai_conversations",
    );
  }

  rename(conversationId: number, title: string) {
    return this.invokeCommand<QuickAiConversation>(
      "rename_quick_ai_conversation",
      { conversationId, title },
    );
  }

  delete(conversationId: number) {
    return this.invokeCommand<boolean>("delete_quick_ai_conversation", {
      conversationId,
    });
  }

  async export(conversationId: number, suggestedFileName: string) {
    const filePath = await this.saveDialog({
      title: "导出 ReadRay 对话",
      defaultPath: suggestedFileName,
      filters: [{ name: "Markdown", extensions: ["md"] }],
    });
    if (!filePath) {
      return null;
    }
    return this.invokeCommand<QuickAiConversationExport>(
      "export_quick_ai_conversation",
      { conversationId, filePath },
    );
  }

  send(
    conversationId: number,
    expectedUserSequence: number,
    content: string,
  ) {
    return this.invokeCommand<QuickAiConversation>("send_quick_ai_message", {
      conversationId,
      expectedUserSequence,
      content,
    });
  }
}
