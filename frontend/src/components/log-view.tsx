import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
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
import { useLogs, filterLogs } from "@/providers/logs-provider";
import type { LogLine, LogSource } from "@/ipc/protocol";
import {
  readLogLineLimitPref,
  readLogShowTimePref,
  readLogWrapPref,
  writeLogLineLimitPref,
  writeLogShowTimePref,
  writeLogWrapPref,
} from "@/lib/workspace-storage";
import { copyText } from "@/lib/copy-text";
import { downloadTextFile } from "@/lib/download-text";
import { formatLogLineText, LogLineRow } from "@/components/log-line";
import { useToast } from "@/components/ui/toast";
import {
  AlignLeft,
  ArrowDownToLine,
  Clock,
  Copy,
  Download,
  Filter,
  Loader2,
  Maximize2,
  Minimize2,
  Pause,
  Play,
  Search,
  Trash2,
  WrapText,
  X,
} from "lucide-react";

const LINE_LIMIT_PRESETS = [100, 200, 500, 1000, 2000, 5000] as const;

function clampLimit(n: number): number {
  if (!Number.isInteger(n) || n < 50) return 50;
  if (n > 5000) return 5000;
  return n;
}

function isPresetLimit(n: number): n is (typeof LINE_LIMIT_PRESETS)[number] {
  return (LINE_LIMIT_PRESETS as readonly number[]).includes(n);
}

const toolBtnBase =
  "inline-flex cursor-pointer items-center justify-center gap-1 rounded-md border text-[0.71rem] font-medium transition-colors duration-150 disabled:cursor-not-allowed disabled:opacity-40";

function ToolBtn({
  active,
  danger,
  icon,
  label,
  title,
  disabled,
  busy,
  onClick,
  className,
}: {
  active?: boolean;
  danger?: boolean;
  icon: ReactNode;
  label?: string;
  title: string;
  disabled?: boolean;
  busy?: boolean;
  onClick: () => void;
  className?: string;
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
              toolBtnBase,
              label ? "h-7 px-2" : "size-7",
              danger
                ? "border-transparent text-[var(--st-danger,#dc2626)] hover:border-red-200 hover:bg-[#FDECEC]"
                : active
                  ? "border-[rgb(94_106_210_/_0.35)] bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent-hover,#4f5ac8)]"
                  : "border-transparent text-[var(--t2,#62666d)] hover:border-[var(--line,#e6e6e6)] hover:bg-[var(--surface-2,#f3f4f5)] hover:text-[var(--t1,#222326)]",
              className,
            )}
          >
            {busy ? <Loader2 className="size-3.5 animate-spin" /> : icon}
            {label ? <span>{label}</span> : null}
          </button>
        </span>
      </TooltipTrigger>
      <TooltipContent side="bottom" sideOffset={6} className="text-xs">
        {title}
      </TooltipContent>
    </Tooltip>
  );
}

