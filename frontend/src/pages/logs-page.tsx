import { useEffect, useMemo, useState } from "react";
import { Download, Loader2, Search, ScrollText } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { useRuntime } from "@/providers/runtime-provider";
import { useWorkspace } from "@/providers/workspace-provider";
import { useOperations } from "@/providers/operation-provider";
import { useToast } from "@/components/ui/toast";
import { LogView } from "@/components/log-view";
import { StatusDot, opErrorLabel } from "@/lib/status";
import { apiLogsExport, apiLogsSearch } from "@/ipc/api";
import { IpcFailure, type LogSearchHit, type LogSource } from "@/ipc/protocol";

const navCls = (active: boolean) =>
  cn(
    "flex w-full items-center gap-2.5 rounded-[var(--r-sm,8px)] px-2.5 py-1.5 text-[0.83rem] font-medium transition-colors duration-150",
    active
      ? "bg-[var(--surface,#fff)] text-[var(--t1,#222326)] shadow-[0_1px_2px_rgb(16_24_40_/_0.05),inset_0_0_0_1px_var(--line,#e6e6e6)]"
      : "text-[var(--t2,#62666d)] hover:bg-[rgb(0_0_0_/_0.045)] hover:text-[var(--t1,#222326)]",
  );

export function LogsPage() {
  const { state: rt } = useRuntime();
  const ws = useWorkspace();
  const operations = useOperations();
  const { toast } = useToast();
  const ids = Object.keys(rt.services);
  const [sel, setSel] = useState<string | null>(ids[0] ?? null);
  const [query, setQuery] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [limit, setLimit] = useState("200");
  const [destination, setDestination] = useState("");
  const [searchOp, setSearchOp] = useState<string | null>(null);
  const [exportOp, setExportOp] = useState<string | null>(null);
  const [history, setHistory] = useState<LogSearchHit[]>([]);
  const [truncated, setTruncated] = useState(false);

  const current = sel && rt.services[sel] ? sel : ids[0] ?? null;
  const source: LogSource | null = current ? { kind: "service", id: current } : null;
  const workspaceId = ws.state.workspaceId;
  const searchState = searchOp ? operations.get(searchOp) : null;
  const exportState = exportOp ? operations.get(exportOp) : null;
  const searching = searchState?.state === "queued" || searchState?.state === "running";
  const exporting = exportState?.state === "queued" || exportState?.state === "running";

  useEffect(() => {
    if (searchState?.state === "succeeded") {
      const result = (searchState.result ?? {}) as { items?: LogSearchHit[]; truncated?: boolean };
      setHistory(result.items ?? []);
      setTruncated(result.truncated === true);
      toast(`搜索完成：${result.items?.length ?? 0} 条结果`, "ok");
      setSearchOp(null);
    } else if (searchState?.state === "failed") {
      toast(`${opErrorLabel(searchState.error_code)}${searchState.message ? `（${searchState.message}）` : ""}`, "err");
      setSearchOp(null);
    }
  }, [searchState, toast]);

  useEffect(() => {
    if (exportState?.state === "succeeded") {
      toast("日志导出完成", "ok");
      setExportOp(null);
    } else if (exportState?.state === "failed") {
      toast(`${opErrorLabel(exportState.error_code)}${exportState.message ? `（${exportState.message}）` : ""}`, "err");
      setExportOp(null);
    }
  }, [exportState, toast]);

  const search = async () => {
    if (!workspaceId || !query.trim() || searching) return;
    try {
      const out = await apiLogsSearch(workspaceId, query, {
        source,
        caseSensitive,
        limit: Math.max(1, Math.min(5000, Number(limit) || 200)),
      });
      setSearchOp(out.operation_id);
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  const exportHistory = async () => {
    if (!workspaceId || !destination.trim() || exporting) return;
    try {
      const out = await apiLogsExport(workspaceId, "text", destination.trim(), {
        source,
        query: query.trim() || null,
        caseSensitive,
      });
      setExportOp(out.operation_id);
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  const sourceLabel = useMemo(() => (current ? `service:${current}` : "全部服务"), [current]);

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="border-b border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] px-4 py-3">
        <div className="flex flex-wrap items-center gap-2">
          <div className="flex items-center gap-2 text-[0.85rem] font-semibold text-[var(--t1,#222326)]">
            <Search className="size-4 text-[var(--st-accent,#5e6ad2)]" /> 历史搜索
          </div>
          <Badge variant="secondary">literal</Badge>
          <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">当前范围：{sourceLabel}</span>
          <div className="ml-auto flex items-center gap-2">
            <label className="flex items-center gap-1 text-[0.72rem] text-[var(--t2,#62666d)]">
              <input type="checkbox" checked={caseSensitive} onChange={(e) => setCaseSensitive(e.target.checked)} /> 区分大小写
            </label>
            <Input className="h-8 w-20 font-mono text-xs" value={limit} onChange={(e) => setLimit(e.target.value)} aria-label="结果上限" />
          </div>
        </div>
        <div className="mt-2 flex flex-wrap gap-2">
          <Input
            className="h-9 min-w-[14rem] flex-1 font-mono text-xs"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") void search(); }}
            placeholder="搜索日志文本（不支持正则）"
            aria-label="日志搜索"
          />
          <Button size="sm" onClick={() => void search()} disabled={!workspaceId || !query.trim() || searching}>
            {searching ? <Loader2 className="size-3.5 animate-spin" /> : <Search className="size-3.5" />}
            {searching ? "搜索中…" : "搜索"}
          </Button>
          <Input
            className="h-9 min-w-[14rem] flex-1 font-mono text-xs"
            value={destination}
            onChange={(e) => setDestination(e.target.value)}
            placeholder="导出目标路径（不覆盖已有文件）"
            aria-label="导出目标路径"
          />
          <Button variant="outline" size="sm" onClick={() => void exportHistory()} disabled={!workspaceId || !destination.trim() || exporting}>
            {exporting ? <Loader2 className="size-3.5 animate-spin" /> : <Download className="size-3.5" />}
            {exporting ? "导出中…" : "导出文本"}
          </Button>
        </div>
      </div>

      {history.length > 0 || truncated ? (
        <section className="max-h-[15rem] overflow-auto border-b border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] px-4 py-3">
          <div className="mb-2 flex items-center gap-2 text-[0.72rem] font-semibold text-[var(--t2,#62666d)]">
            搜索结果 <Badge variant="secondary">{history.length}{truncated ? "+" : ""}</Badge>
            {truncated ? <span className="font-normal text-[#B7791F]">已达到结果上限</span> : null}
          </div>
          <div className="flex flex-col gap-1">
            {history.map((hit, index) => (
              <div key={`${hit.file}:${hit.line_no}:${index}`} className="grid grid-cols-[8rem_3rem_1fr] gap-2 rounded bg-[var(--surface,#fff)] px-2 py-1 font-mono text-[0.7rem]">
                <span className="truncate text-[var(--t3,#8a8f98)]" title={hit.file}>{hit.file}</span>
                <span className="text-[var(--t3,#8a8f98)]">L{hit.line_no}</span>
                <span className="whitespace-pre-wrap break-all text-[var(--t1,#222326)]">{hit.text}</span>
              </div>
            ))}
          </div>
        </section>
      ) : null}

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
          {current === null ? <LogView source={null} height="100%" /> : <LogView source={source} height="100%" />}
        </div>
      </div>
    </div>
  );
}
