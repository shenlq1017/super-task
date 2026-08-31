import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  ChevronRight,
  FileInput,
  FileText,
  KeyRound,
  Layers,
  RefreshCw,
  SlidersHorizontal,
  Sparkles,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input, Textarea } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { EnvVariablesEditor } from "@/components/env-variables-editor";
import { TaskfileImportPanel } from "@/components/taskfile-import-panel";
import { ScanPreviewPanel, type FieldChoice } from "@/components/scan-merge";
import { cn } from "@/lib/utils";
import { useYaml } from "@/providers/yaml-provider";
import { useWorkspace } from "@/providers/workspace-provider";
import { useToast } from "@/components/ui/toast";
import { useUnsavedEntry, useUnsavedGuard } from "@/providers/unsaved-guard";
import { AiOutputBody } from "@/components/ai-output-panel";
import {
  apiScanApply,
  apiScanPreview,
  apiTaskfileApply,
  apiTaskfilePreview,
  apiYamlGet,
  apiAiComplete,
  apiProfilesList,
  apiProfilesActivate,
  apiSecretsStatus,
  apiSecretsValidate,
  apiSecretsSet,
  apiSecretsDelete,
} from "../ipc/api";
import { IpcFailure } from "../ipc/protocol";
import type {
  MergeChoice,
  ScanPreviewOut,
  SuperTaskFile,
  TaskfilePreviewOut,
} from "@/ipc/protocol";
import { opErrorLabel } from "@/lib/status";
import { formatIpcFailure } from "@/lib/error-messages";

function detectCycle(spec: SuperTaskFile): string[] | null {
  const deps: Record<string, string[]> = {};
  for (const [id, s] of Object.entries(spec.services)) deps[id] = s.depends_on ?? [];
  const GRAY = 1,
    BLACK = 2;
  const color: Record<string, number> = {};
  const stack: string[] = [];
  const dfs = (u: string): string[] | null => {
    color[u] = GRAY;
    stack.push(u);
    for (const v of deps[u] ?? []) {
      if (!(v in color)) {
        const r = dfs(v);
        if (r) return r;
      } else if (color[v] === GRAY) {
        const idx = stack.indexOf(v);
        return stack.slice(idx).concat(v);
      }
    }
    color[u] = BLACK;
    stack.pop();
    return null;
  };
  for (const id of Object.keys(deps)) {
    if (!(id in color)) {
      const r = dfs(id);
      if (r) return r;
    }
  }
  return null;
}

