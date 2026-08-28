/**
 * 1.4 Taskfile 导入向导（feature spec §7 / §11.2，ipc.md §10.8）。
 * 预览表 → 勾选 → 应用到 supertask.yaml；交互样式对齐 1.1 扫描合并向导（config-page ScanPreviewPanel）。
 * 仅展示与勾选，apply 调用方负责（yaml.saveForm 机制 + YAML_CONFLICT 对话框）。
 */
import { useTranslation } from "react-i18next";
import { ArrowRight, Ban, TriangleAlert } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { TaskfileImportItem, TaskfilePreviewOut } from "@/ipc/protocol";

function ItemWarnings({ warnings }: { warnings: string[] }) {
  if (!warnings.length) return null;
  return (
    <div className="mt-1 flex flex-col gap-0.5">
      {warnings.map((w, i) => (
        <span
          key={i}
          className="flex items-start gap-1 text-[0.72rem] leading-relaxed text-[#B7791F]"
        >
          <TriangleAlert className="mt-0.5 size-3 shrink-0" aria-hidden />
          {w}
        </span>
      ))}
    </div>
  );
}

function TaskRow({
  item,
  checked,
  onToggle,
}: {
  item: TaskfileImportItem;
  checked: boolean;
  onToggle: (v: boolean) => void;
}) {
  const { t } = useTranslation();
  const greyed = item.internal;
  return (
    <div
      className={cn(
        "rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] p-2.5",
        greyed && "opacity-55",
      )}
    >
      <div className="flex flex-wrap items-center gap-2">
        <label
          className={cn(
            "flex shrink-0 items-center gap-1.5 text-[0.76rem] font-medium text-[var(--t1,#222326)]",
            greyed ? "cursor-not-allowed opacity-60" : "cursor-pointer",
          )}
        >
          <input
            type="checkbox"
            checked={checked}
            disabled={greyed}
            onChange={(e) => onToggle(e.target.checked)}
            aria-label={t("pages.config.taskfile.rowAria", { task: item.task })}
          />
          {greyed ? <Ban className="size-3.5 text-[var(--t3,#8a8f98)]" aria-hidden /> : null}
          {greyed ? t("pages.config.taskfile.internal") : t("pages.config.taskfile.importAs")}
        </label>
        <span className="font-mono text-[0.82rem] font-semibold text-[var(--t1,#222326)]" title={item.task}>
          {item.task}
        </span>
        <ArrowRight className="size-3.5 shrink-0 text-[var(--t3,#8a8f98)]" aria-hidden />
        <span className="font-mono text-[0.78rem] text-[var(--st-accent,#5e6ad2)]">{item.script_id}</span>
        <Badge variant="outline" className="font-mono text-[10px]">
          {t("pages.config.taskfile.cmdsCount", { n: item.cmds_count })}
        </Badge>
        {item.id_conflict ? (
          <Badge variant="outline" className="border-[#f0d58a] bg-[#fdf6e3] text-[#B7791F]">
            {t("pages.config.taskfile.conflictBadge")}
          </Badge>
        ) : null}
      </div>
      <ItemWarnings warnings={item.warnings} />
    </div>
  );
}

export function TaskfileImportPanel({
  preview,
  checked,
  onToggle,
  onSelectAll,
  applying,
  applyCount,
  onApply,
  onClose,
}: {
  preview: TaskfilePreviewOut;
  checked: Record<string, boolean>;
  onToggle: (scriptId: string, v: boolean) => void;
  onSelectAll: (v: boolean) => void;
  applying: boolean;
  applyCount: number;
  onApply: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const selectable = preview.tasks.filter((it) => !it.internal);
  const allChecked = selectable.length > 0 && selectable.every((it) => checked[it.script_id]);

  return (
    <section
      className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[var(--r-lg,16px)] border border-[rgb(94_106_210_/_0.35)] bg-[var(--surface,#fff)] shadow-[var(--shadow-2,0_6px_20px_rgb(16_24_40_/_0.09))]"
      aria-label={t("pages.config.taskfile.panelAria")}
    >
      <div className="flex shrink-0 flex-wrap items-center gap-2 rounded-t-[var(--r-lg,16px)] border-b border-[var(--line,#e6e6e6)] bg-[var(--st-accent-tint,#eef0fb)] px-3 py-2.5">
        <h3 className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">
          {t("pages.config.taskfile.title")}
        </h3>
        <span className="font-mono text-[0.7rem] text-[var(--t3,#8a8f98)]">
          {t("pages.config.taskfile.itemCount", { n: preview.tasks.length })}
        </span>
        {selectable.length > 0 ? (
          <Button size="sm" variant="outline" onClick={() => onSelectAll(!allChecked)}>
            {allChecked ? t("pages.config.taskfile.unselectAll") : t("pages.config.taskfile.selectAll", { n: selectable.length })}
          </Button>
        ) : null}
        <Button variant="outline" size="sm" className="ml-auto" onClick={onClose}>
          {t("common.close")}
        </Button>
      </div>

      {preview.warnings.length > 0 ? (
        <div
          className="mx-3 mt-2 rounded-[var(--r-sm,8px)] border border-[#f0d58a] bg-[#fdf6e3] px-2.5 py-1.5 text-[0.74rem] leading-relaxed text-[#B7791F]"
          role="alert"
        >
          {preview.warnings.map((w, i) => (
            <div key={i}>{w}</div>
          ))}
        </div>
      ) : null}

      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-2">
        {preview.tasks.length === 0 ? (
          <p className="py-6 text-center text-[0.8rem] text-[var(--t3,#8a8f98)]">
            {t("pages.config.taskfile.empty")}
          </p>
        ) : (
          <div className="flex flex-col gap-1.5">
            {preview.tasks.map((it) => (
              <TaskRow
                key={it.script_id}
                item={it}
                checked={checked[it.script_id] ?? false}
                onToggle={(v) => onToggle(it.script_id, v)}
              />
            ))}
          </div>
        )}
      </div>

      <div className="flex shrink-0 items-center gap-2 rounded-b-[var(--r-lg,16px)] border-t border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] px-3 py-2.5">
        <span className="min-w-0 flex-1 text-[0.74rem] text-[var(--t3,#8a8f98)]">
          {t("pages.config.taskfile.applyHint")}
        </span>
        <Button size="sm" variant="default" onClick={onApply} disabled={applying || applyCount === 0}>
          {applying ? t("pages.config.taskfile.applying") : t("pages.config.taskfile.applySelected", { n: applyCount })}
        </Button>
      </div>
    </section>
  );
}
