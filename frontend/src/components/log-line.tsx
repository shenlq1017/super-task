import { type MouseEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Check, Copy } from "lucide-react";
import { cn } from "@/lib/utils";
import type { LogLine } from "@/ipc/protocol";
import { fmtTime } from "@/lib/status";

export type LogStreamStyle = {
  bar: string;
  text: string;
  hoverBg: string;
};

export function streamStyle(stream: LogLine["stream"]): LogStreamStyle {
  if (stream === "stderr") {
    return { bar: "#E06C75", text: "text-[#F0ABA8]", hoverBg: "hover:bg-[#E06C75]/8" };
  }
  if (stream === "system") {
    return { bar: "#E5C07B", text: "text-[#E8D49A]", hoverBg: "hover:bg-[#E5C07B]/8" };
  }
  return { bar: "transparent", text: "text-[#C9CFD6]", hoverBg: "hover:bg-[#FFFFFF]/4" };
}

export function formatLogLineText(l: LogLine, showTime: boolean): string {
  const time = showTime ? `${fmtTime(l.ts_ms)} ` : "";
  return `${time}${l.text}`;
}

/** 按字面量（不区分大小写）把命中片段高亮出来；query 为空时原样返回。 */
export function HighlightMatches({ text, query }: { text: string; query: string }) {
  const q = query.toLowerCase();
  if (!q) return <>{text}</>;
  const parts: ReactNode[] = [];
  const lower = text.toLowerCase();
  let from = 0;
  let at = lower.indexOf(q);
  let key = 0;
  while (at !== -1) {
    if (at > from) parts.push(text.slice(from, at));
    parts.push(
      <mark key={key++} className="rounded-[2px] bg-[#E5C07B]/35 text-inherit">
        {text.slice(at, at + q.length)}
      </mark>,
    );
    from = at + q.length;
    at = lower.indexOf(q, from);
  }
  if (from < text.length) parts.push(text.slice(from));
  return <>{parts}</>;
}

export function LogLineRow({
  line,
  wrap,
  showTime,
  selected,
  copied,
  highlight,
  onSelect,
  onCopy,
}: {
  line: LogLine;
  wrap: boolean;
  showTime: boolean;
  selected: boolean;
  copied: boolean;
  /** 视图内筛选关键词；非空时在行内高亮命中片段。 */
  highlight?: string;
  onSelect: () => void;
  onCopy: (e: MouseEvent) => void;
}) {
  const { t } = useTranslation();
  const style = streamStyle(line.stream);
  const hasBar = line.stream !== "stdout";

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onSelect}
      onDoubleClick={(e) => {
        e.preventDefault();
        onCopy(e);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") onSelect();
      }}
      className={cn(
        "group/line relative flex min-h-[1.35rem] cursor-pointer items-center gap-2 rounded px-1.5 py-px -mx-1.5 transition-colors duration-100",
        wrap ? "w-full whitespace-pre-wrap break-all pr-14" : "w-max min-w-full whitespace-nowrap",
        style.hoverBg,
        selected && "bg-[rgb(94_106_210_/_0.2)] ring-1 ring-[rgb(94_106_210_/_0.4)]",
      )}
    >
      <span
        aria-hidden
        className={cn("my-0.5 w-0.5 shrink-0 self-stretch rounded-full", hasBar ? "opacity-100" : "opacity-0 group-hover/line:opacity-30")}
        style={{ background: hasBar ? style.bar : "#4B5263" }}
      />
      {showTime ? (
        <span className="shrink-0 select-none tabular-nums text-[#5C6470]">{fmtTime(line.ts_ms)}</span>
      ) : null}
      <span className={cn("min-w-0 select-text", style.text, wrap ? "flex-1 break-all" : "shrink-0")}>
        {highlight ? <HighlightMatches text={line.text} query={highlight} /> : line.text}
      </span>
      <button
        type="button"
        title={copied ? t("common.copied") : t("logs.copyLine")}
        aria-label={copied ? t("common.copied") : t("logs.copyLine")}
        tabIndex={selected ? 0 : -1}
        onClick={(e) => {
          e.stopPropagation();
          onCopy(e);
        }}
        className={cn(
          "inline-flex shrink-0 cursor-pointer items-center justify-center gap-1 rounded-md border px-2 py-0.5 text-[0.65rem] font-medium shadow-sm transition-[opacity,colors] duration-150",
          wrap
            ? "absolute top-1/2 right-1 min-w-[4.5rem] -translate-y-1/2"
            : "sticky right-0 ml-2 min-w-[4.5rem] bg-[#16181D] shadow-[-10px_0_14px_#16181D]",
          selected
            ? copied
              ? "pointer-events-auto border-[#6BCF8A] bg-[#1B3D28] text-[#B8F5C8] opacity-100"
              : "pointer-events-auto border-[#5C6470] bg-[#2E3440] text-[#F0F3F7] opacity-100 hover:border-[#8B93FF] hover:bg-[#3D4460] hover:text-white"
            : "pointer-events-none border-transparent bg-transparent text-transparent opacity-0",
          !wrap && !selected && "shadow-none",
        )}
      >
        {copied ? <Check className="size-3" /> : <Copy className="size-3" />}
        <span className="hidden sm:inline">{copied ? t("common.copied") : t("common.copy")}</span>
      </button>
    </div>
  );
}
