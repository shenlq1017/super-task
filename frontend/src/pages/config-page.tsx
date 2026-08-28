import { useEffect, useMemo, useState } from "react";
import {
  ChevronDown,
  ChevronRight,
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
import { cn } from "@/lib/utils";
import { useYaml } from "@/providers/yaml-provider";
import { useWorkspace } from "@/providers/workspace-provider";
import { useToast } from "@/components/ui/toast";
import {
  apiScanApply,
  apiScanPreview,
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
} from "@/ipc/protocol";
import { opErrorLabel } from "@/lib/status";

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
      toast("配置尚未加载完成，请稍后再切换 profile", "warn");
      return;
    }
    if (profiles?.active === id) return;
    try {
      await apiProfilesActivate(wid, id, yaml.state.hash);
      await Promise.all([yaml.actions.reload(), ws.actions.refreshSpec()]);
      await reload();
      toast(`已切换到 profile：${id}`, "ok");
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
      toast(`已保存 ${secretKey.trim()}（值不会回显）`, "ok");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  const deleteSecret = async (key: string) => {
    if (!wid) return;
    try {
      await apiSecretsDelete(wid, key);
      await reload();
      toast(`已删除 ${key}`, "ok");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  const validateSecrets = async () => {
    if (!wid) return;
    try {
      const out = await apiSecretsValidate(wid);
      toast(out.ok ? "必需密钥检查通过" : `缺少：${out.missing.join(", ")}`, out.ok ? "ok" : "warn");
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
      const suffix = meta?.enabled_count != null ? ` · ${meta.enabled_count} 项启用` : "";
      return { id, label: id === "default" ? `default（隐式）${suffix}` : `${id}${suffix}` };
    });
  }, [profiles]);
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
              <RefreshCw className={cn("size-3.5", loading && "animate-spin")} /> 刷新
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
                  aria-label="当前 profile"
                >
                  <SelectValue placeholder="选择 profile" />
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
                <span className="text-[0.7rem] text-[var(--t3,#8a8f98)]">yaml 未定义其他 profile</span>
              </div>
            )}
          </div>
          <p className="mt-2 text-[0.7rem] leading-relaxed text-[var(--t3,#8a8f98)]">
            运行中服务/脚本会阻止切换，避免运行态与配置不一致。
            {!yaml.state.hash ? " 配置加载中，切换暂不可用。" : null}
          </p>
        </div>

        <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] p-3">
          <div className="mb-2 flex items-center gap-2">
            <KeyRound className="size-3.5 text-[var(--st-accent,#5e6ad2)]" />
            <h3 className="text-[0.8rem] font-semibold text-[var(--t1,#222326)]">Secrets</h3>
            {secrets?.file ? <Badge variant="outline" className="max-w-[10rem] truncate" title={secrets.file}>{secrets.file}</Badge> : <Badge variant="outline">用户环境</Badge>}
            <Badge variant="secondary" className="font-mono text-[10px]">{secretCount}</Badge>
            <Button className="ml-auto" variant="soft" size="sm" onClick={() => void validateSecrets()} disabled={!wid}>校验必需项</Button>
          </div>

          <div className="flex max-h-[10rem] flex-col gap-1 overflow-y-auto">
            {secrets?.keys.length ? secrets.keys.map((item) => (
              <div
                key={item.key}
                className="grid grid-cols-[1fr_auto_auto_auto] items-center gap-2 rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] px-2 py-1.5 text-xs"
              >
                <code className="min-w-0 truncate font-mono" title={item.key}>{item.key}</code>
                <Badge variant={item.present ? "default" : "outline"}>{item.present ? "已设置" : "缺失"}</Badge>
                {item.git_tracked ? <Badge variant="outline" className="border-red-200 text-red-600">Git</Badge> : <span />}
                <button
                  type="button"
                  onClick={() => setDeleteKey(item.key)}
                  className="grid size-7 cursor-pointer place-items-center rounded-[var(--r-sm,8px)] text-[var(--t3,#8a8f98)] transition-colors hover:bg-[var(--st-danger-tint,#fdecec)] hover:text-[var(--st-danger,#dc2626)]"
                  title={`删除 ${item.key}`}
                >
                  <Trash2 className="size-3.5" />
                </button>
              </div>
            )) : (
              <p className="py-1 text-xs text-[var(--t3,#8a8f98)]">暂无登记密钥；在下方添加。</p>
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
              placeholder="值（保存后不回显）"
              value={secretValue}
              onChange={(e) => setSecretValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && secretKey.trim()) void saveSecret();
              }}
              aria-label="secret value"
            />
            <Button size="sm" variant="success" onClick={() => void saveSecret()} disabled={!wid || !secretKey.trim()}>
              保存
            </Button>
          </div>
        </div>
      </div>

      <ConfirmDialog
        open={deleteKey != null}
        title="删除密钥"
        description={deleteKey ? `确定删除 ${deleteKey}？值将从用户环境中移除，已运行服务可能仍持有旧值直到重启。` : undefined}
        confirmText="删除"
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

  if (!spec || !draft) return <div className="p-4 text-[0.875rem] text-[var(--t3,#8a8f98)]">无配置</div>;

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
      toast(`depends_on 存在循环依赖：${cycle.join(" → ")}，已拒绝保存`, "err");
      return;
    }
    setSaving(true);
    const ok = await yaml.actions.saveForm(draft!);
    setSaving(false);
    if (ok) toast("配置已保存", "ok");
    else toast(yaml.state.error ?? "保存失败（可能外部已修改）", "err");
  };

  const wsEnvCount = Object.keys(draft.env).length;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="sticky top-0 z-10 flex items-center gap-2 border-b border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] px-4 py-2">
        <Button size="sm" variant="success" onClick={save} disabled={saving}>
          {saving ? "保存中…" : "保存表单"}
        </Button>
        <span className="font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]">
          {serviceIds.length} 服务 · {wsEnvCount} 工作区变量
        </span>
        {yaml.state.warnings.length ? (
          <span className="text-[0.75rem] text-[var(--st-warn,#9a6700)]">{yaml.state.warnings.length} 条解析警告</span>
        ) : null}
        <div className="ml-auto flex gap-1">
          <Button size="sm" variant="outline" onClick={() => setOpenIds(new Set(serviceIds))}>全部展开</Button>
          <Button size="sm" variant="outline" onClick={() => setOpenIds(new Set())}>全部收起</Button>
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
            <span className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">工作区环境变量</span>
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
            <h3 className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">服务</h3>
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
                    {s.port != null ? <span className="shrink-0 font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]">{s.port}</span> : null}
                    <span className="shrink-0 font-mono text-[0.68rem] text-[var(--t3,#8a8f98)]">{envN} env</span>
                  </button>
                  <label className="flex shrink-0 cursor-pointer items-center gap-1.5 text-[0.75rem] text-[var(--t2,#62666d)]">
                    <input
                      type="checkbox"
                      checked={s.enabled}
                      onChange={(e) => setSvc(id, { enabled: e.target.checked })}
                    />
                    启用
                  </label>
                </div>
                {open ? (
                  <div className="space-y-3 border-t border-[var(--line,#e6e6e6)] px-3 py-3">
                    <div className="grid grid-cols-1 gap-3 sm:grid-cols-[7.5rem_1fr]">
                      <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
                        端口
                        <Input
                          type="number"
                          className="mt-1 font-mono"
                          value={s.port ?? ""}
                          onChange={(e) => setSvc(id, { port: e.target.value ? Number(e.target.value) : null })}
                        />
                      </label>
                      <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
                        depends_on（逗号分隔）
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
                      <div className="mb-1.5 text-[0.75rem] font-medium text-[var(--t3,#8a8f98)]">服务环境变量</div>
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
    if (ok) toast("YAML 已保存", "ok");
    else toast(yaml.state.error ?? "保存失败（可能外部已修改，请重新加载）", "err");
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="sticky top-0 z-10 flex flex-wrap items-center gap-2 border-b border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] px-4 py-2">
        <Button size="sm" variant="success" onClick={save} disabled={saving || !dirty}>
          {saving ? "保存中…" : "保存 YAML"}
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={!dirty}
          onClick={() => setText(yaml.state.text)}
          title="丢弃未保存修改"
        >
          还原
        </Button>
        {dirty ? <Badge variant="outline" className="border-[#f0d58a] bg-[#fdf6e3] text-[#B7791F]">未保存</Badge> : null}
        {ports ? (
          <span className="text-[0.75rem] text-[var(--st-warn,#9a6700)]">端口 {ports} 重复（仍可保存）</span>
        ) : (
          <span className="text-[0.75rem] text-[var(--st-ok-deep,#1e7e35)]">端口无重复</span>
        )}
        <span className="ml-auto font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]">
          hash {yaml.state.hash.slice(0, 8) || "—"} · {text.split("\n").length} 行
        </span>
      </div>
      <Textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        className="min-h-0 flex-1 resize-none rounded-none border-0 bg-[#FBFBFC] px-4 py-3 font-mono text-[0.78rem] leading-[1.65]"
        spellCheck={false}
        aria-label="supertask.yaml 原文"
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
  if (typeof v === "string") return v === "" ? "（空）" : v;
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  try {
    return JSON.stringify(v);
  } catch {
    return "—";
  }
}

const SCAN_GROUPS: { status: ScanMergeItem["status"]; title: string }[] = [
  { status: "added", title: "新发现" },
  { status: "id_conflict", title: "ID 冲突" },
  { status: "match_diff", title: "有差异" },
  { status: "match_same", title: "一致" },
  { status: "missing", title: "未发现" },
];

function ScanStatusBadge({ status }: { status: ScanMergeItem["status"] }) {
  if (status === "match_same") return <Badge variant="secondary">一致</Badge>;
  if (status === "match_diff") return <Badge variant="outline" className="border-[#f0d58a] bg-[#fdf6e3] text-[#B7791F]">有差异</Badge>;
  if (status === "missing") return <Badge variant="outline" className="border-red-200 bg-[var(--st-danger-tint,#fdecec)] text-[#DC2626]">未发现</Badge>;
  if (status === "id_conflict") return <Badge variant="outline" className="border-[#f0d58a] bg-[#fdf6e3] text-[#B7791F]">ID 冲突</Badge>;
  return <Badge variant="soon">新发现</Badge>;
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
  const cur = specFieldValue(item.current, field);
  const disc = specFieldValue(item.discovered, field);
  return (
    <div className="flex flex-wrap items-center gap-2 rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f3f4f5)] px-2 py-1.5">
      <code className="shrink-0 font-mono text-[0.72rem] font-semibold text-[var(--t1,#222326)]">{field}</code>
      <span className="min-w-0 flex-1 truncate font-mono text-[0.7rem] text-[var(--t2,#62666d)]" title={`当前：${cur}`}>
        当前 {cur}
      </span>
      <span className="min-w-0 flex-1 truncate font-mono text-[0.7rem] text-[var(--st-accent,#5e6ad2)]" title={`发现：${disc}`}>
        发现 {disc}
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
          保留当前
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
          采用发现值
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
  const kind = item.discovered?.kind ?? item.current?.kind ?? "";
  return (
    <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] p-2.5">
      <div className="flex flex-wrap items-center gap-2">
        {item.status === "added" || item.status === "id_conflict" ? (
          <label className="flex shrink-0 items-center gap-1.5 text-[0.76rem] font-medium text-[var(--t1,#222326)]">
            <input type="checkbox" checked={checked} onChange={(e) => onToggle(e.target.checked)} />
            {item.status === "added" ? "加入" : "以候选 id 加入"}
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
          已存在同名服务，将以此候选 id 写入：
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
            采用发现值仅覆盖扫描器负责字段（kind、module/dir、package_manager），端口、环境变量等用户字段一律保留。
          </span>
        </div>
      ) : null}

      {item.status === "missing" ? (
        <div className="mt-1.5 text-[0.74rem] text-[#B7791F]">
          磁盘上未发现该服务对应的项目结构；不会删除，仅提示。
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
  const addable = preview.items.filter((it) => it.status === "added" || it.status === "id_conflict");
  const allAddableChecked = addable.length > 0 && addable.every((it) => addChecked[it.service_id]);

  return (
    <section
      className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[var(--r-lg,16px)] border border-[rgb(94_106_210_/_0.35)] bg-[var(--surface,#fff)] shadow-[var(--shadow-2,0_6px_20px_rgb(16_24_40_/_0.09))]"
      aria-label="重新扫描预览"
    >
      <div className="flex shrink-0 flex-wrap items-center gap-2 rounded-t-[var(--r-lg,16px)] border-b border-[var(--line,#e6e6e6)] bg-[var(--st-accent-tint,#eef0fb)] px-3 py-2.5">
        <h3 className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">重新扫描预览</h3>
        <span className="font-mono text-[0.7rem] text-[var(--t3,#8a8f98)]">{preview.items.length} 项</span>
        {addable.length > 0 ? (
          <Button size="sm" variant="outline" onClick={() => onSelectAllAddable(!allAddableChecked)}>
            {allAddableChecked ? "取消全选新发现" : `全选新发现（${addable.length}）`}
          </Button>
        ) : null}
        <Button variant="outline" size="sm" className="ml-auto" onClick={onClose}>
          关闭
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
          {SCAN_GROUPS.map(({ status, title }) => {
            const items = preview.items.filter((it) => it.status === status);
            if (items.length === 0) return null;
            return (
              <div key={status}>
                <div className="mb-1.5 flex items-center gap-2 px-0.5">
                  <span className="text-[0.72rem] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">{title}</span>
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
          应用所选写回 yaml（base_hash 校验）。一致/未发现项不会删除。
        </span>
        <Button size="sm" variant="default" onClick={onApply} disabled={applying || applyCount === 0}>
          {applying ? "应用中…" : `应用所选（${applyCount}）`}
        </Button>
      </div>
    </section>
  );
}


export function ConfigPage() {
  const ws = useWorkspace();
  const yaml = useYaml();
  const { toast } = useToast();
  const [tab, setTab] = useState<"form" | "raw">("form");

  // 重新扫描预览状态
  const [preview, setPreview] = useState<ScanPreviewOut | null>(null);
  const [scanning, setScanning] = useState(false);
  const [applying, setApplying] = useState(false);
  const [conflictOpen, setConflictOpen] = useState(false);
  const [addChecked, setAddChecked] = useState<Record<string, boolean>>({});
  const [fieldChoices, setFieldChoices] = useState<Record<string, Record<string, FieldChoice>>>({});

  const resetChoices = () => {
    setAddChecked({});
    setFieldChoices({});
  };

  const closePreview = () => {
    setPreview(null);
    resetChoices();
  };

  // 切换工作区后旧预览失效
  useEffect(() => {
    closePreview();
  }, [ws.state.workspaceId]);

  const rescan = async () => {
    const wid = ws.state.workspaceId;
    if (!wid || scanning) return;
    setScanning(true);
    try {
      const out = await apiScanPreview(wid);
      setPreview(out);
      resetChoices(); // 二次扫描重置选择
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setScanning(false);
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
      toast("请先勾选要应用的变更", "warn");
      return;
    }

    setApplying(true);
    try {
      // base_hash 优先取 yaml-provider 当前值；无 hash 时先 yaml.get
      let baseHash = yaml.state.hash;
      if (!baseHash) baseHash = (await apiYamlGet()).hash;
      await apiScanApply(wid, choices, baseHash);
      toast(`已应用 ${choices.length} 项变更`, "ok");
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
            { k: "form", label: "表单", icon: SlidersHorizontal },
            { k: "raw", label: "原文 YAML", icon: FileText },
          ] as const).map((t) => (
            <button
              key={t.k}
              type="button"
              onClick={() => setTab(t.k)}
              className={cn(
                "flex cursor-pointer items-center gap-1 rounded-[7px] px-3 py-1.5 text-[0.73rem] font-semibold transition-all duration-150",
                tab === t.k
                  ? "bg-[var(--surface,#fff)] text-[var(--st-accent,#5e6ad2)] shadow-sm"
                  : "text-[var(--t2,#62666d)] hover:text-[var(--t1,#222326)]",
              )}
            >
              <t.icon className="size-3.5" /> {t.label}
            </button>
          ))}
        </div>
        <Button
          variant="soft"
          size="sm"
          className="ml-auto gap-1"
          onClick={() => void rescan()}
          disabled={!ws.state.workspaceId || scanning}
          title={ws.state.workspaceId ? "重新扫描磁盘并生成合并预览" : "请先打开工作区"}
        >
          <RefreshCw className={cn("size-3.5", scanning && "animate-spin")} />
          {scanning ? "扫描中…" : preview ? "重新扫描" : "重新扫描"}
        </Button>
        {preview ? <Badge variant="outline" className="border-[rgb(94_106_210_/_0.35)] bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent,#5e6ad2)]">预览中</Badge> : null}
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
        ) : tab === "form" ? (
          <FormTab />
        ) : (
          <RawTab />
        )}
      </div>

      <ConfirmDialog
        open={conflictOpen}
        title="文件已被外部修改"
        description="supertask.yaml 在扫描后被外部修改，本次应用已取消。请重新加载最新内容后重试。"
        confirmText="重新加载"
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
