import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useRuntime } from "@/providers/runtime-provider";
import { useToast } from "@/components/ui/toast";
import { isTauri } from "@/ipc/invoke";

/**
 * 1.7 §8.2 崩溃通知（零 core 改动）：
 * - `→exited` 且 last_exit.code ≠ 0，或 `→unhealthy` → 触发；
 * - 窗口聚焦时 Toast，失焦（含托盘）时系统通知；
 * - 同一服务 10s 去重；主动停止（prev=stopping/stopped）不触发；
 * - 设置页「通用」开关 `st:crashNotify`（默认开）。
 */
const DEDUP_MS = 10_000;

export function readCrashNotifyPref(): boolean {
  return localStorage.getItem("st:crashNotify") !== "off";
}

export function CrashNotifier() {
  const { state } = useRuntime();
  const services = state.services;
  const { toast } = useToast();
  const { t } = useTranslation();
  const prev = useRef<Record<string, string>>({});
  const lastNotified = useRef<Record<string, number>>({});

  useEffect(() => {
    if (!readCrashNotifyPref()) {
      prev.current = {};
      return;
    }
    const now = Date.now();
    for (const [id, cur] of Object.entries(services)) {
      const before = prev.current[id];
      prev.current[id] = cur.state;
      if (!before || before === cur.state) continue;
      if (before === "stopping" || before === "stopped") continue;
      const crashed =
        (cur.state === "exited" && cur.last_exit != null && cur.last_exit.code !== 0) ||
        cur.state === "unhealthy";
      if (!crashed) continue;
      if (now - (lastNotified.current[id] ?? 0) < DEDUP_MS) continue;
      lastNotified.current[id] = now;
      const title = t("notify.crashTitle", { id });
      const body = t("notify.crashBody", { state: cur.state, code: cur.last_exit?.code ?? "-" });
      if (document.hasFocus()) {
        toast(`${title}：${body}`, "err");
      } else if (isTauri()) {
        void (async () => {
          try {
            const mod = await import("@tauri-apps/plugin-notification");
            let granted = await mod.isPermissionGranted();
            if (!granted) {
              granted = (await mod.requestPermission()) === "granted";
            }
            if (granted) mod.sendNotification({ title, body });
          } catch {
            // 通知不可用（权限拒绝等）静默降级
          }
        })();
      }
    }
  }, [services, toast, t]);

  return null;
}
