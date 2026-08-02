import type { ConversationExportResult } from "./conversationViewModel";

export function deliverConversationExport(result: ConversationExportResult) {
  if (!result.exported) {
    return result;
  }
  if (!result.fileName.trim() || result.messageCount <= 0) {
    throw new Error("对话服务返回了无效的导出文件。");
  }

  if (result.browserFile) {
    const file = result.browserFile;
    if (
      !file.fileName.trim() ||
      !file.mimeType.trim() ||
      !file.content.trim()
    ) {
      throw new Error("对话服务返回了无效的浏览器导出文件。");
    }
    const blob = new Blob([file.content], { type: file.mimeType });
    if (blob.size === 0) {
      throw new Error("对话服务返回了空的浏览器导出文件。");
    }
    const downloadUrl = URL.createObjectURL(blob);
    const downloadLink = document.createElement("a");
    downloadLink.href = downloadUrl;
    downloadLink.download = file.fileName;
    downloadLink.style.display = "none";
    document.body.appendChild(downloadLink);
    downloadLink.click();
    downloadLink.remove();
    window.setTimeout(() => URL.revokeObjectURL(downloadUrl), 0);
  } else if (!result.nativeFilePath?.trim()) {
    throw new Error("对话服务没有返回导出文件路径。");
  }

  return result;
}
