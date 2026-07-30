import { invoke } from "@tauri-apps/api/core";
import type { QuickAiConversation } from "./types/quickAi";

export interface ConversationRepository {
  create(): Promise<QuickAiConversation>;
  get(conversationId: number): Promise<QuickAiConversation | null>;
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

export class TauriConversationRepository implements ConversationRepository {
  private readonly invokeCommand: ConversationInvoke;

  constructor(invokeCommand: ConversationInvoke = invoke) {
    this.invokeCommand = invokeCommand;
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
