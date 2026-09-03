import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, CheckCircle2, FolderOpen, Laptop, Settings2, Shapes } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { CloudMigratePlanOut } from "../ipc/protocol";
import { useTranslation } from "react-i18next";

export function CloudMigrationDialog(props: {
  plan: CloudMigratePlanOut | null;
  busy: boolean;
  includeTemplates: boolean;
  includeSettings: boolean;
  workspaceDirs: Record<string, string>;
  onIncludeTemplates: (value: boolean) => void;
  onIncludeSettings: (value: boolean) => void;
  onWorkspaceDir: (id: string, dir: string) => void;
  onApply: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const entities = props.plan?.entities ?? [];
  const workspaces = entities.filter((entity) => entity.type === "workspace");
  const templates = entities.filter((entity) => entity.type === "template");
  const settings = entities.filter((entity) => entity.type === "settings");
  const mapped = workspaces.filter((entity) => props.workspaceDirs[entity.id]?.trim()).length;
  const canApply = !!props.plan && !props.busy && mapped === workspaces.length;

  const chooseDirectory = async (id: string) => {
    const selected = await openDialog({ directory: true, multiple: false });
    if (typeof selected === "string" && selected.trim()) props.onWorkspaceDir(id, selected);
  };

  return (
    <Dialog open={props.plan != null} onOpenChange={(open) => (!open ? props.onClose() : undefined)}>
      {props.plan ? (
        <DialogContent className="max-h-[min(46rem,calc(100vh-3rem))] gap-0 overflow-hidden p-0 sm:max-w-2xl">
          <DialogHeader className="gap-1 border-b border-[var(--line)] px-5 py-4">
            <DialogTitle className="flex items-center gap-2">
              <Laptop className="size-4 text-[var(--st-accent)]" />
              {t("pages.cloud.migrateTitle")}
            </DialogTitle>
            <DialogDescription className="text-[0.75rem]">{t("pages.cloud.migrationDialogHint")}</DialogDescription>
          </DialogHeader>

          <div className="min-h-0 overflow-y-auto px-5 py-4">
            <div className="grid grid-cols-3 gap-2">
              <Summary icon={<Laptop className="size-3.5" />} label={t("pages.cloud.workspaces")} value={workspaces.length} />
              <Summary icon={<Shapes className="size-3.5" />} label={t("pages.cloud.templates")} value={templates.length} />
              <Summary icon={<Settings2 className="size-3.5" />} label={t("pages.cloud.settingsEntity")} value={settings.length} />
            </div>

            {workspaces.length ? (
              <section className="mt-5">
                <div className="mb-2 flex items-center justify-between gap-2">
                  <h4 className="text-[0.72rem] font-semibold uppercase tracking-wider text-[var(--t3)]">
                    {t("pages.cloud.workspaceDestinations")}
                  </h4>
                  <Badge variant={mapped === workspaces.length ? "default" : "outline"}>
                    {mapped}/{workspaces.length}
                  </Badge>
                </div>
                <div className="flex flex-col gap-2">
                  {workspaces.map((entity) => {
                    const dir = props.workspaceDirs[entity.id] ?? "";
                    return (
                      <div key={entity.id} className="rounded-[var(--r-sm)] border border-[var(--line-strong)] p-3">
                        <div className="flex items-center gap-2">
                          <span className="min-w-0 flex-1 truncate text-[0.78rem] font-semibold text-[var(--t1)]">
                            {entity.name ?? entity.id}
                          </span>
                          {dir ? <CheckCircle2 className="size-3.5 text-[var(--st-ok)]" /> : null}
                        </div>
                        <div className="mt-2 flex items-center gap-2">
                          <code className="min-w-0 flex-1 truncate rounded bg-[var(--surface-2)] px-2 py-1.5 text-[0.68rem] text-[var(--t3)]" title={dir}>
                            {dir || t("pages.cloud.destinationRequired")}
                          </code>
                          <Button variant="outline" size="sm" className="shrink-0 gap-1" onClick={() => void chooseDirectory(entity.id)}>
                            <FolderOpen className="size-3.5" /> {t("pages.cloud.chooseFolder")}
                          </Button>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </section>
            ) : null}

            <section className="mt-5 rounded-[var(--r-sm)] border border-[var(--line)] p-3">
              <h4 className="mb-2 text-[0.72rem] font-semibold uppercase tracking-wider text-[var(--t3)]">
                {t("pages.cloud.otherData")}
              </h4>
              <div className="flex flex-col gap-2 text-[0.78rem] text-[var(--t2)]">
                <label className="flex cursor-pointer items-center justify-between gap-3">
                  <span>{t("pages.cloud.includeTemplates")} ({templates.length})</span>
                  <input type="checkbox" checked={props.includeTemplates} onChange={(event) => props.onIncludeTemplates(event.target.checked)} />
                </label>
                <label className="flex cursor-pointer items-center justify-between gap-3">
                  <span>{t("pages.cloud.includeSettings")} ({settings.length})</span>
                  <input type="checkbox" checked={props.includeSettings} onChange={(event) => props.onIncludeSettings(event.target.checked)} />
                </label>
              </div>
            </section>

            {props.plan.toolchain_gaps.length ? (
              <section className="mt-4 rounded-[var(--r-sm)] border border-[var(--st-warn)]/30 bg-[var(--st-warn-tint)] p-3">
                <h4 className="flex items-center gap-1.5 text-[0.75rem] font-semibold text-[var(--st-warn)]">
                  <AlertTriangle className="size-3.5" />
                  {t("pages.cloud.toolchainGaps", { n: props.plan.toolchain_gaps.length })}
                </h4>
                <ul className="mt-2 flex flex-col gap-1 font-mono text-[0.68rem] text-[var(--t2)]">
                  {props.plan.toolchain_gaps.map((gap, index) => (
                    <li key={`${gap.tool}-${index}`}>{gap.tool} · {gap.required ?? gap.version ?? "—"} · {gap.status}</li>
                  ))}
                </ul>
              </section>
            ) : null}
          </div>

          <DialogFooter className="mx-0 mb-0 items-center px-5 sm:justify-between">
            <span className="text-[0.7rem] text-[var(--t3)]">
              {workspaces.length && mapped < workspaces.length ? t("pages.cloud.mapAllWorkspaces") : t("pages.cloud.noServiceStart")}
            </span>
            <span className="flex gap-2">
              <Button variant="outline" size="sm" onClick={props.onClose} disabled={props.busy}>{t("common.cancel")}</Button>
              <Button variant="default" size="sm" onClick={props.onApply} disabled={!canApply}>
                {props.busy ? t("common.loading") : t("pages.cloud.applyMigration")}
              </Button>
            </span>
          </DialogFooter>
        </DialogContent>
      ) : null}
    </Dialog>
  );
}

function Summary(props: { icon: React.ReactNode; label: string; value: number }) {
  return (
    <div className="rounded-[var(--r-sm)] border border-[var(--line)] bg-[var(--surface-2)] px-3 py-2">
      <span className="flex items-center gap-1.5 text-[0.68rem] text-[var(--t3)]">{props.icon}{props.label}</span>
      <strong className="mt-1 block font-mono text-base text-[var(--t1)]">{props.value}</strong>
    </div>
  );
}
