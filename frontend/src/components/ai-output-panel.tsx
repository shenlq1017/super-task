import { Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { MarkdownPreview } from "@/components/markdown-preview";
import { cn } from "@/lib/utils";

export type AiOutputVariant = "ok" | "error" | "neutral";

/** AI 一次性输出正文：加载占位 + Markdown 预览（explain / test / suggest 共用）。 */
export function AiOutputBody({
  loading,
  content,
  streaming = false,
  className,
  bare = false,
}: {
  loading?: boolean;
  content: string;
  streaming?: boolean;
  className?: string;
  /** 为 true 时不加内层描边底（外层 AiOutputPanel 已着色时）。 */
  bare?: boolean;
}) {
  const { t } = useTranslation();
  const waiting = loading && !content && !streaming;

  const inner = waiting ? (
    <div
      className="flex min-h-[6rem] items-center gap-2 text-[0.82rem] text-[var(--t2,#62666d)]"
      role="status"
      aria-live="polite"
    >
      <Loader2 className="size-4 animate-spin text-[var(--st-accent,#5e6ad2)]" />
      {t("pages.ai.outputLoading")}
    </div>
  ) : (
    <MarkdownPreview content={content} streaming={streaming} />
  );

  if (bare) {
    return <div className={className}>{inner}</div>;
  }

  return (
    <div
      className={cn(
        "rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] p-3",
        className,
      )}
    >
      {inner}
    </div>
  );
}

/** 内联 AI 一次性结果面板（连接测试、配置建议等；对话框正文用 AiOutputBody）。 */
export function AiOutputPanel({
  variant,
  title,
  subtitle,
  loading,
  content,
  streaming,
  className,
}: {
  variant: AiOutputVariant;
  title: string;
  subtitle?: string;
  loading?: boolean;
  content: string;
  streaming?: boolean;
  className?: string;
}) {
  const borderBg =
    variant === "ok"
      ? "border-[#BFE0CA] bg-[#E9F7ED]"
      : variant === "error"
        ? "border-[#F0C9C9] bg-[#FDECEC]"
        : "border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)]";

  const titleColor =
    variant === "ok"
      ? "text-[#187A3D]"
      : variant === "error"
        ? "text-[var(--st-danger,#dc2626)]"
        : "text-[var(--t1,#222326)]";

  const prefix = variant === "ok" ? "✓" : variant === "error" ? "✗" : null;

  return (
    <div className={cn("rounded-[var(--r-sm,8px)] border p-3", borderBg, className)}>
      <p className={cn("text-[0.75rem] font-semibold", titleColor)}>
        {prefix ? `${prefix} ${title}` : title}
      </p>
      {subtitle ? (
        <p className="mt-0.5 font-mono text-[0.72rem] text-[var(--t2,#62666d)]">{subtitle}</p>
      ) : null}
      <div className="mt-2">
        <AiOutputBody loading={loading} content={content} streaming={streaming} bare />
      </div>
    </div>
  );
}