function LineLimitSelect({
  value,
  fullscreen,
  onChange,
}: {
  value: number;
  fullscreen: boolean;
  onChange: (n: number) => void;
}) {
  const [customMode, setCustomMode] = useState(() => !isPresetLimit(value));
  const [customDraft, setCustomDraft] = useState(String(value));

  useEffect(() => {
    if (!customMode) setCustomDraft(String(value));
  }, [value, customMode]);

  const commitCustom = () => {
    const n = clampLimit(Number(customDraft) || 500);
    setCustomDraft(String(n));
    onChange(n);
  };

  const selectValue = customMode ? "custom" : String(value);
  const triggerClass = fullscreen
    ? "h-7 min-w-[5.5rem] border-[#3A3F4B] bg-[#23262E] font-mono text-[0.68rem] text-[#C9CFD6] shadow-none hover:bg-[#2A2E38] focus-visible:border-[#5E6AD2] focus-visible:ring-[#5E6AD2]/20"
    : "h-7 min-w-[5.5rem] rounded-[var(--r-sm,8px)] border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] font-mono text-[0.68rem] text-[var(--t2,#62666d)] shadow-none focus-visible:border-[var(--st-accent,#5e6ad2)] focus-visible:ring-[2px] focus-visible:ring-[var(--st-accent-tint,#eef0fb)]";

  return (
    <div className="flex items-center gap-1">
      <Select
        value={selectValue}
        onValueChange={(v) => {
          if (v === "custom") {
            setCustomMode(true);
            setCustomDraft(String(value));
            return;
          }
          setCustomMode(false);
          onChange(clampLimit(Number(v)));
        }}
      >
        <SelectTrigger size="sm" className={triggerClass} aria-label="显示行数上限">
          <SelectValue placeholder="行数" />
        </SelectTrigger>
        <SelectContent
          position="popper"
          sideOffset={4}
          className={cn(
            "min-w-[7rem] rounded-[var(--r-sm,8px)] py-1 font-mono text-[0.72rem]",
            fullscreen && "border-[#3A3F4B] bg-[#23262E] text-[#C9CFD6]",
          )}
        >
          {LINE_LIMIT_PRESETS.map((n) => (
            <SelectItem key={n} value={String(n)} className="cursor-pointer">
              {n} 行
            </SelectItem>
          ))}
          <SelectItem value="custom" className="cursor-pointer">
            自定义…
          </SelectItem>
        </SelectContent>
      </Select>
      {customMode ? (
        <Input
          type="number"
          min={50}
          max={5000}
          value={customDraft}
          onChange={(e) => setCustomDraft(e.target.value)}
          onBlur={commitCustom}
          onKeyDown={(e) => {
            if (e.key === "Enter") commitCustom();
          }}
          aria-label="自定义行数"
          className={cn(
            "h-7 w-16 px-1.5 text-center font-mono text-[0.68rem]",
            fullscreen && "border-[#3A3F4B] bg-[#23262E] text-[#C9CFD6] focus-visible:border-[#5E6AD2] focus-visible:ring-[#5E6AD2]/20",
          )}
        />
      ) : null}
    </div>
  );
}

/** 视图内实时筛选框：literal、不区分大小写，只过滤当前缓冲区，不走后端历史搜索。 */
function LogFilterInput({
  value,
  matchCount,
  fullscreen,
  onChange,
}: {
  value: string;
  matchCount: number;
  fullscreen: boolean;
  onChange: (v: string) => void;
}) {
  const inputCls = fullscreen
    ? "h-7 w-40 border-[#3A3F4B] bg-[#23262E] pl-7 pr-6 font-mono text-[0.68rem] text-[#C9CFD6] placeholder:text-[#5C6470] focus-visible:border-[#5E6AD2] focus-visible:ring-[#5E6AD2]/20"
    : "h-7 w-40 border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] pl-7 pr-6 font-mono text-[0.68rem] text-[var(--t1,#222326)] placeholder:text-[var(--t3,#8a8f98)] focus-visible:border-[var(--st-accent,#5e6ad2)] focus-visible:ring-[2px] focus-visible:ring-[var(--st-accent-tint,#eef0fb)]";

  return (
    <div className="relative mr-1 inline-flex items-center">
      <Search
        aria-hidden
        className={cn(
          "pointer-events-none absolute left-2 size-3.5",
          fullscreen ? "text-[#5C6470]" : "text-[var(--t3,#8a8f98)]",
        )}
      />
      <Input
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape" && value) {
            e.stopPropagation();
            onChange("");
          }
        }}
        placeholder="筛选本视图日志…"
        aria-label="筛选本视图日志"
        className={inputCls}
      />
      {value ? (
        <span
          className={cn(
            "pointer-events-none absolute right-6 font-mono text-[0.65rem] tabular-nums",
            matchCount === 0 ? "text-[#E06C75]" : fullscreen ? "text-[#9DA3AE]" : "text-[var(--t3,#8a8f98)]",
          )}
        >
          {matchCount}
        </span>
      ) : null}
      {value ? (
        <button
          type="button"
          title="清除筛选"
          aria-label="清除筛选"
          onClick={() => onChange("")}
          className={cn(
            "absolute right-1 inline-flex size-5 cursor-pointer items-center justify-center rounded transition-colors duration-150",
            fullscreen
              ? "text-[#9DA3AE] hover:bg-[#2A2E38] hover:text-[#C9CFD6]"
              : "text-[var(--t3,#8a8f98)] hover:bg-[var(--surface-2,#f3f4f5)] hover:text-[var(--t1,#222326)]",
          )}
        >
          <X className="size-3" />
        </button>
      ) : null}
    </div>
  );
}

