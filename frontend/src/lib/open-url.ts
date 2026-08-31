import { isTauri } from "@/ipc/invoke";

/**
 * 用系统默认浏览器打开 URL。
 * - Tauri：`@tauri-apps/plugin-opener`（capability 已含 `opener:default`，允许 http/https）
 * - 浏览器 mock：`window.open`
 */
export async function openUrlInBrowser(url: string): Promise<boolean> {
  try {
    if (isTauri()) {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(url);
    } else {
      window.open(url, "_blank", "noopener");
    }
    return true;
  } catch {
    return false;
  }
}

/** 本地服务端口 → 浏览器地址 */
export function localPortUrl(port: number): string {
  return `http://localhost:${port}/`;
}
