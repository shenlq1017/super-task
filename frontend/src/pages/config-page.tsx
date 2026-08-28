import { useEffect, useMemo, useState } from "react";
import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input, Textarea } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
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
import { FileText, SlidersHorizontal } from "lucide-react";

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
    if (!wid || !yaml.state.hash) return;
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

  return (
    <section className="border-b border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] px-4 py-3">
      <div className="grid gap-4 lg:grid-cols-2">
        <div>
          <div className="mb-2 flex items-center gap-2">
            <h3 className="text-[0.8rem] font-semibold text-[var(--t1,#222326)]">Profile</h3>
            <Badge variant="secondary">只覆盖 env / enabled / port</Badge>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <select
              className="h-8 min-w-[10rem] rounded-[var(--r-sm,8px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] px-2 text-xs"
              value={profiles?.active ?? "default"}
              onChange={(e) => void activate(e.target.value)}
              disabled={!wid || loading || !profiles || profiles.profiles.length === 0}
              aria-label="当前 profile"
            >
              <option value="default">default（隐式）</option>
              {profiles?.profiles.map((p) => <option key={p.id} value={p.id}>{p.id}{p.enabled_count != null ? ` · ${p.enabled_count} 项启用` : ""}</option>)}
            </select>
            <Button variant="outline" size="sm" onClick={() => void reload()} disabled={!wid || loading}>刷新</Button>
          </div>
          <p className="mt-2 text-[0.7rem] text-[var(--t3,#8a8f98)]">运行中的服务或脚本会阻止切换，避免运行态与配置不一致。</p>
        </div>

        <div>
          <div className="mb-2 flex items-center gap-2">
            <h3 className="text-[0.8rem] font-semibold text-[var(--t1,#222326)]">Secret 文件</h3>
            {secrets?.file ? <Badge variant="outline">{secrets.file}</Badge> : <Badge variant="outline">用户环境</Badge>}
            <Button className="ml-auto" variant="outline" size="sm" onClick={() => void validateSecrets()} disabled={!wid}>校验必需项</Button>
          </div>
          <div className="flex flex-col gap-1.5">
            {secrets?.keys.length ? secrets.keys.map((item) => (
              <div key={item.key} className="flex items-center gap-2 rounded bg-[var(--surface,#fff)] px-2 py-1 text-xs">
                <code className="min-w-0 flex-1 truncate font-mono">{item.key}</code>
                <Badge variant={item.present ? "default" : "outline"}>{item.present ? "已设置" : "缺失"}</Badge>
                {item.git_tracked ? <Badge variant="outline" className="border-red-200 text-red-600">Git tracked</Badge> : null}
                <Button variant="ghost" size="sm" onClick={() => void deleteSecret(item.key)}>删除</Button>
              </div>
            )) : <span className="text-xs text-[var(--t3,#8a8f98)]">暂无状态；可在下方新增 key。</span>}
          </div>
          <div className="mt-2 flex gap-2">
            <Input className="h-8 font-mono text-xs" placeholder="KEY_NAME" value={secretKey} onChange={(e) => setSecretKey(e.target.value)} aria-label="secret key" />
            <Input className="h-8 text-xs" type="password" placeholder="值不会回显" value={secretValue} onChange={(e) => setSecretValue(e.target.value)} aria-label="secret value" />
            <Button size="sm" onClick={() => void saveSecret()} disabled={!wid || !secretKey.trim()}>保存</Button>
          </div>
        </div>
      </div>
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

  useEffect(() => {
    setDraft(spec);
  }, [spec]);

  if (!spec || !draft) return <div className="p-4 text-[0.875rem] text-[var(--t3,#8a8f98)]">无配置</div>;

  const setSvc = (id: string, patch: Partial<SuperTaskFile["services"][string]>) =>
    setDraft((d) => (d ? { ...d, services: { ...d.services, [id]: { ...d.services[id], ...patch } } } : d));

  const setEnv = (id: string, env: Record<string, string>) => setSvc(id, { env });
  const setWsEnv = (env: Record<string, string>) => setDraft((d) => (d ? { ...d, env } : d));

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

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-2 px-4 py-2">
        <Button size="sm" onClick={save} disabled={saving}>
          保存
        </Button>
        {yaml.state.warnings.length ? (
          <span className="text-[0.75rem] text-[var(--st-warn,#9a6700)]">{yaml.state.warnings.length} 条解析警告</span>
        ) : null}
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-4">
        <section className="mb-6">
          <h3 className="mb-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">工作区环境变量</h3>
          <EnvEditor value={draft.env} onChange={setWsEnv} />
        </section>
        <section className="flex flex-col gap-3">
          <h3 className="text-[0.875rem] font-semibold text-[var(--t1,#222326)]">服务</h3>
          {Object.entries(draft.services).map(([id, s]) => (
            <div key={id} className="rounded-[var(--r-md,12px)] border border-[var(--line,#e6e6e6)] p-3 shadow-[var(--shadow-1,0_1px_2px_rgb(16_24_40_/_0.05))]">
              <div className="mb-2 flex items-center gap-2">
                <span className="font-semibold text-[var(--t1,#222326)]">{id}</span>
                <Badge variant="outline" className="text-[10px] uppercase">
                  {s.kind}
                </Badge>
                <label className="ml-auto flex items-center gap-1 text-[0.75rem] text-[var(--t3,#8a8f98)]">
                  <input
                    type="checkbox"
                    checked={s.enabled}
                    onChange={(e) => setSvc(id, { enabled: e.target.checked })}
                  />
                  启用
                </label>
              </div>
              <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
                <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
                  端口
                  <Input
                    type="number"
                    className="mt-1"
                    value={s.port ?? ""}
                    onChange={(e) => setSvc(id, { port: e.target.value ? Number(e.target.value) : null })}
                  />
                </label>
                <label className="col-span-2 text-[0.75rem] text-[var(--t3,#8a8f98)] sm:col-span-2">
                  depends_on（逗号分隔）
                  <Input
                    className="mt-1"
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
              <div className="mt-3">
                <div className="mb-1 text-[0.75rem] text-[var(--t3,#8a8f98)]">环境变量</div>
                <EnvEditor value={s.env} onChange={(env) => setEnv(id, env)} />
              </div>
            </div>
          ))}
        </section>
      </div>
    </div>
  );
}

