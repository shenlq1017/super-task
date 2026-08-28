import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useLogs, filterLogs } from "@/providers/logs-provider";
import type { LogSource } from "@/ipc/protocol";
import { fmtTime } from "@/lib/status";
import { Pause, Play, Trash2 } from "lucide-react";

export function LogView({
  source,
  className,
  height = "100%",
}: {
  source: LogSource | null;
  className?: string;
  height?: string;
}) {
  const { state, actions } = useLogs();
  const [follow, setFollow] = useState(true);
  const scrollRef = useRef<HTMLDivElement>(null);
  const lines = filterLogs(state.all, source);

  useLayoutEffect(() => {
    if (follow && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [lines.length, follow]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const onScroll = () => {
      const bottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
      setFollow(bottom);
    };
    el.addEventListener("scroll", onScroll);
    return () => el.removeEventListener("scroll", onScroll);
  }, []);

  return (
    <div className={cn("flex flex-col", className)} style={{ height }}>
      <div className="flex flex-wrap items-center gap-2 border-b border-[var(--line,#e6e6e6)] px-4 py-2.5">
        <span className="font-mono text-[0.71rem] text-[var(--t2,#62666d)]">
          {source ? `${source.kind}:${source.id}` : "全部"}
        </span>
        <span className="text-[0.64rem] text-[var(--t3,#8a8f98)]">{lines.length} 行</span>
        <div className="ml-auto flex items-center gap-2">
          <button
            type="button"
            onClick={() => setFollow((v) => !v)}
            title={follow ? "暂停跟随" : "跟随底部"}
            className={cn(
              "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[0.71rem] font-medium transition-all duration-150",
              follow
                ? "border-[rgb(94_106_210_/_0.3)] bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent-hover,#4f5ac8)]"
                : "border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] text-[var(--t2,#62666d)] hover:border-[var(--line-strong,#d0d6e0)]",
            )}
          >
            {follow ? <Pause className="size-3" /> : <Play className="size-3" />}
            {follow ? "跟随" : "暂停"}
          </button>
          <Button
            variant="outline"
            size="sm"
            disabled={!source}
            onClick={() => source && actions.clear(source)}
            title="清除本视图"
            className="h-auto rounded-full px-3 py-1 text-[0.72rem]"
          >
            <Trash2 className="size-3" /> 清除
          </Button>
        </div>
      </div>
      <div
        ref={scrollRef}
        className="min-h-0 flex-1 overflow-auto bg-[#16181D] px-4 py-3 font-mono text-[0.71rem] leading-[1.85] text-[#C9CFD6]"
      >
        {lines.length === 0 ? (
          <div className="p-4 text-center text-[var(--t3,#8a8f98)]">暂无日志</div>
        ) : (
          lines.map((l) => (
            <div key={l.seq} className="flex gap-2 whitespace-pre-wrap break-all">
              <span className="shrink-0 select-none text-[#5C6470]">{fmtTime(l.ts_ms)}</span>
              <span
                className={cn(
                  "shrink-0 select-none uppercase",
                  l.stream === "stderr" ? "text-[#F0908C]" : l.stream === "system" ? "text-[#F0C36A]" : "text-[#8FC7E8]",
                )}
              >
                {l.stream}
              </span>
              <span>{l.text}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
