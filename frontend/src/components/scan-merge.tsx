import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type {
  FieldMeta,
  ScanMergeItem,
  ScanPreviewOut,
  ScriptMergeItem,
  ServiceSpec,
} from "@/ipc/protocol";

/**
 * 扫描合并向导共享组件（1.1 scanPreview / 1.4 Taskfile 之后的 2.1 README 导入
 * 也走同一向导）。config 页与 discover 页共用，禁止各自维护一份近似 UI。
 * 2.1 增量：字段 provenance 徽标（scan/readme + 置信度 + 冲突双值）与脚本合并项。
 */

export type FieldChoice = "keep" | "update";

export function specFieldValue(spec: ServiceSpec | null | undefined, field: string): string {
  const { t } = useTranslation();
  if (!spec) return "—";
  const v = (spec as unknown as Record<string, unknown>)[field];
  if (v === undefined || v === null) return "—";
  if (typeof v === "string") return v === "" ? t("pages.config.emptyValue") : v;
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  try {
    return JSON.stringify(v);
  } catch {
    return "—";
  }
}

export const SCAN_GROUPS: { status: ScanMergeItem["status"]; titleKey: string }[] = [
  { status: "added", titleKey: "pages.config.groupAdded" },
  { status: "id_conflict", titleKey: "pages.config.groupConflict" },
  { status: "match_diff", titleKey: "pages.config.groupDiff" },
  { status: "match_same", titleKey: "pages.config.groupSame" },
  { status: "missing", titleKey: "pages.config.groupMissing" },
];

export function ScanStatusBadge({ status }: { status: ScanMergeItem["status"] }) {
  const { t } = useTranslation();
  if (status === "match_same") return <Badge variant="secondary">{t("pages.config.groupSame")}</Badge>;
  if (status === "match_diff") return <Badge variant="outline" className="border-[#f0d58a] bg-[#fdf6e3] text-[#B7791F]">{t("pages.config.groupDiff")}</Badge>;
  if (status === "missing") return <Badge variant="outline" className="border-red-200 bg-[var(--st-danger-tint,#fdecec)] text-[#DC2626]">{t("pages.config.groupMissing")}</Badge>;
  if (status === "id_conflict") return <Badge variant="outline" className="border-[#f0d58a] bg-[#fdf6e3] text-[#B7791F]">{t("pages.config.groupConflict")}</Badge>;
  return <Badge variant="soon">{t("pages.config.groupAdded")}</Badge>;
}

/** 2.1：字段来源徽标（spec §3.4 provenance）。冲突时 scan 值保留、README 值双值可见。 */
export function ProvenanceChips({ metas }: { metas?: FieldMeta[] | null }) {
  const { t } = useTranslation();
  if (!metas || metas.length === 0) return null;
  const confLabel = (c?: string | null) =>
    c ? ` · ${t(`scanMerge.conf.${c}`)}` : "";
  return (
    <div className="mt-1.5 flex flex-wrap gap-1.5">
      {metas.map((m) => (
        <span
          key={m.field}
          className="inline-flex items-center gap-1 rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f3f4f5)] px-1.5 py-0.5 text-[0.68rem] text-[var(--t2,#62666d)]"
        >
          <code className="font-mono font-semibold text-[var(--t1,#222326)]">{m.field}</code>
          {m.readme_value ? (
            <span>
              {t("scanMerge.scanKept")} · {t("scanMerge.readmeSuggest")}{" "}
              <code className="font-mono text-[var(--st-accent,#5e6ad2)]">{m.readme_value}</code>
            </span>
          ) : (
            <span>
              {m.source === "readme" ? t("scanMerge.fromReadme") : t("scanMerge.fromScan")}
              {m.source === "readme" ? confLabel(m.confidence) : ""}
            </span>
          )}
        </span>
      ))}
    </div>
  );
}