function V12ConfigPanel() {
  const ws = useWorkspace();
  const yaml = useYaml();
  const { toast } = useToast();
  const { t } = useTranslation();
  const wid = ws.state.workspaceId;
  const [profiles, setProfiles] = useState<import("@/ipc/protocol").ProfilesListOut | null>(null);
  const [secrets, setSecrets] = useState<import("@/ipc/protocol").SecretsStatusOut | null>(null);
  const [secretKey, setSecretKey] = useState("");
  const [secretValue, setSecretValue] = useState("");
  const [deleteKey, setDeleteKey] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const reload = async () => {
    if (!wid) return;
    setLoading(true);
    try {
      const [p, s] = await Promise.all([apiProfilesList(wid), apiSecretsStatus(wid)]);
      setProfiles(p);
      setSecrets(s);
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { void reload(); }, [wid]);

  const activate = async (id: string) => {
    if (!wid || !yaml.state.hash) {
      toast(t("pages.config.profileNotReady"), "warn");
      return;
    }
    if (profiles?.active === id) return;
    try {
      await apiProfilesActivate(wid, id, yaml.state.hash);
      await Promise.all([yaml.actions.reload(), ws.actions.refreshSpec()]);
      await reload();
      toast(t("pages.config.profileSwitched", { id }), "ok");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  const saveSecret = async () => {
    if (!wid || !secretKey.trim()) return;
    try {
      await apiSecretsSet(wid, secretKey.trim(), secretValue);
      setSecretValue("");
      await reload();
      toast(t("pages.config.secretSaved", { key: secretKey.trim() }), "ok");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  const deleteSecret = async (key: string) => {
    if (!wid) return;
    try {
      await apiSecretsDelete(wid, key);
      await reload();
      toast(t("pages.config.secretDeleted", { key }), "ok");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  const validateSecrets = async () => {
    if (!wid) return;
    try {
      const out = await apiSecretsValidate(wid);
      toast(out.ok ? t("pages.config.secretsOk") : t("pages.config.secretsMissing", { keys: out.missing.join(", ") }), out.ok ? "ok" : "warn");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  const secretCount = secrets?.keys.length ?? 0;
  const activeProfile = profiles?.active ?? "default";
  const profileOptions = useMemo(() => {
    const items = profiles?.profiles ?? [];
    const ids = new Set<string>(["default", ...items.map((p) => p.id)]);
    return [...ids].map((id) => {
      const meta = items.find((p) => p.id === id);
      const suffix = meta?.enabled_count != null ? ` · ${meta.enabled_count}` : "";
      return { id, label: id === "default" ? `${t("pages.config.defaultImplicit", { id })}${suffix}` : `${id}${suffix}` };
    });
  }, [profiles, t]);
  const canSwitchProfile = (profiles?.profiles.length ?? 0) > 0;

  return (
    <section className="shrink-0 border-b border-[var(--line,#e6e6e6)] bg-[var(--bg,#f7f8f8)] px-4 py-3">
      <div className="grid gap-3 lg:grid-cols-2">
        <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] p-3">
          <div className="mb-2 flex items-center gap-2">
            <Layers className="size-3.5 text-[var(--st-accent,#5e6ad2)]" />
            <h3 className="text-[0.8rem] font-semibold text-[var(--t1,#222326)]">Profile</h3>
            <Badge variant="secondary" className="text-[10px]">env / enabled / port</Badge>
            <Button className="ml-auto" variant="soft" size="sm" onClick={() => void reload()} disabled={!wid || loading}>
              <RefreshCw className={cn("size-3.5", loading && "animate-spin")} /> {t("common.refresh")}
            </Button>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            {canSwitchProfile ? (
              <Select
                value={activeProfile}
                onValueChange={(id) => void activate(id)}
                disabled={!wid || loading || !profiles}
              >
                <SelectTrigger
                  size="sm"
                  className="h-8 min-w-[12rem] flex-1 cursor-pointer border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] text-xs"
                  aria-label={t("pages.config.currentProfileAria")}
                >
                  <SelectValue placeholder={t("pages.config.pickProfile")} />
                </SelectTrigger>
                <SelectContent>
                  {profileOptions.map((p) => (
                    <SelectItem key={p.id} value={p.id} className="cursor-pointer text-xs">
                      {p.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : (
              <div className="flex min-h-8 flex-1 items-center gap-2">
                <Badge variant="outline" className="font-mono text-xs">{activeProfile}</Badge>
                <span className="text-[0.7rem] text-[var(--t3,#8a8f98)]">{t("pages.config.noExtraProfiles")}</span>
              </div>
            )}
          </div>
          <p className="mt-2 text-[0.7rem] leading-relaxed text-[var(--t3,#8a8f98)]">
            {t("pages.config.profileSwitchHint")}
            {!yaml.state.hash ? ` ${t("pages.config.profileLoading")}` : null}
          </p>
        </div>

        <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] p-3">
          <div className="mb-2 flex items-center gap-2">
            <KeyRound className="size-3.5 text-[var(--st-accent,#5e6ad2)]" />
            <h3 className="text-[0.8rem] font-semibold text-[var(--t1,#222326)]">Secrets</h3>
            {secrets?.file ? <Badge variant="outline" className="max-w-[10rem] truncate" title={secrets.file}>{secrets.file}</Badge> : <Badge variant="outline">{t("pages.config.userEnv")}</Badge>}
            <Badge variant="secondary" className="font-mono text-[10px]">{secretCount}</Badge>
            <Button className="ml-auto" variant="soft" size="sm" onClick={() => void validateSecrets()} disabled={!wid}>{t("pages.config.validateRequired")}</Button>
          </div>

          <div className="flex max-h-[10rem] flex-col gap-1 overflow-y-auto">
            {secrets?.keys.length ? secrets.keys.map((item) => (
              <div
                key={item.key}
                className="grid grid-cols-[1fr_auto_auto_auto] items-center gap-2 rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] px-2 py-1.5 text-xs"
              >
                <code className="min-w-0 truncate font-mono" title={item.key}>{item.key}</code>
                <Badge variant={item.present ? "default" : "outline"}>{item.present ? t("pages.config.present") : t("pages.config.absent")}</Badge>
                {item.git_tracked ? <Badge variant="outline" className="border-red-200 text-red-600">Git</Badge> : <span />}
                <button
                  type="button"
                  onClick={() => setDeleteKey(item.key)}
                  className="grid size-7 cursor-pointer place-items-center rounded-[var(--r-sm,8px)] text-[var(--t3,#8a8f98)] transition-colors hover:bg-[var(--st-danger-tint,#fdecec)] hover:text-[var(--st-danger,#dc2626)]"
                  title={t("pages.config.deleteSecretTitle", { key: item.key })}
                >
                  <Trash2 className="size-3.5" />
                </button>
              </div>
            )) : (
              <p className="py-1 text-xs text-[var(--t3,#8a8f98)]">{t("pages.config.noSecrets")}</p>
            )}
          </div>

          <div className="mt-2 grid grid-cols-1 gap-2 sm:grid-cols-[1fr_1fr_auto]">
            <Input
              className="h-8 font-mono text-xs"
              placeholder="KEY_NAME"
              value={secretKey}
              onChange={(e) => setSecretKey(e.target.value)}
              aria-label="secret key"
            />
            <Input
              className="h-8 text-xs"
              type="password"
              placeholder={t("pages.config.secretValuePlaceholder")}
              value={secretValue}
              onChange={(e) => setSecretValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && secretKey.trim()) void saveSecret();
              }}
              aria-label="secret value"
            />
            <Button size="sm" variant="success" onClick={() => void saveSecret()} disabled={!wid || !secretKey.trim()}>
              {t("common.save")}
            </Button>
          </div>
        </div>
      </div>

      <ConfirmDialog
        open={deleteKey != null}
        title={t("pages.config.deleteSecretHeading")}
        description={deleteKey ? t("pages.config.deleteSecretDesc", { key: deleteKey }) : undefined}
        confirmText={t("common.delete")}
        destructive
        onConfirm={() => {
          if (deleteKey) void deleteSecret(deleteKey);
          setDeleteKey(null);
        }}
        onCancel={() => setDeleteKey(null)}
      />
    </section>
  );
}

function FormTab() {
  const ws = useWorkspace();
  const yaml = useYaml();
  const { toast } = useToast();
  const { t } = useTranslation();
  const spec = ws.state.spec;
  const [draft, setDraft] = useState<SuperTaskFile | null>(spec);
  const [saving, setSaving] = useState(false);
  const serviceIds = useMemo(() => (draft ? Object.keys(draft.services) : []), [draft]);
  const [openIds, setOpenIds] = useState<Set<string>>(() => new Set(serviceIds.slice(0, 1)));
  const [wsEnvOpen, setWsEnvOpen] = useState(false);

  useEffect(() => {
    setDraft(spec);
  }, [spec]);

  useEffect(() => {
    setOpenIds((prev) => {
      if (prev.size > 0) return prev;
      return new Set(serviceIds.slice(0, 1));
    });
  }, [serviceIds]);

  const save = async (): Promise<boolean> => {
    if (!draft) return false;
    const cycle = detectCycle(draft);
    if (cycle) {
      toast(t("pages.config.cycleDetected", { cycle: cycle.join(" → ") }), "err");
      return false;
    }
    setSaving(true);
    const ok = await yaml.actions.saveForm(draft);
    setSaving(false);
    if (ok) toast(t("pages.config.saved"), "ok");
    else toast(yaml.state.error ?? t("pages.config.saveFailed"), "err");
    return ok;
  };

  // 未保存守卫：表单草稿与 spec 有差异即视为脏
  useUnsavedEntry(
    "config.form",
    () => !!draft && !!spec && JSON.stringify(draft) !== JSON.stringify(spec),
    () => save(),
  );

  if (!spec || !draft) return <div className="p-4 text-[0.875rem] text-[var(--t3,#8a8f98)]">{t("pages.config.noSpec")}</div>;

  const setSvc = (id: string, patch: Partial<SuperTaskFile["services"][string]>) =>
    setDraft((d) => (d ? { ...d, services: { ...d.services, [id]: { ...d.services[id], ...patch } } } : d));

  const setEnv = (id: string, env: Record<string, string>) => setSvc(id, { env });
  const setWsEnv = (env: Record<string, string>) => setDraft((d) => (d ? { ...d, env } : d));

  const toggleOpen = (id: string) =>
    setOpenIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const wsEnvCount = Object.keys(draft.env).length;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="sticky top-0 z-10 flex items-center gap-2 border-b border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] px-4 py-2">
        <Button size="sm" variant="success" onClick={save} disabled={saving}>
          {saving ? t("pages.config.saving") : t("pages.config.saveForm")}
        </Button>
        <span className="font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]">
          {t("pages.config.serviceCount", { n: serviceIds.length })} · {t("pages.config.wsVarCount", { n: wsEnvCount })}
        </span>
        {yaml.state.warnings.length ? (
          <span className="text-[0.75rem] text-[var(--st-warn,#9a6700)]">{t("pages.config.warningCount", { n: yaml.state.warnings.length })}</span>
        ) : null}
        <div className="ml-auto flex gap-1">
          <Button size="sm" variant="outline" onClick={() => setOpenIds(new Set(serviceIds))}>{t("pages.config.expandAll")}</Button>
          <Button size="sm" variant="outline" onClick={() => setOpenIds(new Set())}>{t("pages.config.collapseAll")}</Button>
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-4">
        <section className="mb-4 rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)]">
          <button
            type="button"
            className="flex w-full cursor-pointer items-center gap-2 px-3 py-2.5 text-left transition-colors hover:bg-[var(--surface-2,#f3f4f5)]"
            onClick={() => setWsEnvOpen((v) => !v)}
            aria-expanded={wsEnvOpen}
          >
            {wsEnvOpen ? <ChevronDown className="size-4 text-[var(--t3,#8a8f98)]" /> : <ChevronRight className="size-4 text-[var(--t3,#8a8f98)]" />}
            <span className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">{t("pages.config.wsEnv")}</span>
            <Badge variant="secondary" className="font-mono text-[10px]">{wsEnvCount}</Badge>
          </button>
          {wsEnvOpen ? (
            <div className="border-t border-[var(--line,#e6e6e6)] px-3 py-3">
              <EnvVariablesEditor value={draft.env} onChange={setWsEnv} hideTitle />
            </div>
          ) : null}
        </section>

        <section className="flex flex-col gap-2">
          <div className="flex items-center gap-2 px-0.5">
            <h3 className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">{t("pages.config.services")}</h3>
            <span className="font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]">{serviceIds.length}</span>
          </div>
          {Object.entries(draft.services).map(([id, s]) => {
            const open = openIds.has(id);
            const envN = Object.keys(s.env ?? {}).length;
            return (
              <div key={id} className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] shadow-[var(--shadow-1,0_1px_2px_rgb(16_24_40_/_0.05))]">
                <div className="flex flex-wrap items-center gap-2 px-3 py-2">
                  <button
                    type="button"
                    className="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5 text-left"
                    onClick={() => toggleOpen(id)}
                    aria-expanded={open}
                  >
                    {open ? <ChevronDown className="size-4 shrink-0 text-[var(--t3,#8a8f98)]" /> : <ChevronRight className="size-4 shrink-0 text-[var(--t3,#8a8f98)]" />}
                    <span className="truncate font-semibold text-[var(--t1,#222326)]">{id}</span>
                    <Badge variant="outline" className="shrink-0 text-[10px] uppercase">{s.kind}</Badge>
                    {s.build_tool === "gradle" ? <Badge variant="outline" className="shrink-0 text-[10px] uppercase">gradle</Badge> : null}
                    {s.port != null ? <span className="shrink-0 font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]">{s.port}</span> : null}
                    <span className="shrink-0 font-mono text-[0.68rem] text-[var(--t3,#8a8f98)]">{envN} env</span>
                  </button>
                  <label className="flex shrink-0 cursor-pointer items-center gap-1.5 text-[0.75rem] text-[var(--t2,#62666d)]">
                    <input
                      type="checkbox"
                      checked={s.enabled}
                      onChange={(e) => setSvc(id, { enabled: e.target.checked })}
                    />
                    {t("pages.config.enabled")}
                  </label>
                </div>
                {open ? (
                  <div className="space-y-3 border-t border-[var(--line,#e6e6e6)] px-3 py-3">
                    <div className="grid grid-cols-1 gap-3 sm:grid-cols-[7.5rem_1fr]">
                      <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
                        {t("pages.config.portLabel")}
                        <Input
                          type="number"
                          className="mt-1 font-mono"
                          value={s.port ?? ""}
                          onChange={(e) => setSvc(id, { port: e.target.value ? Number(e.target.value) : null })}
                        />
                      </label>
                      <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
                        {t("pages.config.dependsOnLabel")}
                        <Input
                          className="mt-1 font-mono"
                          value={(s.depends_on ?? []).join(", ")}
                          onChange={(e) =>
                            setSvc(id, {
                              depends_on: e.target.value
                                .split(",")
                                .map((x) => x.trim())
                                .filter(Boolean),
                            })
                          }
                        />
                      </label>
                    </div>
                    {/* 1.7 §4：per-kind 字段（group + 新 kind 字段），按 kind 显隐 */}
                    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                      <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
                        {t("pages.config.groupLabel")}
                        <Input
                          className="mt-1"
                          value={s.group ?? ""}
                          onChange={(e) => setSvc(id, { group: e.target.value || null })}
                        />
                      </label>
                      {s.kind === "spring-boot" || s.kind === "python" ? (
                        <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
                          {s.kind === "python" ? t("pages.config.modulePythonLabel") : t("pages.config.moduleLabel")}
                          <Input
                            className="mt-1 font-mono"
                            value={s.module ?? ""}
                            onChange={(e) => setSvc(id, { module: e.target.value || null })}
                          />
                        </label>
                      ) : null}
                      {s.kind === "node" || s.kind === "python" || s.kind === "go" || s.kind === "generic" ? (
                        <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
                          {t("pages.config.dirLabel")}
                          <Input
                            className="mt-1 font-mono"
                            value={s.dir ?? ""}
                            onChange={(e) => setSvc(id, { dir: e.target.value || null })}
                          />
                        </label>
                      ) : null}
                      {s.kind === "python" ? (
                        <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
                          {t("pages.config.entryLabel")}
                          <Input
                            className="mt-1 font-mono"
                            value={s.entry ?? ""}
                            onChange={(e) => setSvc(id, { entry: e.target.value || null })}
                          />
                        </label>
                      ) : null}
                      {s.kind === "go" ? (
                        <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
                          {t("pages.config.packageLabel")}
                          <Input
                            className="mt-1 font-mono"
                            value={s.package ?? ""}
                            placeholder="./cmd/server"
                            onChange={(e) => setSvc(id, { package: e.target.value || null })}
                          />
                        </label>
                      ) : null}
                      {s.kind === "generic" ? (
                        <>
                          <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
                            {t("pages.config.programLabel")}
                            <Input
                              className="mt-1 font-mono"
                              value={s.program ?? ""}
                              onChange={(e) => setSvc(id, { program: e.target.value || null })}
                            />
                          </label>
                          <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
                            {t("pages.config.argsLabel")}
                            <Input
                              className="mt-1 font-mono"
                              value={(s.args ?? []).join(", ")}
                              onChange={(e) =>
                                setSvc(id, {
                                  args: e.target.value
                                    .split(",")
                                    .map((x) => x.trim())
                                    .filter(Boolean),
                                })
                              }
                            />
                          </label>
                        </>
                      ) : null}
                    </div>
                    <div>
                      <div className="mb-1.5 text-[0.75rem] font-medium text-[var(--t3,#8a8f98)]">{t("pages.config.serviceEnv")}</div>
                      <EnvVariablesEditor value={s.env} onChange={(env) => setEnv(id, env)} hideTitle />
                    </div>
                  </div>
                ) : null}
              </div>
            );
          })}
        </section>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// 2.1 §4.2 场景 2：AI 配置建议（sanitize 在后端；建议 yaml 整段填入编辑器，保存仍走用户点击）
// ---------------------------------------------------------------------------

/** 从 AI 回复中提取 ```yaml 围栏作为「建议 yaml 参考稿」。 */
function extractYamlFence(text: string): string | null {
  const match = /```yaml\s*\n([\s\S]*?)(?:```|$)/.exec(text);
  const body = match?.[1]?.trim();
  return body ? body : null;
}

type AiSuggestion = { text: string; suggestedYaml: string | null };

function AiSuggestCard({
  suggestion,
  busy,
  onFill,
  onClose,
}: {
  suggestion: AiSuggestion;
  busy: boolean;
  onFill: (yaml: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="mx-4 mt-3 rounded-[var(--r-md,12px)] border border-[rgb(94_106_210_/_0.35)] bg-[var(--surface,#fff)] p-3 shadow-[var(--shadow-1,0_1px_2px_rgb(16_24_40_/_0.05))]">
      <div className="flex items-center gap-2">
        <Sparkles className="size-3.5 text-[var(--st-accent,#5e6ad2)]" />
        <h4 className="text-[0.8rem] font-semibold text-[var(--t1,#222326)]">{t("pages.config.aiSuggestTitle")}</h4>
        <Badge variant="outline" className="text-[10px]">{t("pages.config.aiSuggestBadge")}</Badge>
        <Button variant="outline" size="sm" className="ml-auto" onClick={onClose}>
          {t("common.close")}
        </Button>
      </div>
      <AiOutputBody content={suggestion.text} className="mt-2" />
      {suggestion.suggestedYaml ? (
        <>
          <p className="mt-3 text-[0.72rem] font-medium text-[var(--t3,#8a8f98)]">{t("pages.config.aiSuggestYamlLabel")}</p>
          <pre className="mt-1 max-h-56 overflow-auto rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] p-2 font-mono text-[0.72rem] leading-relaxed text-[var(--t1,#222326)]">
            {suggestion.suggestedYaml}
          </pre>
          <div className="mt-2 flex items-center gap-2">
            <Button variant="outline" size="sm" onClick={() => onFill(suggestion.suggestedYaml!)} disabled={busy}>
              {t("pages.config.aiSuggestFill")}
            </Button>
            <span className="text-[0.7rem] text-[var(--t3,#8a8f98)]">{t("pages.config.aiSuggestFillHint")}</span>
          </div>
        </>
      ) : null}
    </div>
  );
}

function RawTab() {
  const yaml = useYaml();
  const { toast } = useToast();
  const { t } = useTranslation();
  const [text, setText] = useState(yaml.state.text);
  const [saving, setSaving] = useState(false);
  // 2.1：AI 配置建议（soft 按钮触发；只建议不落盘）
  const [suggesting, setSuggesting] = useState(false);
  const [suggestion, setSuggestion] = useState<AiSuggestion | null>(null);

  useEffect(() => {
    setText(yaml.state.text);
  }, [yaml.state.text]);

  const dirty = text !== yaml.state.text;

  const save = async (): Promise<boolean> => {
    setSaving(true);
    const ok = await yaml.actions.saveText(text);
    setSaving(false);
    if (ok) toast(t("pages.config.yamlSaved"), "ok");
    else toast(yaml.state.error ?? t("pages.config.saveFailedReload"), "err");
    return ok;
  };

  // 未保存守卫：原始 YAML 文本有改动即视为脏
  useUnsavedEntry("config.raw", () => text !== yaml.state.text, save);

  const ports = useMemo(() => {
    const map: Record<number, number> = {};
    let dup: number | null = null;
    const re = /^ {4}port:\s*(\d+)/gm;
    let m: RegExpExecArray | null;
    while ((m = re.exec(text))) {
      const p = Number(m[1]);
      if (map[p]) dup = p;
      map[p] = (map[p] ?? 0) + 1;
    }
    return dup;
  }, [text]);

  const askAi = async () => {
    if (suggesting) return;
    setSuggesting(true);
    setSuggestion(null);
    try {
      const problems = [...yaml.state.warnings];
      if (ports) problems.push(t("pages.config.portDup", { port: ports }));
      const out = await apiAiComplete("config_suggest", {
        yaml: text,
        problems,
      });
      setSuggestion({ text: out.text, suggestedYaml: extractYamlFence(out.text) });
    } catch (e) {
      toast(e instanceof IpcFailure ? formatIpcFailure(e) : String(e), "err");
    } finally {
      setSuggesting(false);
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="sticky top-0 z-10 flex flex-wrap items-center gap-2 border-b border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] px-4 py-2">
        <Button size="sm" variant="success" onClick={save} disabled={saving || !dirty}>
          {saving ? t("pages.config.saving") : t("pages.config.saveYaml")}
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={!dirty}
          onClick={() => setText(yaml.state.text)}
          title={t("pages.config.discardHint")}
        >
          {t("pages.config.revert")}
        </Button>
        {dirty ? <Badge variant="outline" className="border-[#f0d58a] bg-[#fdf6e3] text-[#B7791F]">{t("pages.config.unsaved")}</Badge> : null}
        <Button
          size="sm"
          variant="soft"
          className="gap-1"
          onClick={() => void askAi()}
          disabled={suggesting || !text.trim()}
          title={t("pages.config.aiSuggestTitleFull")}
        >
          <Sparkles className={suggesting ? "size-3.5 animate-pulse" : "size-3.5"} />
          {suggesting ? t("pages.config.aiSuggesting") : t("pages.config.aiSuggest")}
        </Button>
        {ports ? (
          <span className="text-[0.75rem] text-[var(--st-warn,#9a6700)]">{t("pages.config.portDup", { port: ports })}</span>
        ) : (
          <span className="text-[0.75rem] text-[var(--st-ok-deep,#1e7e35)]">{t("pages.config.portOk")}</span>
        )}
        <span className="ml-auto font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]">
          hash {yaml.state.hash.slice(0, 8) || "—"} · {t("common.linesUnit", { n: text.split("\n").length })}
        </span>
      </div>
      {suggestion ? (
        <AiSuggestCard
          suggestion={suggestion}
          busy={suggesting}
          onFill={(yamlText) => setText(yamlText)}
          onClose={() => setSuggestion(null)}
        />
      ) : null}
      <Textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        className="min-h-0 flex-1 resize-none rounded-none border-0 bg-[#FBFBFC] px-4 py-3 font-mono text-[0.78rem] leading-[1.65]"
        spellCheck={false}
        aria-label={t("pages.config.rawAria")}
      />
    </div>
  );
}
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 重新扫描 merge 预览（ipc.md §10.4）：向导组件已抽到 components/scan-merge.tsx，
// 2.1 README 导入（/discover）共用同一向导；本页仅保留状态与调用。
// ---------------------------------------------------------------------------


export function ConfigPage() {
  const ws = useWorkspace();
  const yaml = useYaml();
  const { toast } = useToast();
  const { t } = useTranslation();
  const [tab, setTab] = useState<"form" | "raw">("form");
  const { confirmLeave } = useUnsavedGuard();

  // 页内 Tab 切换守卫：另一 Tab 有未保存内容时先确认
  const switchTab = async (k: "form" | "raw") => {
    if (k === tab) return;
    if (!(await confirmLeave())) return;
    setTab(k);
  };

  // 重新扫描预览状态
  const [preview, setPreview] = useState<ScanPreviewOut | null>(null);
  const [scanning, setScanning] = useState(false);
  const [applying, setApplying] = useState(false);
  const [conflictOpen, setConflictOpen] = useState(false);
  const [addChecked, setAddChecked] = useState<Record<string, boolean>>({});
  const [fieldChoices, setFieldChoices] = useState<Record<string, Record<string, FieldChoice>>>({});

  // 1.4 Taskfile 导入向导状态（feature spec §7 / §11.2）
  const [taskPreview, setTaskPreview] = useState<TaskfilePreviewOut | null>(null);
  const [taskLoading, setTaskLoading] = useState(false);
  const [taskApplying, setTaskApplying] = useState(false);
  const [taskChecked, setTaskChecked] = useState<Record<string, boolean>>({});

  const resetChoices = () => {
    setAddChecked({});
    setFieldChoices({});
  };

  const closePreview = () => {
    setPreview(null);
    resetChoices();
  };

  const closeTaskPreview = () => {
    setTaskPreview(null);
    setTaskChecked({});
  };

  // 切换工作区后旧预览失效
  useEffect(() => {
    closePreview();
    closeTaskPreview();
  }, [ws.state.workspaceId]);

  const rescan = async () => {
    const wid = ws.state.workspaceId;
    if (!wid || scanning) return;
    setScanning(true);
    try {
      const out = await apiScanPreview(wid);
      setPreview(out);
      closeTaskPreview(); // 与 Taskfile 导入向导互斥
      resetChoices(); // 二次扫描重置选择
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setScanning(false);
    }
  };

  // 1.4：Taskfile 导入（ipc.md §10.8）。预览为纯内存计算；应用走 yaml.saveForm 机制。
  const openTaskfileImport = async () => {
    const wid = ws.state.workspaceId;
    if (!wid || taskLoading) return;
    setTaskLoading(true);
    try {
      const out = await apiTaskfilePreview(wid);
      closePreview(); // 与扫描预览互斥
      setTaskPreview(out);
      const checked: Record<string, boolean> = {};
      for (const it of out.tasks) checked[it.script_id] = it.selected;
      setTaskChecked(checked);
    } catch (e) {
      toast(formatIpcFailure(e), "err");
    } finally {
      setTaskLoading(false);
    }
  };

  const applyTaskfile = async () => {
    const wid = ws.state.workspaceId;
    if (!wid || !taskPreview || taskApplying) return;
    const selected = taskPreview.tasks
      .filter((it) => !it.internal && taskChecked[it.script_id])
      .map((it) => it.script_id);
    if (selected.length === 0) {
      toast(t("pages.config.taskfile.selectFirst"), "warn");
      return;
    }
    setTaskApplying(true);
    try {
      // base_hash 优先取 yaml-provider 当前值；无 hash 时先 yaml.get
      let baseHash = yaml.state.hash;
      if (!baseHash) baseHash = (await apiYamlGet()).hash;
      await apiTaskfileApply(wid, selected, baseHash);
      toast(t("pages.config.taskfile.applied", { n: selected.length }), "ok");
      closeTaskPreview();
      await yaml.actions.reload();
      await ws.actions.refreshSpec();
    } catch (e) {
      if (e instanceof IpcFailure && e.code === "YAML_CONFLICT") {
        setConflictOpen(true);
      } else {
        toast(formatIpcFailure(e), "err");
      }
    } finally {
      setTaskApplying(false);
    }
  };

  const applyCount = useMemo(() => {
    if (!preview) return 0;
    let n = 0;
    for (const it of preview.items) {
      if ((it.status === "added" || it.status === "id_conflict") && addChecked[it.service_id]) n += 1;
      if (it.status === "match_diff" && it.field_diffs.some((f) => fieldChoices[it.service_id]?.[f] === "update")) n += 1;
    }
    return n;
  }, [preview, addChecked, fieldChoices]);

  const applyChoices = async () => {
    const wid = ws.state.workspaceId;
    if (!wid || !preview || applying) return;
    const choices: MergeChoice[] = [];
    for (const it of preview.items) {
      if (it.status === "added") {
        if (addChecked[it.service_id]) choices.push({ id: it.service_id, action: "add" });
      } else if (it.status === "id_conflict") {
        if (addChecked[it.service_id]) choices.push({ id: it.candidate_id ?? it.service_id, action: "add" });
      } else if (it.status === "match_diff") {
        const fields = it.field_diffs.filter((f) => fieldChoices[it.service_id]?.[f] === "update");
        if (fields.length > 0) choices.push({ id: it.service_id, action: "update", fields });
      }
      // match_same / missing：不传（默认 keep 语义）
    }
    if (choices.length === 0) {
      toast(t("pages.config.selectChangesFirst"), "warn");
      return;
    }

    setApplying(true);
    try {
      // base_hash 优先取 yaml-provider 当前值；无 hash 时先 yaml.get
      let baseHash = yaml.state.hash;
      if (!baseHash) baseHash = (await apiYamlGet()).hash;
      await apiScanApply(wid, choices, baseHash);
      toast(t("pages.config.appliedN", { n: choices.length }), "ok");
      closePreview();
      await yaml.actions.reload();
      await ws.actions.refreshSpec();
    } catch (e) {
      if (e instanceof IpcFailure && e.code === "YAML_CONFLICT") {
        setConflictOpen(true);
      } else {
        toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
      }
    } finally {
      setApplying(false);
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-2 border-b border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] px-4 py-2">
        <div className="inline-flex items-center gap-0.5 rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f3f4f5)] p-0.5">
          {([
            { k: "form", label: t("pages.config.tabForm"), icon: SlidersHorizontal },
            { k: "raw", label: t("pages.config.tabRaw"), icon: FileText },
          ] as const).map((tabItem) => (
            <button
              key={tabItem.k}
              type="button"
              onClick={() => void switchTab(tabItem.k)}
              className={cn(
                "flex cursor-pointer items-center gap-1 rounded-[7px] px-3 py-1.5 text-[0.73rem] font-semibold transition-all duration-150",
                tab === tabItem.k
                  ? "bg-[var(--surface,#fff)] text-[var(--st-accent,#5e6ad2)] shadow-sm"
                  : "text-[var(--t2,#62666d)] hover:text-[var(--t1,#222326)]",
              )}
            >
              <tabItem.icon className="size-3.5" /> {tabItem.label}
            </button>
          ))}
        </div>
        <Button
          variant="soft"
          size="sm"
          className="ml-auto gap-1"
          onClick={() => void rescan()}
          disabled={!ws.state.workspaceId || scanning}
          title={ws.state.workspaceId ? t("pages.config.rescanTitleFull") : t("pages.config.openWsFirst")}
        >
          <RefreshCw className={cn("size-3.5", scanning && "animate-spin")} />
          {scanning ? t("pages.config.scanning") : t("pages.config.rescan")}
        </Button>
        <Button
          variant="outline"
          size="sm"
          className="gap-1"
          onClick={() => void openTaskfileImport()}
          disabled={!ws.state.workspaceId || taskLoading}
          title={ws.state.workspaceId ? t("pages.config.taskfile.entryHint") : t("pages.config.openWsFirst")}
        >
          <FileInput className={cn("size-3.5", taskLoading && "animate-pulse")} />
          {t("pages.config.taskfile.entry")}
        </Button>
        {preview ? <Badge variant="outline" className="border-[rgb(94_106_210_/_0.35)] bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent,#5e6ad2)]">{t("pages.config.previewing")}</Badge> : null}
        {taskPreview ? <Badge variant="outline" className="border-[rgb(94_106_210_/_0.35)] bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent,#5e6ad2)]">{t("pages.config.taskfile.previewing")}</Badge> : null}
      </div>

      <V12ConfigPanel />

      <div className="flex min-h-0 flex-1 flex-col">
        {preview ? (
          <div className="flex min-h-0 flex-1 flex-col p-4">
            <ScanPreviewPanel
              preview={preview}
              addChecked={addChecked}
              onToggleAdd={(id, v) => setAddChecked((m) => ({ ...m, [id]: v }))}
              onSelectAllAddable={(v) => {
                const next: Record<string, boolean> = {};
                for (const it of preview.items) {
                  if (it.status === "added" || it.status === "id_conflict") next[it.service_id] = v;
                }
                setAddChecked((m) => ({ ...m, ...next }));
              }}
              fieldChoices={fieldChoices}
              onFieldChoice={(id, f, c) =>
                setFieldChoices((m) => ({ ...m, [id]: { ...(m[id] ?? {}), [f]: c } }))
              }
              applying={applying}
              applyCount={applyCount}
              onApply={() => void applyChoices()}
              onClose={closePreview}
            />
          </div>
        ) : taskPreview ? (
          <div className="flex min-h-0 flex-1 flex-col p-4">
            <TaskfileImportPanel
              preview={taskPreview}
              checked={taskChecked}
              onToggle={(scriptId, v) => setTaskChecked((m) => ({ ...m, [scriptId]: v }))}
              onSelectAll={(v) => {
                const next: Record<string, boolean> = {};
                for (const it of taskPreview.tasks) {
                  if (!it.internal) next[it.script_id] = v;
                }
                setTaskChecked((m) => ({ ...m, ...next }));
              }}
              applying={taskApplying}
              applyCount={taskPreview.tasks.filter((it) => !it.internal && taskChecked[it.script_id]).length}
              onApply={() => void applyTaskfile()}
              onClose={closeTaskPreview}
            />
          </div>
        ) : tab === "form" ? (
          <FormTab />
        ) : (
          <RawTab />
        )}
      </div>

      <ConfirmDialog
        open={conflictOpen}
        title={t("pages.config.conflictTitle")}
        description={t("pages.config.conflictDesc")}
        confirmText={t("pages.config.reload")}
        onConfirm={() => {
          setConflictOpen(false);
          void yaml.actions.reload();
          void ws.actions.refreshSpec();
        }}
        onCancel={() => setConflictOpen(false)}
      />
    </div>
  );
}
