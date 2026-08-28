import { isTauri } from "@/ipc/invoke";
import { invoke } from "@/ipc/invoke";

function browserDownload(filename: string, text: string): boolean {
  try {
    const blob = new Blob([text], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    a.style.display = "none";
    document.body.appendChild(a);
    a.click();
    window.setTimeout(() => {
      a.remove();
      URL.revokeObjectURL(url);
    }, 1000);
    return true;
  } catch {
    try {
      const a = document.createElement("a");
      a.href = `data:text/plain;charset=utf-8,${encodeURIComponent(text)}`;
      a.download = filename;
      a.style.display = "none";
      document.body.appendChild(a);
      a.click();
      window.setTimeout(() => a.remove(), 1000);
      return true;
    } catch {
      return false;
    }
  }
}

/**
 * 保存文本为本地文件。
 * - Tauri：系统「另存为」对话框 + `app.writeTextFile`
 * - 浏览器 mock：Blob 触发下载
 */
export async function downloadTextFile(filename: string, text: string): Promise<"saved" | "cancelled" | "failed"> {
  if (!text) return "failed";

  if (isTauri()) {
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const path = await save({
        defaultPath: filename,
        filters: [{ name: "日志", extensions: ["log", "txt"] }],
      });
      if (!path) return "cancelled";
      await invoke<{ ok: boolean }>("app.writeTextFile", { path, contents: text });
      return "saved";
    } catch {
      return "failed";
    }
  }

  return browserDownload(filename, text) ? "saved" : "failed";
}