/** match_diff 的字段行：当前值 / 发现值并排小字 + 保留/采用切换。 */
export function DiffFieldRow({
  item,
  field,
  choice,
  onChoose,
}: {
  item: ScanMergeItem;
  field: string;
  choice: FieldChoice;
  onChoose: (c: FieldChoice) => void;
}) {
  const { t } = useTranslation();
  const cur = specFieldValue(item.current, field);
  const disc = specFieldValue(item.discovered, field);
  return (
    <div className="flex flex-wrap items-center gap-2 rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f3f4f5)] px-2 py-1.5">
      <code className="shrink-0 font-mono text-[0.72rem] font-semibold text-[var(--t1,#222326)]">{field}</code>
      <span className="min-w-0 flex-1 truncate font-mono text-[0.7rem] text-[var(--t2,#62666d)]" title={t("pages.config.currentTitle", { value: cur })}>
        {t("pages.config.currentShort")} {cur}
      </span>
      <span className="min-w-0 flex-1 truncate font-mono text-[0.7rem] text-[var(--st-accent,#5e6ad2)]" title={t("pages.config.discoveredTitle", { value: disc })}>
        {t("pages.config.discoveredShort")} {disc}
      </span>
      <span className="inline-flex shrink-0 items-center gap-0.5 rounded-[var(--r-sm,8px)] bg-[var(--surface,#fff)] p-0.5">
        <button
          type="button"
          aria-pressed={choice === "keep"}
          onClick={() => onChoose("keep")}
          className={cn(
            "rounded-[6px] px-2 py-0.5 text-[0.7rem] font-semibold transition-all duration-150",
            choice === "keep"
              ? "bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent-hover,#4f5ac8)]"
              : "text-[var(--t3,#8a8f98)] hover:text-[var(--t1,#222326)]",
          )}
        >
          {t("pages.config.keepCurrent")}
        </button>
        <button
          type="button"
          aria-pressed={choice === "update"}
          onClick={() => onChoose("update")}
          className={cn(
            "rounded-[6px] px-2 py-0.5 text-[0.7rem] font-semibold transition-all duration-150",
            choice === "update"
              ? "bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent-hover,#4f5ac8)]"
              : "text-[var(--t3,#8a8f98)] hover:text-[var(--t1,#222326)]",
          )}
        >
          {t("pages.config.useDiscovered")}
        </button>
      </span>
    </div>
  );
}

export function ScanItemRow({
  item,
  checked,
  onToggle,
  fieldChoices,
  onFieldChoice,
}: {
  item: ScanMergeItem;
  checked: boolean;
  onToggle: (v: boolean) => void;
  fieldChoices: Record<string, FieldChoice>;
  onFieldChoice: (field: string, c: FieldChoice) => void;
}) {
  const { t } = useTranslation();
  const kind = item.discovered?.kind ?? item.current?.kind ?? "";
  return (
    <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] p-2.5">
      <div className="flex flex-wrap items-center gap-2">
        {item.status === "added" || item.status === "id_conflict" ? (
          <label className="flex shrink-0 cursor-pointer items-center gap-1.5 text-[0.76rem] font-medium text-[var(--t1,#222326)]">
            <input type="checkbox" checked={checked} onChange={(e) => onToggle(e.target.checked)} />
            {item.status === "added" ? t("pages.config.addToYaml") : t("pages.config.addAsCandidate")}
          </label>
        ) : null}
        <span className="font-mono text-[0.82rem] font-semibold text-[var(--t1,#222326)]">{item.service_id}</span>
        {kind ? (
          <Badge variant="outline" className="text-[10px] uppercase">
            {kind}
          </Badge>
        ) : null}
        <ScanStatusBadge status={item.status} />
      </div>

      {item.status === "id_conflict" ? (
        <div className="mt-1.5 text-[0.74rem] text-[var(--t2,#62666d)]">
          {t("pages.config.idConflictDesc")}
          <code className="ml-1 rounded bg-[var(--surface-2,#f3f4f5)] px-1.5 py-0.5 font-mono text-[0.72rem]">
            {item.candidate_id ?? "—"}
          </code>
        </div>
      ) : null}

      {item.status === "match_diff" && item.field_diffs.length > 0 ? (
        <div className="mt-2 flex flex-col gap-1.5">
          {item.field_diffs.map((f) => (
            <DiffFieldRow
              key={f}
              item={item}
              field={f}
              choice={fieldChoices[f] ?? "keep"}
              onChoose={(c) => onFieldChoice(f, c)}
            />
          ))}
          <span className="text-[0.7rem] text-[var(--t3,#8a8f98)]">
            {t("pages.config.updateScopeHint")}
          </span>
        </div>
      ) : null}

      <ProvenanceChips metas={item.fields_meta} />

      {item.status === "missing" ? (
        <div className="mt-1.5 text-[0.74rem] text-[#B7791F]">
          {t("pages.config.missingDesc")}
        </div>
      ) : null}
    </div>
  );
}

