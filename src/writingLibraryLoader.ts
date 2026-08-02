import type { WritingService } from "./writingService";
import type { WritingDocumentSummary } from "./writingViewModel";

export type WritingLibraryLoadState =
  | {
      status: "loading";
      records: WritingDocumentSummary[];
    }
  | {
      status: "ready";
      records: WritingDocumentSummary[];
    }
  | {
      status: "error";
      records: WritingDocumentSummary[];
      error: string;
    };

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export async function loadWritingLibrary(
  service: Pick<WritingService, "listDocuments">,
  query: string,
  onState: (state: WritingLibraryLoadState) => void,
) {
  onState({ status: "loading", records: [] });
  try {
    const records = await service.listDocuments(query);
    onState({ status: "ready", records });
    return records;
  } catch (error) {
    onState({
      status: "error",
      records: [],
      error: errorMessage(error),
    });
    throw error;
  }
}
