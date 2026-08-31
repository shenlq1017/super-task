import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { AiOutputBody } from "@/components/ai-output-panel";
import { cn } from "@/lib/utils";

/** AI 日志解释结果对话框（全局挂载；z 高于全屏日志 z-[220]）。 */
export function AiExplainDialog({
  open,
  onOpenChange,
  loading,
  text,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  loading: boolean;
  text: string;
}) {
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onOpenChange(false);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, onOpenChange]);

  useEffect(() => {
    if (!open || !scrollRef.current) return;
    scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
  }, [open, text, loading]);

  if (!open) return null;

  const waiting = loading && !text;

  return (
    <div
      className="fixed inset-0 z-[230] flex items-center justify-center bg-black/40 p-4 backdrop-blur-[1px]"
      onClick={() => onOpenChange(false)}
      role="dialog"
      aria-modal="true"
      aria-labelledby="ai-explain-title"
      aria-busy={loading}
    >
      <div
        className={cn(
          "flex max-h-[min(80vh,40rem)] w-full max-w-2xl flex-col overflow-hidden rounded-xl border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] shadow-2xl",
        )}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex shrink-0 items-start gap-3 border-b border-[var(--line,#e6e6e6)] px-4 py-3">
          <div className="min-w-0 flex-1">
            <h2 id="ai-explain-title" className="text-[0.92rem] font-semibold text-[var(--t1,#222326)]">
              {t("pages.ai.explainDialogTitle")}
            </h2>
            <p className="mt-1 text-[0.75rem] leading-relaxed text-[var(--t3,#8a8f98)]">
              {t("pages.ai.explainDialogHint")}
            </p>
          </div>
          {loading ? (
            <Loader2
              className="mt-0.5 size-4 shrink-0 animate-spin text-[var(--st-accent,#5e6ad2)]"
              aria-hidden
            />
          ) : null}
          <Button
            variant="ghost"
            size="icon-sm"
            className="shrink-0"
            aria-label={t("common.close")}
            onClick={() => onOpenChange(false)}
          >
            <X className="size-4" />
          </Button>
        </div>
        <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto p-3">
          <AiOutputBody loading={waiting} content={text} streaming={loading && !!text} />
        </div>
      </div>
    </div>
  );
}