/** 2.1：脚本合并项（cmds 只来自文档，写入前人确认）。 */
export function ScriptItemRow({
  item,
  checked,
  onToggle,
}: {
  item: ScriptMergeItem;
  checked: boolean;
  onToggle: (v: boolean) => void;
}) {
  const { t } = useTranslation();
  const cmds = item.discovered?.cmds ?? [];
  return (
    <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] p-2.5">
      <div className="flex flex-wrap items-center gap-2">
        {item.status === "added" ? (
          <label className="flex shrink-0 cursor-pointer items-center gap-1.5 text-[0.76rem] font-medium text-[var(--t1,#222326)]">
            <input type="checkbox" checked={checked} onChange={(e) => onToggle(e.target.checked)} />
            {t("pages.config.addToYaml")}
          </label>
        ) : null}
        <span className="font-mono text-[0.82rem] font-semibold text-[var(--t1,#222326)]">{item.script_id}</span>
        <ScanStatusBadge status={item.status} />
      </div>
      {cmds.length > 0 ? (
        <div className="mt-1.5 flex flex-col gap-1">
          {cmds.map((c, i) => (
            <code key={i} className="break-all rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f3f4f5)] px-2 py-1 font-mono text-[0.72rem] text-[var(--t1,#222326)]">
              {c}
            </code>
          ))}
        </div>
      ) : null}
      <ProvenanceChips metas={item.fields_meta} />
    </div>
  );
}

