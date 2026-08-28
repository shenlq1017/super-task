import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { isIpcError, IpcFailure } from "./protocol";
import { mockInvoke } from "./mock";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    const data = isTauri()
      ? await tauriInvoke<T>(command, args)
      : ((await mockInvoke(command, args)) as T);
    return data;
  } catch (e) {
    if (isIpcError(e)) {
      throw new IpcFailure(e);
    }
    throw e;
  }
}
