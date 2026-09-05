import { useSyncExternalStore } from "react";
import { TEMP_MODES, type TempMode } from "../ipc/protocol";

/**
 * The temperature sampling preference is shared by every `system.metrics`
 * caller (status bar and monitor page): a caller polling with a different mode
 * than the status bar would each second tear down / respawn the Windows
 * resident fast sampler. Keep one storage key, one loader and one reactive
 * store so all pollers always agree on the mode.
 */
const TEMP_MODE_KEY = "supertask.statusBar.tempMode";

export function loadTempMode(): TempMode {
  try {
    const raw = window.localStorage.getItem(TEMP_MODE_KEY);
    if (raw && (TEMP_MODES as readonly string[]).includes(raw)) return raw as TempMode;
  } catch {
    // Private mode / disabled storage: fall back to the default.
  }
  return "auto";
}

export function saveTempMode(mode: TempMode): void {
  try {
    window.localStorage.setItem(TEMP_MODE_KEY, mode);
  } catch {
    // Preference is cosmetic; losing persistence is not worth surfacing.
  }
}

let current = loadTempMode();
const listeners = new Set<() => void>();

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function pickTempMode(mode: TempMode): void {
  current = mode;
  saveTempMode(mode);
  listeners.forEach((l) => l());
}

/** The shared temp-mode preference; updates in every mounted consumer. */
export function useTempMode(): TempMode {
  return useSyncExternalStore(subscribe, () => current);
}
