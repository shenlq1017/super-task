import { useState } from "react";
import { ScrollText } from "lucide-react";
import { cn } from "@/lib/utils";
import { useRuntime } from "@/providers/runtime-provider";
import { LogView } from "@/components/log-view";
import { StatusDot } from "@/lib/status";
import type { LogSource } from "@/ipc/protocol";

const navCls = (active: boolean) =>
  cn(
    "flex w-full items-center gap-2.5 rounded-[var(--r-sm,8px)] px-2.5 py-1.5 text-[0.83rem] font-medium transition-colors duration-150",
    active
      ? "bg-[var(--surface,#fff)] text-[var(--t1,#222326)] shadow-[0_1px_2px_rgb(16_24_40_/_0.05),inset_0_0_0_1px_var(--line,#e6e6e6)]"
      : "text-[var(--t2,#62666d)] hover:bg-[rgb(0_0_0_/_0.045)] hover:text-[var(--t1,#222326)]",
  );

export function LogsPage() {
  const { state: rt } = useRuntime();
  const ids = Object.keys(rt.services);
  const [sel, setSel] = useState<string | null>(ids[0] ?? null);

  const current = sel && rt.services[sel] ? sel : ids[0] ?? null;
  const source: LogSource | null = current ? { kind: "service", id: current } : null;

  return (
    <div className="flex min-h-0 flex-1 overflow-hidden">
      <aside className="w-52 shrink-0 overflow-y-auto border-r border-[var(--line,#e6e6e6)] bg-[var(--bg,#f7f8f8)] p-2">
        <button onClick={() => setSel(null)} className={navCls(current === null)}>
          <ScrollText className="size-3.5" /> 全部服务
        </button>
        {ids.map((id) => (
          <button key={id} onClick={() => setSel(id)} className={navCls(current === id)}>
            <StatusDot state={rt.services[id].state} size={7} />
            <span className="truncate">{id}</span>
          </button>
        ))}
      </aside>
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        <LogView source={source} height="100%" />
      </div>
    </div>
  );
}
