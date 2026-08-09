import { Channel, invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import type {
  QuickAiConversation,
  QuickAiConversationExport,
  RecentQuickAiConversation,
  ConversationOrigin,
} from "./types/quickAi";

export type QuickAiStreamEvent =
  | { type: "delta"; text: string }
  | { type: "done" }
  | { type: "stopped" }
  | { type: "truncated" }
  | { type: "error"; message: string };

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
  sendStreaming(
    conversationId: number,
    expectedUserSequence: number,
    content: string,
    onEvent: (event: QuickAiStreamEvent) => void,
  ): Promise<QuickAiConversation>;
  abortStreaming(conversationId: number): Promise<void>;
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

  private readonly creationOrigin: Exclude<ConversationOrigin, "legacy">;

  constructor(
    invokeCommand: ConversationInvoke = invoke,
    saveDialog: ConversationSaveDialog = save,
    creationOrigin: Exclude<ConversationOrigin, "legacy"> = "main",
  ) {
    this.invokeCommand = invokeCommand;
    this.saveDialog = saveDialog;
    this.creationOrigin = creationOrigin;
  }

  create() {
    return this.invokeCommand<QuickAiConversation>(
      "create_quick_ai_conversation",
      { origin: this.creationOrigin },
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

  async sendStreaming(
    conversationId: number,
    expectedUserSequence: number,
    content: string,
    onEvent: (event: QuickAiStreamEvent) => void,
  ) {
    const channel = new Channel<QuickAiStreamEvent>(onEvent);
    return this.invokeCommand<QuickAiConversation>(
      "send_quick_ai_message_streaming",
      {
        conversationId,
        expectedUserSequence,
        content,
        channel,
      },
    );
  }

  abortStreaming(conversationId: number) {
    return this.invokeCommand<void>("abort_quick_ai_streaming", {
      conversationId,
    });
  }
}