export function ScanPreviewPanel({
  preview,
  titleKey = "pages.config.rescanTitle",
  ariaKey = "pages.config.rescanAria",
  headerExtra,
  scriptItems,
  scriptChecked,
  onToggleScript,
  addChecked,
  onToggleAdd,
  onSelectAllAddable,
  fieldChoices,
  onFieldChoice,
  applying,
  applyCount,
  onApply,
  onClose,
}: {
  preview: ScanPreviewOut;
  /** 面板标题/aria 的 i18n key（config 复用默认，README 导入传 scanMerge.*） */
  titleKey?: string;
  ariaKey?: string;
  headerExtra?: React.ReactNode;
  /** 2.1：脚本合并项（传入才渲染脚本分组） */
  scriptItems?: ScriptMergeItem[];
  scriptChecked?: Record<string, boolean>;
  onToggleScript?: (id: string, v: boolean) => void;
  addChecked: Record<string, boolean>;
  onToggleAdd: (id: string, v: boolean) => void;
  onSelectAllAddable: (v: boolean) => void;
  fieldChoices: Record<string, Record<string, FieldChoice>>;
  onFieldChoice: (id: string, field: string, c: FieldChoice) => void;
  applying: boolean;
  applyCount: number;
  onApply: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const addable = preview.items.filter((it) => it.status === "added" || it.status === "id_conflict");
  const allAddableChecked =
    addable.length > 0 && addable.every((it) => addChecked[it.service_id]);

  return (
    <section
      className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[var(--r-lg,16px)] border border-[rgb(94_106_210_/_0.35)] bg-[var(--surface,#fff)] shadow-[var(--shadow-2,0_6px_20px_rgb(16_24_40_/_0.09))]"
      aria-label={t(ariaKey)}
    >
      <div className="flex shrink-0 flex-wrap items-center gap-2 rounded-t-[var(--r-lg,16px)] border-b border-[var(--line,#e6e6e6)] bg-[var(--st-accent-tint,#eef0fb)] px-3 py-2.5">
        <h3 className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">{t(titleKey)}</h3>
        <span className="font-mono text-[0.7rem] text-[var(--t3,#8a8f98)]">{t("pages.config.itemCount", { n: preview.items.length })}</span>
        {headerExtra}
        {addable.length > 0 ? (
          <Button size="sm" variant="outline" onClick={() => onSelectAllAddable(!allAddableChecked)}>
            {allAddableChecked ? t("pages.config.unselectAllAdded") : t("pages.config.selectAllAdded", { n: addable.length })}
          </Button>
        ) : null}
        <Button variant="outline" size="sm" className="ml-auto" onClick={onClose}>
          {t("common.close")}
        </Button>
      </div>

      {preview.warnings.length > 0 ? (
        <div className="mx-3 mt-2 rounded-[var(--r-sm,8px)] border border-[#f0d58a] bg-[#fdf6e3] px-2.5 py-1.5 text-[0.74rem] leading-relaxed text-[#B7791F]" role="alert">
          {preview.warnings.map((w, i) => (
            <div key={i}>{w}</div>
          ))}
        </div>
      ) : null}

      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-2">
        <div className="flex flex-col gap-3">
          {scriptItems && scriptItems.length > 0 ? (
            <div>
              <div className="mb-1.5 flex items-center gap-2 px-0.5">
                <span className="text-[0.72rem] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">{t("scanMerge.scriptGroup")}</span>
                <span className="font-mono text-[0.7rem] text-[var(--t3,#8a8f98)]">{scriptItems.length}</span>
              </div>
              <div className="flex flex-col gap-1.5">
                {scriptItems.map((it) => (
                  <ScriptItemRow
                    key={it.script_id}
                    item={it}
                    checked={scriptChecked?.[it.script_id] ?? false}
                    onToggle={(v) => onToggleScript?.(it.script_id, v)}
                  />
                ))}
              </div>
            </div>
          ) : null}
          {SCAN_GROUPS.map(({ status, titleKey: groupKey }) => {
            const items = preview.items.filter((it) => it.status === status);
            if (items.length === 0) return null;
            return (
              <div key={status}>
                <div className="mb-1.5 flex items-center gap-2 px-0.5">
                  <span className="text-[0.72rem] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">{t(groupKey)}</span>
                  <span className="font-mono text-[0.7rem] text-[var(--t3,#8a8f98)]">{items.length}</span>
                </div>
                <div className="flex flex-col gap-1.5">
                  {items.map((it) => (
                    <ScanItemRow
                      key={it.service_id}
                      item={it}
                      checked={addChecked[it.service_id] ?? false}
                      onToggle={(v) => onToggleAdd(it.service_id, v)}
                      fieldChoices={fieldChoices[it.service_id] ?? {}}
                      onFieldChoice={(f, c) => onFieldChoice(it.service_id, f, c)}
                    />
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      </div>

      <div className="flex shrink-0 items-center gap-2 rounded-b-[var(--r-lg,16px)] border-t border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] px-3 py-2.5">
        <span className="min-w-0 flex-1 text-[0.74rem] text-[var(--t3,#8a8f98)]">
          {t("pages.config.applyHint")}
        </span>
        <Button size="sm" variant="default" onClick={onApply} disabled={applying || applyCount === 0}>
          {applying ? t("pages.config.applying") : t("pages.config.applySelected", { n: applyCount })}
        </Button>
      </div>
    </section>
  );
}
