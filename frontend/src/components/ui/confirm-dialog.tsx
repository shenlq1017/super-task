import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "./button";
import { cn } from "@/lib/utils";
import { TriangleAlert } from "lucide-react";

/**
 * 通用确认弹框（项目无 radix dialog，自绘与设计语言一致）。
 * Esc / 点遮罩 = 取消；确认按钮支持 destructive 红色样式。
 */
export function ConfirmDialog({
  open,
  title,
  description,
  confirmText,
  cancelText,
  destructive,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  description?: React.ReactNode;
  confirmText?: string;
  cancelText?: string;
  /** 确认动作为危险操作（停止/删除）时用红色强调 */
  destructive?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const confirm = confirmText ?? t("common.confirm");
  const cancel = cancelText ?? t("common.cancel");
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onCancel();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, onCancel]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-[200] grid place-items-center bg-black/40 backdrop-blur-[1px]"
      onClick={onCancel}
      role="dialog"
      aria-modal="true"
      aria-label={title}
    >
      <div
        className={cn(
          "mx-4 w-[24rem] rounded-xl border bg-[var(--surface,#fff)] p-4 shadow-2xl",
          destructive ? "border-red-200" : "border-[var(--line,#e6e6e6)]",
        )}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-1.5 flex items-center gap-2">
          {destructive ? (
            <span className="grid size-7 shrink-0 place-items-center rounded-full bg-[#FDECEC]">
              <TriangleAlert className="size-4 text-[#DC2626]" />
            </span>
          ) : null}
          <span className="text-[0.92rem] font-semibold text-[var(--t1,#222326)]">{title}</span>
        </div>
        {description ? (
          <div className="whitespace-pre-wrap break-words pl-9 text-[0.8rem] leading-relaxed text-[var(--t2,#62666d)]">
            {description}
          </div>
        ) : null}
        <div className="mt-4 flex justify-end gap-2">
          <Button variant="outline" size="sm" onClick={onCancel}>
            {cancel}
          </Button>
          <Button
            size="sm"
            autoFocus
            className={
              destructive
                ? "border-[#DC2626] bg-[#DC2626] hover:border-[#b91c1c] hover:bg-[#b91c1c]"
                : undefined
            }
            onClick={onConfirm}
          >
            {confirm}
          </Button>
        </div>
      </div>
    </div>
  );
}
