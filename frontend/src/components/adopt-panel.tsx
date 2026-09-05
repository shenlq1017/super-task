/**
 * 孤儿进程纳管向导（ipc.md §10.16）。
 * dry-run 预览 → 勾选 → 应用到 supertask.yaml；交互样式对齐 Taskfile 导入向导。
 * 仅展示与勾选，apply 调用方负责（yaml.saveForm 机制 + YAML_CONFLICT 对话框）。
 * 命令行 / 草稿参数已由 core 脱敏（敏感值为 <redacted>）。
 */
import { useTranslation } from "react-i18next";
import { ArrowRight, Ban, CircleHelp, TriangleAlert } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { AdoptItem, AdoptPreviewOut, AdoptStatus } from "@/ipc/protocol";

const STATUS_BADGE: Record<AdoptStatus, { className: string; key: string }> = {
  adoptable: {
    className: "border-[rgb(94_106_210_/_0.35)] bg-[rgb(94_106_210_/_0.08)] text-[var(--st-accent,#5e6ad2)]",
    key: "adoptable",
  },
  matched: {
    className: "border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] text-[var(--t2,#62666d)]",
    key: "matched",
  },
  id_conflict: {
    className: "border-[#f0d58a] bg-[#fdf6e3] text-[#B7791F]",
    key: "idConflict",
  },
  unadoptable: {
    className: "border-red-200 bg-[var(--st-danger-tint,#fdecec)] text-[#DC2626]",
    key: "unadoptable",
  },
};

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

function AdoptRow({
  item,
  checked,
  onToggle,
}: {
  item: AdoptItem;
  checked: boolean;
  onToggle: (v: boolean) => void;
}) {
  const { t } = useTranslation();
  const selectable = item.status === "adoptable" || item.status === "id_conflict";
  const badge = STATUS_BADGE[item.status];
  return (
    <div
      className={cn(
        "rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] p-2.5",
        !selectable && "opacity-60",
      )}
    >
      <div className="flex flex-wrap items-center gap-2">
        <label
          className={cn(
            "flex shrink-0 items-center gap-1.5 text-[0.76rem] font-medium text-[var(--t1,#222326)]",
            selectable ? "cursor-pointer" : "cursor-not-allowed opacity-60",
          )}
        >
          <input
            type="checkbox"
            checked={checked}
            disabled={!selectable}
            onChange={(e) => onToggle(e.target.checked)}
            aria-label={t("pages.discover.adopt.rowAria", { name: item.process_name, pid: item.pid })}
          />
          {!selectable ? <Ban className="size-3.5 text-[var(--t3,#8a8f98)]" aria-hidden /> : null}
        </label>
        <Badge variant="outline" className={cn("shrink-0", badge.className)}>
          {t(`pages.discover.adopt.status.${badge.key}`)}
        </Badge>
        <span className="font-mono text-[0.78rem] font-semibold text-[var(--t1,#222326)]" title={item.process_name}>
          {item.process_name}
        </span>
        <span className="font-mono text-[0.7rem] text-[var(--t3,#8a8f98)]">PID {item.pid}</span>
        {item.parent_pid != null ? (
          <span
            className="inline-flex items-center gap-0.5 text-[0.7rem] text-[var(--t3,#8a8f98)]"
            title={t("pages.discover.adopt.parentTitle")}
          >
            <CircleHelp className="size-3 shrink-0" aria-hidden />
            {t("pages.discover.adopt.parent", {
              name: item.parent_name ?? "?",
              pid: item.parent_pid,
            })}
          </span>
        ) : null}
        <span className="flex flex-wrap items-center gap-1">
          {item.ports.map((p) => (
            <span
              key={p}
              className="inline-flex h-5 items-center rounded-full bg-[var(--surface-2,#f3f4f5)] px-1.5 font-mono text-[0.68rem] leading-none text-[var(--primary,#5E6AD2)]"
            >
              {p}
            </span>
          ))}
        </span>
      </div>

      <div className="mt-1.5 flex flex-col gap-1 pl-6">
        {item.draft ? (
          <span className="flex min-w-0 flex-wrap items-center gap-1.5">
            <ArrowRight className="size-3.5 shrink-0 text-[var(--t3,#8a8f98)]" aria-hidden />
            <span className="font-mono text-[0.78rem] text-[var(--st-accent,#5e6ad2)]">
              {item.candidate_id ?? item.service_id}
            </span>
            <span className="min-w-0 break-all font-mono text-[0.72rem] text-[var(--t2,#62666d)]">
              {item.draft.program}
              {item.draft.args?.length ? ` ${item.draft.args.join(" ")}` : ""}
            </span>
            {item.draft.dir ? (
              <Badge variant="outline" className="font-mono text-[10px]" title={t("pages.discover.adopt.dirBadge")}>
                {item.draft.dir}
              </Badge>
            ) : null}
          </span>
        ) : null}
        {item.cwd ? (
          <span className="block truncate font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]" title={item.cwd}>
            {item.cwd}
          </span>
        ) : null}
        {item.reason ? (
          <span className="text-[0.72rem] leading-relaxed text-[var(--t2,#62666d)]">{item.reason}</span>
        ) : null}
        <ItemWarnings warnings={item.warnings} />
      </div>
    </div>
  );
}