function EnvEditor({ value, onChange }: { value: Record<string, string>; onChange: (v: Record<string, string>) => void }) {
  const entries = Object.entries(value);
  return (
    <div className="flex flex-col gap-1.5">
      {entries.map(([k, v]) => (
        <div key={k} className="flex items-center gap-2">
          <code className="w-40 shrink-0 truncate rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f3f4f5)] px-2 py-1 font-mono text-[0.72rem]">{k}</code>
          <Input value={v} onChange={(e) => onChange({ ...value, [k]: e.target.value })} />
          <button
            className="text-[var(--t3,#8a8f98)] transition-colors hover:text-[var(--st-danger,#dc2626)]"
            onClick={() => {
              const n = { ...value };
              delete n[k];
              onChange(n);
            }}
            title="删除"
          >
            ✕
          </button>
        </div>
      ))}
      <button
        className="self-start text-[0.75rem] font-semibold text-[var(--st-accent,#5e6ad2)] hover:underline"
        onClick={() => onChange({ ...value, [`NEW_${entries.length + 1}`]: "" })}
      >
        + 添加变量
      </button>
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
      <div className="flex items-center gap-2 px-4 py-2">
        <Button size="sm" onClick={save} disabled={saving}>
          保存
        </Button>
        {ports ? (
          <span className="text-[0.75rem] text-[var(--st-warn,#9a6700)]">端口 {ports} 重复（仍可保存，运行时会按 base_hash 校验）</span>
        ) : (
          <span className="text-[0.75rem] text-[var(--st-ok-deep,#1e7e35)]">端口无重复</span>
        )}
        <span className="ml-auto font-mono text-[0.75rem] text-[var(--t3,#8a8f98)]">base_hash: {yaml.state.hash.slice(0, 8)}</span>
      </div>
      <Textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        className="min-h-0 flex-1 rounded-none border-0 font-mono"
        spellCheck={false}
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
    <div className="rounded-[var(--r-md,12px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] p-2.5">
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
  fieldChoices: Record<string, Record<string, FieldChoice>>;
  onFieldChoice: (id: string, field: string, c: FieldChoice) => void;
  applying: boolean;
  applyCount: number;
  onApply: () => void;
  onClose: () => void;
}) {
  return (
    <section className="mx-4 mb-3 rounded-[var(--r-lg,16px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] p-3" aria-label="重新扫描预览">
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">重新扫描预览</h3>
        <span className="font-mono text-[0.7rem] text-[var(--t3,#8a8f98)]">{preview.items.length} 项</span>
        <Button variant="outline" size="sm" className="ml-auto" onClick={onClose}>
          关闭
        </Button>
      </div>

      {preview.warnings.length > 0 ? (
        <div className="mt-2 rounded-[var(--r-sm,8px)] border border-[#f0d58a] bg-[#fdf6e3] px-2.5 py-1.5 text-[0.74rem] leading-relaxed text-[#B7791F]" role="alert">
          {preview.warnings.map((w, i) => (
            <div key={i}>{w}</div>
          ))}
        </div>
      ) : null}

      <div className="mt-2 flex max-h-[22rem] flex-col gap-3 overflow-y-auto pr-1">
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

      <div className="mt-2.5 flex items-center gap-2">
        <span className="min-w-0 flex-1 text-[0.74rem] text-[var(--t3,#8a8f98)]">
          应用以所选内容写回 supertask.yaml（带 base_hash 校验）。
        </span>
        <Button size="sm" onClick={onApply} disabled={applying || applyCount === 0}>
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
      <div className="flex items-center gap-2 border-b border-[var(--line,#e6e6e6)] px-4 py-2">
        <div className="flex items-center gap-1">
          {([
            { k: "form", label: "表单", icon: SlidersHorizontal },
            { k: "raw", label: "原文 YAML", icon: FileText },
          ] as const).map((t) => (
            <button
              key={t.k}
              onClick={() => setTab(t.k)}
              className={cn(
                "flex items-center gap-1 rounded-full px-3 py-1 text-[0.73rem] font-semibold transition-all duration-150",
                tab === t.k
                  ? "bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent-hover,#4f5ac8)]"
                  : "text-[var(--t2,#62666d)] hover:text-[var(--t1,#222326)]",
              )}
            >
              <t.icon className="size-3.5" /> {t.label}
            </button>
          ))}
        </div>
        <Button
          variant="outline"
          size="sm"
          className="ml-auto gap-1"
          onClick={() => void rescan()}
          disabled={!ws.state.workspaceId || scanning}
          title={ws.state.workspaceId ? "重新扫描磁盘并生成合并预览" : "请先打开工作区"}
        >
          <RefreshCw className={cn("size-3.5", scanning && "animate-spin")} /> 重新扫描
        </Button>
      </div>

      <V12ConfigPanel />

      {preview ? (
        <ScanPreviewPanel
          preview={preview}
          addChecked={addChecked}
          onToggleAdd={(id, v) => setAddChecked((m) => ({ ...m, [id]: v }))}
          fieldChoices={fieldChoices}
          onFieldChoice={(id, f, c) =>
            setFieldChoices((m) => ({ ...m, [id]: { ...(m[id] ?? {}), [f]: c } }))
          }
          applying={applying}
          applyCount={applyCount}
          onApply={() => void applyChoices()}
          onClose={closePreview}
        />
      ) : null}

      {tab === "form" ? <FormTab /> : <RawTab />}

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
