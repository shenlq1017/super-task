import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Copy, Download, Loader2, Search } from "lucide-react";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { useOperations } from "@/providers/operation-provider";
import { apiLogsExport, apiLogsSearch } from "@/ipc/api";
import { isTauri } from "@/ipc/invoke";
import { IpcFailure, type LogsSearchResult, type LogSource } from "@/ipc/protocol";
import { errorDisplayText } from "@/lib/error-messages";
import { HighlightMatches } from "@/components/log-line";
import { copyText } from "@/lib/copy-text";
import { useToast } from "@/components/ui/toast";

const LIMIT_OPTIONS = [200, 500, 1000, 2000, 5000] as const;

/** 历史命中行的来源徽章配色（与实时视图 streamStyle 同系暗色面板）。 */
const kindCls: Record<string, string> = {
  service: "text-[#8B93FF]",
  script: "text-[#E5C07B]",
  system: "text-[#98C379]",
  gateway: "text-[#56B6C2]",
};

function HitBtn({
  active,
  icon,
  title,
  disabled,
  busy,
  onClick,
}: {
  active?: boolean;
  icon: React.ReactNode;
  title: string;
  disabled?: boolean;
  busy?: boolean;
  onClick: () => void;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className={cn("inline-flex", (disabled || busy) && "cursor-not-allowed")}>
          <button
            type="button"
            aria-label={title}
            disabled={disabled || busy}
            onClick={onClick}
            className={cn(
              "inline-flex h-7 cursor-pointer items-center justify-center gap-1 rounded-md border px-2 text-[0.71rem] font-medium transition-colors duration-150 disabled:cursor-not-allowed disabled:opacity-40",
              active
                ? "border-[rgb(94_106_210_/_0.35)] bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent-hover,#4f5ac8)]"
                : "border-transparent text-[var(--t2,#62666d)] hover:border-[var(--line,#e6e6e6)] hover:bg-[var(--surface-2,#f3f4f5)] hover:text-[var(--t1,#222326)]",
            )}
          >
            {busy ? <Loader2 className="size-3.5 animate-spin" /> : icon}
          </button>
        </span>
      </TooltipTrigger>
      <TooltipContent side="bottom" sideOffset={6} className="text-xs">
        {title}
      </TooltipContent>
    </Tooltip>
  );
}

/**
 * 历史日志检索视图（§8.3/§8.4）：走 `logs.search` / `logs.export` 长操作，
 * 读取 `.supertask/logs` 落盘文件（含轮转），与实时 tail（内存环）互补。
 * 空 query = 浏览最近历史行；source=null = 全部来源。
 */