export function AdoptPanel({
  preview,
  checked,
  onToggle,
  onSelectAll,
  applying,
  applyCount,
  onApply,
  onClose,
}: {
  preview: AdoptPreviewOut;
  checked: Record<number, boolean>;
  onToggle: (pid: number, v: boolean) => void;
  onSelectAll: (v: boolean) => void;
  applying: boolean;
  applyCount: number;
  onApply: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const selectable = preview.items.filter((it) => it.status === "adoptable" || it.status === "id_conflict");
  const allChecked = selectable.length > 0 && selectable.every((it) => checked[it.pid]);

  return (
    <section
      className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[var(--r-lg,16px)] border border-[rgb(94_106_210_/_0.35)] bg-[var(--surface,#fff)] shadow-[var(--shadow-2,0_6px_20px_rgb(16_24_40_/_0.09))]"
      aria-label={t("pages.discover.adopt.aria")}
    >
      <div className="flex shrink-0 flex-wrap items-center gap-2 rounded-t-[var(--r-lg,16px)] border-b border-[var(--line,#e6e6e6)] bg-[var(--st-accent-tint,#eef0fb)] px-3 py-2.5">
        <h3 className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">{t("pages.discover.adopt.title")}</h3>
        <span className="font-mono text-[0.7rem] text-[var(--t3,#8a8f98)]">
          {t("pages.discover.adopt.itemCount", { n: preview.items.length })}
        </span>
        {selectable.length > 0 ? (
          <Button size="sm" variant="outline" onClick={() => onSelectAll(!allChecked)}>
            {allChecked
              ? t("pages.discover.adopt.unselectAll")
              : t("pages.discover.adopt.selectAll", { n: selectable.length })}
          </Button>
        ) : null}
        <Button variant="outline" size="sm" className="ml-auto" onClick={onClose}>
          {t("common.close")}
        </Button>
      </div>

      {preview.warnings.length > 0 ? (
        <div
          className="mx-3 mt-2 max-h-24 overflow-y-auto rounded-[var(--r-sm,8px)] border border-[#f0d58a] bg-[#fdf6e3] px-2.5 py-1.5 text-[0.74rem] leading-relaxed text-[#B7791F]"
          role="alert"
        >
          {preview.warnings.map((w, i) => (
            <div key={i}>{w}</div>
          ))}
        </div>
      ) : null}

      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-2">
        {preview.items.length === 0 ? (
          <p className="py-6 text-center text-[0.8rem] text-[var(--t3,#8a8f98)]">
            {t("pages.discover.adopt.empty")}
          </p>
        ) : (
          <div className="flex flex-col gap-1.5">
            {preview.items.map((it) => (
              <AdoptRow
                key={it.pid}
                item={it}
                checked={checked[it.pid] ?? false}
                onToggle={(v) => onToggle(it.pid, v)}
              />
            ))}
          </div>
        )}
      </div>

      <div className="flex shrink-0 items-center gap-2 rounded-b-[var(--r-lg,16px)] border-t border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] px-3 py-2.5">
        <span className="min-w-0 flex-1 text-[0.74rem] text-[var(--t3,#8a8f98)]">
          {t("pages.discover.adopt.applyHint")}
        </span>
        <Button size="sm" variant="default" onClick={onApply} disabled={applying || applyCount === 0}>
          {applying ? t("pages.discover.adopt.applying") : t("pages.discover.adopt.applySelected", { n: applyCount })}
        </Button>
      </div>
    </section>
  );
}