function LogToolbar({
  sourceLabel,
  lineLimit,
  wrap,
  showTime,
  follow,
  errorsOnly,
  fullscreen,
  canDownload,
  canClear,
  canCopy,
  downloading,
  textFilter,
  matchCount,
  onLineLimitChange,
  onWrapToggle,
  onShowTimeToggle,
  onErrorsOnlyToggle,
  onFollowToggle,
  onTextFilterChange,
  onCopy,
  onClear,
  onDownload,
  onFullscreenToggle,
}: {
  sourceLabel: string;
  lineLimit: number;
  wrap: boolean;
  showTime: boolean;
  follow: boolean;
  errorsOnly: boolean;
  fullscreen: boolean;
  canDownload: boolean;
  canClear: boolean;
  canCopy: boolean;
  downloading: boolean;
  textFilter: string;
  matchCount: number;
  onLineLimitChange: (n: number) => void;
  onWrapToggle: () => void;
  onShowTimeToggle: () => void;
  onErrorsOnlyToggle: () => void;
  onFollowToggle: () => void;
  onTextFilterChange: (v: string) => void;
  onCopy: () => void;
  onClear: () => void;
  onDownload: () => void;
  onFullscreenToggle: () => void;
}) {
  const displayControls = (
    <>
      <LogFilterInput value={textFilter} matchCount={matchCount} fullscreen={fullscreen} onChange={onTextFilterChange} />
      <LineLimitSelect value={lineLimit} fullscreen={fullscreen} onChange={onLineLimitChange} />
      <ToolBtn
        active={wrap}
        icon={wrap ? <WrapText className="size-3.5" /> : <AlignLeft className="size-3.5" />}
        title={wrap ? "切换为单行模式" : "切换为自动换行"}
        onClick={onWrapToggle}
      />
      <ToolBtn
        active={showTime}
        icon={<Clock className="size-3.5" />}
        title={showTime ? "隐藏时间戳" : "显示时间戳"}
        onClick={onShowTimeToggle}
      />
      <ToolBtn
        active={errorsOnly}
        icon={<Filter className="size-3.5" />}
        title={errorsOnly ? "显示全部日志" : "仅显示 stderr / system"}
        onClick={onErrorsOnlyToggle}
      />
      <ToolBtn
        active={follow}
        icon={follow ? <Pause className="size-3.5" /> : <Play className="size-3.5" />}
        label={follow ? "跟随" : "暂停"}
        title={follow ? "暂停跟随底部" : "跟随底部"}
        onClick={onFollowToggle}
      />
    </>
  );

  const actionControls = (
    <>
      <ToolBtn
        icon={<Copy className="size-3.5" />}
        title="复制选中行（或双击日志行）"
        disabled={!canCopy}
        onClick={onCopy}
      />
      <ToolBtn
        icon={<Download className="size-3.5" />}
        title="下载当前视图日志"
        disabled={!canDownload}
        busy={downloading}
        onClick={onDownload}
      />
      <ToolBtn
        danger
        icon={<Trash2 className="size-3.5" />}
        title="清除本视图日志"
        disabled={!canClear}
        onClick={onClear}
      />
      <ToolBtn
        icon={fullscreen ? <Minimize2 className="size-3.5" /> : <Maximize2 className="size-3.5" />}
        title={fullscreen ? "退出全屏" : "全屏查看"}
        onClick={onFullscreenToggle}
      />
    </>
  );

  return (
    <div
      className={cn(
        "relative flex shrink-0 items-center gap-1 border-b px-3 py-2",
        fullscreen ? "border-[#2A2D35] bg-[#1C1E24]" : "border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)]",
      )}
    >
      <div className="flex flex-wrap items-center gap-1">{displayControls}</div>

      {fullscreen ? (
        <div className="pointer-events-none absolute inset-x-0 flex justify-center px-36">
          <span className="truncate font-mono text-[0.72rem] font-medium text-[#9DA3AE]">{sourceLabel}</span>
        </div>
      ) : null}

      <div className="ml-auto flex items-center gap-1">{actionControls}</div>
    </div>
  );
}

