import { useTranslation } from "react-i18next";
import { Loader2, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { LogLine, LogSource, RtState } from "@/ipc/protocol";
import { useAiExplain } from "@/providers/ai-explain-provider";

/**
 * 「AI 解释」动作（v2.1 规格 §4.2 场景 1 / §7）：把当前视图行发给 ai.complete，
 * 结果在全局对话框中展示。只读、只建议，不写任何东西；由用户显式点击触发。
 * 通过 log-view 的 extraActions 槽位注入，运行页与日志页共用（零分叉）。
 */
export function AiExplainButton({
  lines,
  source,
  serviceKind,
  servicePort,
  serviceState,
}: {
  /** 当前视图行（LogView 传入；后端再做 200 行 / 32KB 尾截断与 sanitize）。 */
  lines: LogLine[];
  source: LogSource | null;
  serviceKind?: string | null;
  servicePort?: number | null;
  serviceState?: RtState | null;
}) {
  const { t } = useTranslation();
  const { startExplain, busy } = useAiExplain();

  const disabled = lines.length === 0;

  const explain = () => {
    if (busy || disabled) return;
    void startExplain({
      service: source
        ? {
            id: source.id,
            kind: serviceKind ?? "unknown",
            port: servicePort ?? null,
            state: serviceState ?? null,
          }
        : null,
      lines: lines.map((l) => l.text),
    });
  };

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className={disabled ? "inline-flex cursor-not-allowed" : "inline-flex"}>
          <Button
            variant="soft"
            size="sm"
            className="gap-1"
            disabled={disabled || busy}
            aria-label={t("pages.ai.explainAction")}
            onClick={explain}
          >
            {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Sparkles className="size-3.5" />}
            {t("pages.ai.explainAction")}
          </Button>
        </span>
      </TooltipTrigger>
      <TooltipContent side="bottom" sideOffset={6} className="text-xs">
        {t("pages.ai.explainTooltip")}
      </TooltipContent>
    </Tooltip>
  );
}
