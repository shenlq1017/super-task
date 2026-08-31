import { useEffect } from "react";
import { TriangleAlert } from "lucide-react";
import { Button } from "./button";
import { cn } from "@/lib/utils";
import { t } from "@/lib/labels";

/**
 * 通用确认弹框：Esc / 点遮罩 = 取消；确认按钮支持 destructive 红色样式。
 * 与 frontend/ 的同名组件同源，只是文案取自 labels.ts 而非 i18next。
 */
export function ConfirmDialog({
  open,
  title,
  description,
  confirmText,
  cancelText,
  destructive,
  busy,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  description?: React.ReactNode;
  confirmText?: string;
  cancelText?: string;
  /** 确认动作为危险操作（停用/删除）时用红色强调 */
  destructive?: boolean;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
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
          "mx-4 w-[26rem] rounded-xl border bg-[var(--surface,#fff)] p-4 shadow-2xl",
          destructive ? "border-red-200" : "border-[var(--line,#e6e6e6)]",
        )}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-1.5 flex items-center gap-2">
          {destructive ? (
            <span className="grid size-7 shrink-0 place-items-center rounded-full bg-[var(--st-danger-tint,#fdecec)]">
              <TriangleAlert className="size-4 text-[var(--st-danger,#dc2626)]" />
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
          <Button variant="outline" size="sm" onClick={onCancel} disabled={busy}>
            {cancel}
          </Button>
          <Button
            size="sm"
            autoFocus
            variant={destructive ? "destructive" : "default"}
            disabled={busy}
            onClick={onConfirm}
          >
            {busy ? t("common.loading") : confirm}
          </Button>
        </div>
      </div>
    </div>
  );
}