function LogBody({
  lines,
  wrap,
  showTime,
  follow,
  filterQuery,
  selectedSeq,
  copiedSeq,
  onSelect,
  onCopyLine,
  onJumpBottom,
  scrollRef,
}: {
  lines: LogLine[];
  wrap: boolean;
  showTime: boolean;
  follow: boolean;
  /** 非空表示视图内筛选生效中（用于空态文案与行内高亮）。 */
  filterQuery: string;
  selectedSeq: number | null;
  copiedSeq: number | null;
  onSelect: (seq: number) => void;
  onCopyLine: (seq: number) => void;
  onJumpBottom: () => void;
  scrollRef: React.RefObject<HTMLDivElement | null>;
}) {
  useLayoutEffect(() => {
    if (follow && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [lines.length, follow, scrollRef]);

  return (
    <div className="relative min-h-0 flex-1">
      <div
        ref={scrollRef}
        className={cn(
          "h-full bg-[#16181D] px-3 py-2 font-mono text-[0.71rem] leading-[1.75]",
          wrap ? "overflow-y-auto overflow-x-hidden" : "overflow-auto",
        )}
      >
        {lines.length === 0 ? (
          <div className="flex h-full min-h-[8rem] items-center justify-center text-[0.75rem] text-[#5C6470]">
            {filterQuery ? `本视图没有匹配「${filterQuery.trim()}」的日志行` : "暂无日志输出"}
          </div>
        ) : (
          <div className="flex flex-col">
            {lines.map((l) => (
              <LogLineRow
                key={l.seq}
                line={l}
                wrap={wrap}
                showTime={showTime}
                selected={selectedSeq === l.seq}
                copied={copiedSeq === l.seq}
                highlight={filterQuery.trim() || undefined}
                onSelect={() => onSelect(l.seq)}
                onCopy={(e) => {
                  e.stopPropagation();
                  onCopyLine(l.seq);
                }}
              />
            ))}
          </div>
        )}
      </div>
      {!follow && lines.length > 0 ? (
        <button
          type="button"
          onClick={onJumpBottom}
          title="回到底部并恢复跟随"
          className="absolute bottom-3 right-3 inline-flex cursor-pointer items-center gap-1 rounded-full border border-[#3A3F4B] bg-[#23262E] px-3 py-1.5 text-[0.72rem] font-medium text-[#D5DAE1] shadow-lg transition-colors duration-150 hover:border-[#5E6AD2] hover:bg-[#2A2E38]"
        >
          <ArrowDownToLine className="size-3.5" />
          回到底部
        </button>
      ) : null}
    </div>
  );
}

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
  const { toast } = useToast();
  const [follow, setFollow] = useState(true);
  const [wrap, setWrap] = useState(() => readLogWrapPref());
  const [showTime, setShowTime] = useState(() => readLogShowTimePref());
  const [errorsOnly, setErrorsOnly] = useState(false);
  const [textFilter, setTextFilter] = useState("");
  const [lineLimit, setLineLimit] = useState(() => readLogLineLimitPref());
  const [fullscreen, setFullscreen] = useState(false);
  const [selectedSeq, setSelectedSeq] = useState<number | null>(null);
  const [copiedSeq, setCopiedSeq] = useState<number | null>(null);
  const [downloading, setDownloading] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const fullscreenScrollRef = useRef<HTMLDivElement>(null);

  const filtered = useMemo(() => filterLogs(state.all, source), [state.all, source]);
  const filterQuery = textFilter.trim();
  // 筛选先于行数上限：命中整个缓冲区，再按上限取尾部，避免命中被视图裁剪吞掉
  const matchFiltered = useMemo(
    () =>
      filterQuery
        ? filtered.filter((l) => l.text.toLowerCase().includes(filterQuery.toLowerCase()))
        : filtered,
    [filtered, filterQuery],
  );
  const streamFiltered = useMemo(
    () => (errorsOnly ? matchFiltered.filter((l) => l.stream === "stderr" || l.stream === "system") : matchFiltered),
    [matchFiltered, errorsOnly],
  );
  const lines = useMemo(() => {
    const limit = clampLimit(lineLimit);
    return streamFiltered.length > limit ? streamFiltered.slice(streamFiltered.length - limit) : streamFiltered;
  }, [streamFiltered, lineLimit]);

  const sourceLabel = source ? `${source.kind}:${source.id}` : "全部服务";

  useEffect(() => {
    setSelectedSeq(null);
    setCopiedSeq(null);
    setTextFilter("");
  }, [source?.kind, source?.id]);

  useEffect(() => {
    if (fullscreen) {
      document.body.style.overflow = "hidden";
      return () => {
        document.body.style.overflow = "";
      };
    }
  }, [fullscreen]);

  useEffect(() => {
    const el = fullscreen ? fullscreenScrollRef.current : scrollRef.current;
    if (!el) return;
    const onScroll = () => {
      const bottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
      setFollow(bottom);
    };
    el.addEventListener("scroll", onScroll);
    return () => el.removeEventListener("scroll", onScroll);
  }, [fullscreen, lines.length]);

  useEffect(() => {
    if (!fullscreen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setFullscreen(false);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [fullscreen]);

  const copyLine = useCallback(
    async (seq: number, silent = false) => {
      const line = lines.find((l) => l.seq === seq);
      if (!line) return false;
      const ok = await copyText(formatLogLineText(line, showTime));
      if (ok) {
        setCopiedSeq(seq);
        window.setTimeout(() => setCopiedSeq((cur) => (cur === seq ? null : cur)), 2500);
        if (!silent) toast("已复制到剪贴板", "ok");
      } else if (!silent) {
        toast("复制失败，请手动选择文本后 Ctrl+C", "warn");
      }
      return ok;
    },
    [lines, showTime, toast],
  );

  const copySelected = useCallback(() => {
    if (selectedSeq == null) return;
    void copyLine(selectedSeq);
  }, [copyLine, selectedSeq]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (selectedSeq == null) return;
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "c") {
        const tag = (e.target as HTMLElement | null)?.tagName;
        if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
        e.preventDefault();
        void copyLine(selectedSeq, true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [copyLine, selectedSeq]);

  const downloadLogs = async () => {
    if (!lines.length || downloading) return;
    setDownloading(true);
    try {
      const text = lines.map((l) => formatLogLineText(l, showTime)).join("\n");
      const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
      const name = source ? `${source.id}-${stamp}.log` : `all-services-${stamp}.log`;
      const result = await downloadTextFile(name, text);
      if (result === "saved") toast("日志已保存", "ok");
      else if (result === "cancelled") toast("已取消保存", "info");
      else toast("下载失败，请重试", "err");
    } finally {
      setDownloading(false);
    }
  };

  const jumpBottom = () => {
    const el = fullscreen ? fullscreenScrollRef.current : scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
    setFollow(true);
  };

  const toolbarProps = {
    sourceLabel,
    lineLimit,
    wrap,
    showTime,
    follow,
    errorsOnly,
    fullscreen,
    canDownload: lines.length > 0,
    canClear: Boolean(source),
    canCopy: selectedSeq != null,
    downloading,
    textFilter,
    matchCount: streamFiltered.length,
    onTextFilterChange: setTextFilter,
    onLineLimitChange: (n: number) => {
      const v = clampLimit(n);
      setLineLimit(v);
      writeLogLineLimitPref(v);
    },
    onWrapToggle: () => {
      setWrap((w) => {
        const next = !w;
        writeLogWrapPref(next);
        return next;
      });
    },
    onShowTimeToggle: () => {
      setShowTime((v) => {
        const next = !v;
        writeLogShowTimePref(next);
        return next;
      });
    },
    onErrorsOnlyToggle: () => setErrorsOnly((v) => !v),
    onFollowToggle: () => setFollow((v) => !v),
    onCopy: copySelected,
    onClear: () => source && void actions.clear(source),
    onDownload: () => void downloadLogs(),
    onFullscreenToggle: () => setFullscreen((v) => !v),
  };

  const bodyProps = {
    lines,
    wrap,
    showTime,
    follow,
    filterQuery,
    selectedSeq,
    copiedSeq,
    onSelect: (seq: number) => {
      setSelectedSeq((cur) => (cur === seq ? null : seq));
    },
    onCopyLine: (seq: number) => {
      setSelectedSeq(seq);
      void copyLine(seq);
    },
    onJumpBottom: jumpBottom,
  };

  if (fullscreen) {
    return (
      <div className={cn("flex flex-col", className)} style={{ height }}>
        <div className="fixed inset-0 z-[220] flex flex-col bg-[#16181D]">
          <LogToolbar {...toolbarProps} fullscreen />
          <LogBody {...bodyProps} scrollRef={fullscreenScrollRef} />
        </div>
      </div>
    );
  }

  return (
    <div className={cn("flex flex-col", className)} style={{ height }}>
      <LogToolbar {...toolbarProps} fullscreen={false} />
      <LogBody {...bodyProps} scrollRef={scrollRef} />
    </div>
  );
}
