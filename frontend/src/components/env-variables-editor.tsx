import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Settings2, Trash2, Upload } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { parseEnvImport, type EnvImportFormat } from "@/lib/env-import";
import { useToast } from "@/components/ui/toast";
import { cn } from "@/lib/utils";

export type EnvRow = { id: string; key: string; value: string };

export function envRowsFromRecord(env: Record<string, string>): EnvRow[] {
  return Object.entries(env).map(([key, value], index) => ({
    id: `${index}-${key}`,
    key,
    value,
  }));
}

export function envRecordFromRows(rows: EnvRow[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const row of rows) {
    const key = row.key.trim();
    if (!key) continue;
    out[key] = row.value;
  }
  return out;
}

type EnvVariablesEditorProps = {
  value: Record<string, string>;
  onChange: (next: Record<string, string>) => void;
  /** 提供时在底部显示保存按钮（运行页环境 Tab） */
  onSave?: () => void | Promise<void>;
  saveDisabled?: boolean;
  /** 保存按钮文案；缺省用 `pages.env.saveEnvVars` 的本地化文案 */
  saveLabel?: string;
  /** 隐藏区块标题（嵌入配置页卡片时） */
  hideTitle?: boolean;
  className?: string;
};

export function EnvVariablesEditor({
  value,
  onChange,
  onSave,
  saveDisabled,
  saveLabel,
  hideTitle,
  className,
}: EnvVariablesEditorProps) {
  const { toast } = useToast();
  const { t } = useTranslation();
  const [rows, setRows] = useState<EnvRow[]>(() => envRowsFromRecord(value));
  const [importText, setImportText] = useState("");
  const [importFormat, setImportFormat] = useState<EnvImportFormat>("auto");
  const [showImport, setShowImport] = useState(false);

  useEffect(() => {
    setRows(envRowsFromRecord(value));
  }, [value]);

  const emit = (nextRows: EnvRow[]) => {
    setRows(nextRows);
    onChange(envRecordFromRows(nextRows));
  };

  const addRow = () => {
    emit([...rows, { id: `new-${Date.now()}`, key: "", value: "" }]);
  };

  const removeRow = (rowId: string) => {
    emit(rows.filter((r) => r.id !== rowId));
  };

  const updateRow = (rowId: string, patch: Partial<Pick<EnvRow, "key" | "value">>) => {
    emit(rows.map((r) => (r.id === rowId ? { ...r, ...patch } : r)));
  };

  const importEnv = () => {
    const parsed = parseEnvImport(importText, importFormat);
    const keys = Object.keys(parsed);
    if (!keys.length) {
      toast(t("operations.envParseEmpty"), "warn");
      return;
    }
    const map = new Map(rows.map((r) => [r.key.trim(), r]));
    for (const [k, v] of Object.entries(parsed)) {
      map.set(k, { id: `import-${k}-${Date.now()}`, key: k, value: v });
    }
    emit([...map.values()]);
    setImportText("");
    setShowImport(false);
    toast(onSave ? t("operations.importEnvOkWithSave", { n: keys.length }) : t("operations.importEnvOk", { n: keys.length }), "ok");
  };

  return (
    <div className={cn("flex flex-col gap-3", className)}>
      <div className="flex flex-wrap items-center gap-2">
        {!hideTitle ? (
          <div className="flex items-center gap-2 text-[0.72rem] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
            <Settings2 className="size-3.5" /> {t("env.title")}
            <span className="rounded-full bg-[var(--surface-2,#f3f4f5)] px-1.5 font-mono text-[10px] normal-case">{rows.length}</span>
          </div>
        ) : null}
        <Button
          size="sm"
          variant="soft"
          className={cn("gap-1", hideTitle ? "" : "ml-auto")}
          onClick={() => setShowImport((v) => !v)}
        >
          <Upload className="size-3.5" /> {t("env.quickImport")}
        </Button>
      </div>

      {showImport ? (
        <div className="flex flex-col gap-2 rounded-lg border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] p-3">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[0.72rem] text-[var(--t2,#62666d)]">{t("env.format")}</span>
            {(["auto", "env", "yaml", "properties", "json"] as const).map((fmt) => (
              <button
                key={fmt}
                type="button"
                onClick={() => setImportFormat(fmt)}
                className={cn(
                  "cursor-pointer rounded-[var(--r-sm,8px)] border px-2 py-0.5 font-mono text-[0.68rem] transition-colors",
                  importFormat === fmt
                    ? "border-[var(--st-accent,#5e6ad2)] bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent,#5e6ad2)]"
                    : "border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] text-[var(--t2,#62666d)] hover:border-[var(--t3,#8a8f98)] hover:bg-[var(--surface,#fff)]",
                )}
              >
                {fmt}
              </button>
            ))}
          </div>
          <textarea
            value={importText}
            onChange={(e) => setImportText(e.target.value)}
            placeholder={t("env.importPlaceholder")}
            className="min-h-[6.5rem] w-full resize-y rounded-md border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] px-3 py-2 font-mono text-[12px] text-[var(--t1,#222326)] outline-none focus:border-[var(--st-accent,#5e6ad2)]"
          />
          <div className="flex gap-2">
            <Button size="sm" variant="success" onClick={importEnv} disabled={!importText.trim()}>
              {t("env.parseAndMerge")}
            </Button>
            <Button size="sm" variant="outline" onClick={() => setShowImport(false)}>
              {t("common.collapse")}
            </Button>
          </div>
        </div>
      ) : null}

      {rows.length === 0 ? (
        <div className="text-sm text-[var(--t3,#8a8f98)]">{t("env.noVars")}</div>
      ) : (
        <div className="flex flex-col gap-1.5">
          {rows.map((row) => (
            <div key={row.id} className="grid grid-cols-[1fr_1fr_auto] items-center gap-2">
              <Input
                value={row.key}
                onChange={(e) => updateRow(row.id, { key: e.target.value })}
                placeholder="KEY"
                className="font-mono text-[12px]"
                aria-label={t("env.varNameAria")}
              />
              <Input
                value={row.value}
                onChange={(e) => updateRow(row.id, { value: e.target.value })}
                placeholder={t("env.valuePlaceholder")}
                className="font-mono text-[12px]"
                aria-label={t("env.varValueAria")}
              />
              <button
                type="button"
                onClick={() => removeRow(row.id)}
                className="grid size-8 cursor-pointer place-items-center rounded-[var(--r-sm,8px)] text-[var(--t3,#8a8f98)] transition-colors hover:bg-[var(--st-danger-tint,#fdecec)] hover:text-[var(--st-danger,#dc2626)]"
                title={t("env.deleteVar")}
              >
                <Trash2 className="size-3.5" />
              </button>
            </div>
          ))}
        </div>
      )}

      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" variant="outline" className="gap-1" onClick={addRow}>
          <Plus className="size-3.5" /> {t("env.addVar")}
        </Button>
        {onSave ? (
          <Button size="sm" variant="success" onClick={() => void onSave()} disabled={saveDisabled}>
            {saveLabel ?? t("env.saveEnvVars")}
          </Button>
        ) : null}
      </div>
    </div>
  );
}