export function LogHistoryView({
  source,
  workspaceId,
  className,
  height = "100%",
}: {
  source: LogSource | null;
  workspaceId: string | null;
  className?: string;
  height?: string;
}) {
  const { t } = useTranslation();
  const { get } = useOperations();
  const { toast } = useToast();

  const [draft, setDraft] = useState("");
  const [query, setQuery] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [limit, setLimit] = useState<number>(200);
  const [nonce, setNonce] = useState(0);
  const [searchOpId, setSearchOpId] = useState<string | null>(null);
  const [result, setResult] = useState<LogsSearchResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [exportFormat, setExportFormat] = useState<"text" | "jsonl">("text");
  const [exportOpId, setExportOpId] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);

  const srcKind = source?.kind ?? null;
  const srcId = source?.id ?? null;

  // 触发搜索：范围 / 条件变化即重跑（进入视图先做一次空 query 浏览）
  useEffect(() => {
    if (!workspaceId) {
      setResult(null);
      setError(null);
      setSearchOpId(null);
      return;
    }
    let alive = true;
    const src: LogSource | null = srcKind ? { kind: srcKind, id: srcId ?? "" } : null;
    apiLogsSearch(workspaceId, query, { source: src, caseSensitive, limit })
      .then((o) => {
        if (alive) {
          setError(null);
          setSearchOpId(o.operation_id);
        }
      })
      .catch((e) => {
        if (!alive) return;
        setResult(null);
        setSearchOpId(null);
        setError(e instanceof IpcFailure ? errorDisplayText(e.code, e.message) : String(e));
      });
    return () => {
      alive = false;
    };
  }, [workspaceId, srcKind, srcId, query, caseSensitive, limit, nonce]);

  const searchOp = searchOpId ? get(searchOpId) : null;
  const searching = searchOp?.state === "queued" || searchOp?.state === "running";

  useEffect(() => {
    if (!searchOp) return;
    if (searchOp.state === "succeeded") {
      setResult((searchOp.result as LogsSearchResult | null) ?? { items: [], truncated: false, files_scanned: 0 });
      setError(null);
    } else if (searchOp.state === "failed" || searchOp.state === "cancelled") {
      setResult(null);
      setError(
        searchOp.error_code
          ? errorDisplayText(searchOp.error_code, searchOp.message)
          : (searchOp.message ?? t("logs.historySearchFailed")),
      );
    }
  }, [searchOp, t]);

  const exportOp = exportOpId ? get(exportOpId) : null;
  useEffect(() => {
    if (!exportOp) return;
    if (exportOp.state === "succeeded") {
      const n = (exportOp.result as { count?: number } | null)?.count ?? 0;
      toast(t("logs.historyExportDone", { n }), "ok");
    } else if (exportOp.state === "failed" || exportOp.state === "cancelled") {
      toast(
        exportOp.error_code
          ? errorDisplayText(exportOp.error_code, exportOp.message)
          : (exportOp.message ?? t("logs.historyExportFailed")),
        "err",
      );
    }
    setExporting(false);
    setExportOpId(null);
  }, [exportOp, toast, t]);

  const submit = () => {
    const q = draft.trim();
    if (q === query) setNonce((n) => n + 1);
    else setQuery(q);
  };

  const copyAll = async () => {
    if (!result?.items.length) return;
    const text = result.items.map((h) => `${h.kind}:${h.id} ${h.file}:${h.line_no} ${h.text}`).join("\n");
    const ok = await copyText(text);
    toast(ok ? t("common.copiedToClipboard") : t("common.copyFailedHint"), ok ? "ok" : "warn");
  };

  const onExport = async () => {
    if (!workspaceId || exporting) return;
    const ext = exportFormat === "jsonl" ? "jsonl" : "log";
    const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
    const name = `history-${srcId ?? "all"}-${stamp}.${ext}`;
    let dest: string | null = null;
    if (isTauri()) {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const sel = await save({
        defaultPath: name,
        filters: [{ name: t("logs.downloadFilterName"), extensions: [ext] }],
      });
      if (typeof sel === "string" && sel) dest = sel;
    } else {
      const p = window.prompt(t("logs.historyExportPathPrompt"), name);
      if (p) dest = p;
    }
    if (!dest) {
      toast(t("logs.historyExportCancelled"), "info");
      return;
    }
    setExporting(true);
    try {
      const o = await apiLogsExport(workspaceId, exportFormat, dest, {
        source,
        query: query || null,
        caseSensitive,
      });
      setExportOpId(o.operation_id);
    } catch (e) {
      setExporting(false);
      toast(e instanceof IpcFailure ? errorDisplayText(e.code, e.message) : String(e), "err");
    }
  };

  const items = result?.items ?? [];

  return (
    <div className={cn("flex min-h-0 flex-col", className)} style={{ height }}>
      {/* 检索工具栏 */}
      <div className="flex shrink-0 flex-wrap items-center gap-1 border-b border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] px-3 py-2">
        <div className="relative inline-flex items-center">
          <Search aria-hidden className="pointer-events-none absolute left-2 size-3.5 text-[var(--t3,#8a8f98)]" />
          <Input
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") submit();
              if (e.key === "Escape" && draft) setDraft("");
            }}
            placeholder={t("logs.historySearchPlaceholder")}
            aria-label={t("logs.historySearchPlaceholder")}
            maxLength={256}
            className="h-7 w-52 border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] pl-7 pr-2 font-mono text-[0.68rem] text-[var(--t1,#222326)] placeholder:text-[var(--t3,#8a8f98)] focus-visible:border-[var(--st-accent,#5e6ad2)] focus-visible:ring-[2px] focus-visible:ring-[var(--st-accent-tint,#eef0fb)]"
          />
        </div>
        <button
          type="button"
          onClick={submit}
          disabled={!workspaceId || searching}
          className="inline-flex h-7 cursor-pointer items-center gap-1 rounded-[var(--r-sm,8px)] border border-[rgb(94_106_210_/_0.35)] bg-[var(--st-accent,#5e6ad2)] px-2.5 text-[0.71rem] font-medium text-white transition-colors duration-150 hover:bg-[var(--st-accent-hover,#4f5ac8)] disabled:cursor-not-allowed disabled:opacity-40"
        >
          {searching ? <Loader2 className="size-3.5 animate-spin" /> : <Search className="size-3.5" />}
          {t("logs.historySearch")}
        </button>
        <HitBtn
          active={caseSensitive}
          icon={<span className="font-mono text-[0.7rem] leading-none">Aa</span>}
          title={t("logs.historyCaseAria")}
          onClick={() => setCaseSensitive((v) => !v)}
        />
        <Select value={String(limit)} onValueChange={(v) => setLimit(Number(v))}>
          <SelectTrigger size="sm" aria-label={t("logs.historyLimitAria")} className="h-7 min-w-[6.5rem] rounded-[var(--r-sm,8px)] border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] font-mono text-[0.68rem] text-[var(--t2,#62666d)] shadow-none focus-visible:border-[var(--st-accent,#5e6ad2)] focus-visible:ring-[2px] focus-visible:ring-[var(--st-accent-tint,#eef0fb)]">
            <SelectValue />
          </SelectTrigger>
          <SelectContent position="popper" sideOffset={4} className="min-w-[7rem] rounded-[var(--r-sm,8px)] py-1 font-mono text-[0.72rem]">
            {LIMIT_OPTIONS.map((n) => (
              <SelectItem key={n} value={String(n)} className="cursor-pointer">
                {t("common.linesUnit", { n })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <div className="ml-auto flex items-center gap-1">
          <HitBtn
            icon={<Copy className="size-3.5" />}
            title={t("logs.historyCopyAll")}
            disabled={items.length === 0}
            onClick={() => void copyAll()}
          />
          <Select value={exportFormat} onValueChange={(v) => setExportFormat(v as "text" | "jsonl")}>
            <SelectTrigger size="sm" aria-label={t("logs.historyExportFormatAria")} className="h-7 min-w-[5rem] rounded-[var(--r-sm,8px)] border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] font-mono text-[0.68rem] text-[var(--t2,#62666d)] shadow-none focus-visible:border-[var(--st-accent,#5e6ad2)] focus-visible:ring-[2px] focus-visible:ring-[var(--st-accent-tint,#eef0fb)]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent position="popper" sideOffset={4} className="min-w-[6rem] rounded-[var(--r-sm,8px)] py-1 font-mono text-[0.72rem]">
              <SelectItem value="text" className="cursor-pointer">text</SelectItem>
              <SelectItem value="jsonl" className="cursor-pointer">jsonl</SelectItem>
            </SelectContent>
          </Select>
          <button
            type="button"
            onClick={() => void onExport()}
            disabled={!workspaceId || exporting}
            title={t("logs.historyExport")}
            aria-label={t("logs.historyExport")}
            className="inline-flex h-7 cursor-pointer items-center gap-1 rounded-md border border-transparent px-2 text-[0.71rem] font-medium text-[var(--t2,#62666d)] transition-colors duration-150 hover:border-[var(--line,#e6e6e6)] hover:bg-[var(--surface-2,#f3f4f5)] hover:text-[var(--t1,#222326)] disabled:cursor-not-allowed disabled:opacity-40"
          >
            {exporting ? <Loader2 className="size-3.5 animate-spin" /> : <Download className="size-3.5" />}
            {t("logs.historyExport")}
          </button>
        </div>
      </div>

      {/* 结果区 */}
      <div className="min-h-0 flex-1 overflow-auto bg-[#16181D] px-3 py-2 font-mono text-[0.71rem] leading-[1.75]">
        {!workspaceId ? (
          <div className="flex h-full min-h-[8rem] items-center justify-center text-[0.75rem] text-[#5C6470]">
            {t("logs.historyNoWorkspace")}
          </div>
        ) : error ? (
          <div className="flex h-full min-h-[8rem] items-center justify-center text-[0.75rem] text-[#E06C75]">{error}</div>
        ) : searching && !result ? (
          <div className="flex h-full min-h-[8rem] items-center justify-center gap-2 text-[0.75rem] text-[#5C6470]">
            <Loader2 className="size-3.5 animate-spin" />
            {t("logs.historySearching")}
          </div>
        ) : result && result.files_scanned === 0 && items.length === 0 ? (
          <div className="flex h-full min-h-[8rem] items-center justify-center text-[0.75rem] text-[#5C6470]">
            {t("logs.historyNoFiles")}
          </div>
        ) : items.length === 0 ? (
          <div className="flex h-full min-h-[8rem] items-center justify-center text-[0.75rem] text-[#5C6470]">
            {query ? t("logs.historyNoMatch", { query }) : t("logs.historyNoFiles")}
          </div>
        ) : (
          <div className="flex flex-col">
            <div className="mb-1 flex items-center gap-2 border-b border-[#2A2D35] pb-1 text-[0.66rem] text-[#5C6470]">
              <span>{t("logs.historyStats", { files: result?.files_scanned ?? 0, hits: items.length })}</span>
              {result?.truncated ? <span className="text-[#E5C07B]">{t("logs.historyTruncated")}</span> : null}
              {searching ? <Loader2 className="size-3 animate-spin" /> : null}
            </div>
            {items.map((h, i) => (
              <div
                key={`${h.file}:${h.line_no}:${i}`}
                className="group/hit flex items-start gap-2 rounded px-1.5 py-px -mx-1.5 hover:bg-white/4"
              >
                <span className={cn("shrink-0 select-none tabular-nums", kindCls[h.kind] ?? "#9DA3AE")}>
                  {h.kind}:{h.id}
                </span>
                <span className="shrink-0 select-none tabular-nums text-[#5C6470]">
                  {h.file}:{h.line_no}
                </span>
                <span className="min-w-0 select-text break-all text-[#C9CFD6]">
                  <HighlightMatches text={h.text} query={query} />
                </span>
                <button
                  type="button"
                  title={t("logs.copyLine")}
                  aria-label={t("logs.copyLine")}
                  onClick={async (e) => {
                    e.stopPropagation();
                    const ok = await copyText(h.text);
                    toast(ok ? t("common.copiedToClipboard") : t("common.copyFailedHint"), ok ? "ok" : "warn");
                  }}
                  className="ml-auto inline-flex shrink-0 cursor-pointer items-center justify-center rounded-md border border-transparent p-1 text-[#9DA3AE] opacity-0 transition-opacity duration-150 group-hover/hit:opacity-100 hover:border-[#5C6470] hover:bg-[#2E3440] hover:text-[#F0F3F7]"
                >
                  <Copy className="size-3" />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
