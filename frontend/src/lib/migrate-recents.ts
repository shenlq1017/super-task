import { isTauri } from "../ipc/invoke";
import { apiImportRecents } from "../ipc/api";
import type { AppLoadOut } from "../ipc/protocol";
import { clearLastWorkspace, readLastWorkspace, readRecents, writeRecents } from "./workspace-storage";

/**
 * localStorage 最近工作区一次性迁移（ipc.md §10.5）。
 *
 * 触发条件（全部满足才调用 `app.importRecents`）：
 * 1. 真桌面壳（isTauri）；
 * 2. app data 为空 —— `app.load` 返回的 recents 与 stale 都为空；
 * 3. localStorage 里 `st:recents` 或 `st:lastWorkspace` 还有旧数据。
 *
 * 成功后清掉旧 key 防止重复迁移；失败静默（console.warn），下次启动重试。
 */
export function migrateLocalRecents(loadOut: AppLoadOut): Promise<void> {
  // module 级 promise 去重：StrictMode / 重复挂载只跑一次
  if (!inflight) {
    inflight = run(loadOut).finally(() => {
      inflight = null;
    });
  }
  return inflight;
}

let inflight: Promise<void> | null = null;

async function run(loadOut: AppLoadOut): Promise<void> {
  if (!isTauri()) return;
  if (loadOut.recents.length > 0 || loadOut.stale.length > 0) return;

  const recents = readRecents();
  const last = readLastWorkspace();
  if (recents.length === 0 && !last) return;

  try {
    await apiImportRecents(recents, last);
    clearLastWorkspace();
    writeRecents([]);
  } catch (e) {
    console.warn("[supertask] localStorage 最近工作区迁移失败，将在下次启动重试:", e);
  }
}
