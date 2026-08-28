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
import { cn } from "@/lib/utils";
import { useYaml } from "@/providers/yaml-provider";
import { useWorkspace } from "@/providers/workspace-provider";
import { useToast } from "@/components/ui/toast";
import {
  apiScanApply,
  apiScanPreview,
  apiTaskfileApply,
  apiTaskfilePreview,
  apiYamlGet,
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
  ScanMergeItem,
  ScanPreviewOut,
  ServiceSpec,
  SuperTaskFile,
  TaskfilePreviewOut,
} from "@/ipc/protocol";
import { opErrorLabel } from "@/lib/status";
import { formatIpcFailure } from "@/lib/error-messages";
import i18n from "@/i18n";

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

  const save = async () => {
    const cycle = detectCycle(draft!);
    if (cycle) {
      toast(t("pages.config.cycleDetected", { cycle: cycle.join(" → ") }), "err");
      return;
    }
    setSaving(true);
    const ok = await yaml.actions.saveForm(draft!);
    setSaving(false);
    if (ok) toast(t("pages.config.saved"), "ok");
    else toast(yaml.state.error ?? t("pages.config.saveFailed"), "err");
  };

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

function RawTab() {
  const yaml = useYaml();
  const { toast } = useToast();
  const { t } = useTranslation();
  const [text, setText] = useState(yaml.state.text);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setText(yaml.state.text);
  }, [yaml.state.text]);

  const dirty = text !== yaml.state.text;

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

  const save = async () => {
    setSaving(true);
    const ok = await yaml.actions.saveText(text);
    setSaving(false);
    if (ok) toast(t("pages.config.yamlSaved"), "ok");
    else toast(yaml.state.error ?? t("pages.config.saveFailedReload"), "err");
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
        {ports ? (
          <span className="text-[0.75rem] text-[var(--st-warn,#9a6700)]">{t("pages.config.portDup", { port: ports })}</span>
        ) : (
          <span className="text-[0.75rem] text-[var(--st-ok-deep,#1e7e35)]">{t("pages.config.portOk")}</span>
        )}
        <span className="ml-auto font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]">
          hash {yaml.state.hash.slice(0, 8) || "—"} · {t("common.linesUnit", { n: text.split("\n").length })}
        </span>
      </div>
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
// 重新扫描 merge 预览（ipc.md §10.4，spec §2.3/§6/§13）
// ---------------------------------------------------------------------------

type FieldChoice = "keep" | "update";

/** ServiceSpec 字段值的小字展示（对象/数组 JSON 化，仅展示用）。 */
function specFieldValue(spec: ServiceSpec | null | undefined, field: string): string {
  if (!spec) return "—";
  const v = (spec as unknown as Record<string, unknown>)[field];
  if (v === undefined || v === null) return "—";
  if (typeof v === "string") return v === "" ? i18n.t("pages.config.emptyValue") : v;
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  try {
    return JSON.stringify(v);
  } catch {
    return "—";
  }
}

const SCAN_GROUPS: { status: ScanMergeItem["status"]; titleKey: string }[] = [
  { status: "added", titleKey: "pages.config.groupAdded" },
  { status: "id_conflict", titleKey: "pages.config.groupConflict" },
  { status: "match_diff", titleKey: "pages.config.groupDiff" },
  { status: "match_same", titleKey: "pages.config.groupSame" },
  { status: "missing", titleKey: "pages.config.groupMissing" },
];

function ScanStatusBadge({ status }: { status: ScanMergeItem["status"] }) {
  const { t } = useTranslation();
  if (status === "match_same") return <Badge variant="secondary">{t("pages.config.groupSame")}</Badge>;
  if (status === "match_diff") return <Badge variant="outline" className="border-[#f0d58a] bg-[#fdf6e3] text-[#B7791F]">{t("pages.config.groupDiff")}</Badge>;
  if (status === "missing") return <Badge variant="outline" className="border-red-200 bg-[var(--st-danger-tint,#fdecec)] text-[#DC2626]">{t("pages.config.groupMissing")}</Badge>;
  if (status === "id_conflict") return <Badge variant="outline" className="border-[#f0d58a] bg-[#fdf6e3] text-[#B7791F]">{t("pages.config.groupConflict")}</Badge>;
  return <Badge variant="soon">{t("pages.config.groupAdded")}</Badge>;
}

/** match_diff 的字段行：当前值 / 发现值并排小字 + 保留/采用切换。 */
function DiffFieldRow({
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

function ScanItemRow({
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
          <label className="flex shrink-0 items-center gap-1.5 text-[0.76rem] font-medium text-[var(--t1,#222326)]">
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

      {item.status === "missing" ? (
        <div className="mt-1.5 text-[0.74rem] text-[#B7791F]">
          {t("pages.config.missingDesc")}
        </div>
      ) : null}
    </div>
  );
}

function ScanPreviewPanel({
  preview,
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
  const allAddableChecked = addable.length > 0 && addable.every((it) => addChecked[it.service_id]);

  return (
    <section
      className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[var(--r-lg,16px)] border border-[rgb(94_106_210_/_0.35)] bg-[var(--surface,#fff)] shadow-[var(--shadow-2,0_6px_20px_rgb(16_24_40_/_0.09))]"
      aria-label={t("pages.config.rescanAria")}
    >
      <div className="flex shrink-0 flex-wrap items-center gap-2 rounded-t-[var(--r-lg,16px)] border-b border-[var(--line,#e6e6e6)] bg-[var(--st-accent-tint,#eef0fb)] px-3 py-2.5">
        <h3 className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">{t("pages.config.rescanTitle")}</h3>
        <span className="font-mono text-[0.7rem] text-[var(--t3,#8a8f98)]">{t("pages.config.itemCount", { n: preview.items.length })}</span>
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
          {SCAN_GROUPS.map(({ status, titleKey }) => {
            const items = preview.items.filter((it) => it.status === status);
            if (items.length === 0) return null;
            return (
              <div key={status}>
                <div className="mb-1.5 flex items-center gap-2 px-0.5">
                  <span className="text-[0.72rem] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">{t(titleKey)}</span>
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


export function ConfigPage() {
  const ws = useWorkspace();
  const yaml = useYaml();
  const { toast } = useToast();
  const { t } = useTranslation();
  const [tab, setTab] = useState<"form" | "raw">("form");

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
              onClick={() => setTab(tabItem.k)}
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
