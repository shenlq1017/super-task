import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Activity, History, ScrollText } from "lucide-react";
import { cn } from "@/lib/utils";
import { useRuntime } from "@/providers/runtime-provider";
import { useWorkspace } from "@/providers/workspace-provider";
import { LogView } from "@/components/log-view";
import { LogHistoryView } from "@/components/log-history";
import { AiExplainButton } from "@/components/ai-explain";
import { StatusDot } from "@/lib/status";
import type { LogSource } from "@/ipc/protocol";

const navCls = (active: boolean) =>
  cn(
    "flex w-full items-center gap-2.5 rounded-[var(--r-sm,8px)] px-2.5 py-1.5 text-[0.83rem] font-medium transition-colors duration-150",
    active
      ? "bg-[var(--surface,#fff)] text-[var(--t1,#222326)] shadow-[0_1px_2px_rgb(16_24_40_/_0.05),inset_0_0_0_1px_var(--line,#e6e6e6)]"
      : "text-[var(--t2,#62666d)] hover:bg-[rgb(0_0_0_/_0.045)] hover:text-[var(--t1,#222326)]",
  );

type LogsMode = "live" | "history";

export function LogsPage() {
  const { state: rt } = useRuntime();
  const { state: ws } = useWorkspace();
  const { t } = useTranslation();
  const ids = Object.keys(rt.services);
  const [sel, setSel] = useState<string | null>(ids[0] ?? null);
  const [mode, setMode] = useState<LogsMode>("live");

  const current = sel && rt.services[sel] ? sel : ids[0] ?? null;
  const source: LogSource | null = current ? { kind: "service", id: current } : null;

  const modeBtn = (m: LogsMode, icon: React.ReactNode, label: string) => (
    <button
      type="button"
      onClick={() => setMode(m)}
      aria-pressed={mode === m}
      className={cn(
        "inline-flex h-6 cursor-pointer items-center gap-1 rounded-[var(--r-sm,8px)] px-2 text-[0.72rem] font-medium transition-colors duration-150",
        mode === m
          ? "bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent-hover,#4f5ac8)] ring-1 ring-[rgb(94_106_210_/_0.35)]"
          : "text-[var(--t2,#62666d)] hover:bg-[rgb(0_0_0_/_0.045)] hover:text-[var(--t1,#222326)]",
      )}
    >
      {icon}
      {label}
    </button>
  );

  return (
    <div className="flex min-h-0 flex-1 overflow-hidden">
      <aside className="w-52 shrink-0 overflow-y-auto border-r border-[var(--line,#e6e6e6)] bg-[var(--bg,#f7f8f8)] p-2">
        <div className="mb-2 flex items-center gap-1 px-1">
          {modeBtn("live", <Activity className="size-3.5" />, t("logs.modeLive"))}
          {modeBtn("history", <History className="size-3.5" />, t("logs.modeHistory"))}
        </div>
        <button onClick={() => setSel(null)} className={navCls(current === null)}>
          <ScrollText className="size-3.5" /> {t("logs.allServices")}
        </button>
        {ids.map((id) => (
          <button key={id} onClick={() => setSel(id)} className={navCls(current === id)}>
            <StatusDot state={rt.services[id].state} size={7} />
            <span className="truncate">{id}</span>
          </button>
        ))}
      </aside>
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        {mode === "history" ? (
          <LogHistoryView source={source} workspaceId={ws.workspaceId} height="100%" />
        ) : (
          <LogView
            source={source}
            height="100%"
            extraActions={({ lines }) => {
              const svc = current != null ? rt.services[current] : undefined;
              return (
                <AiExplainButton
                  lines={lines}
                  source={source}
                  serviceKind={svc?.kind ?? null}
                  servicePort={svc?.port ?? null}
                  serviceState={svc?.state ?? null}
                />
              );
            }}
          />
        )}
      </div>
    </div>
  );
}
