import { useEffect, useRef, useState } from "react";
import { CheckCircle2, Loader2, Pin, RefreshCw, XCircle, Wrench, ArrowUpCircle, Download } from "lucide-react";
import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useSession } from "../providers/session-provider";
import { useWorkspace } from "../providers/workspace-provider";
import { useYaml } from "@/providers/yaml-provider";
import { useOperations } from "../providers/operation-provider";
import { useToast } from "@/components/ui/toast";
import { apiToolchainInstall, apiToolchainProbe, apiToolchainUpgrade } from "../ipc/api";
import { IpcFailure, type ManagerAvailability, type ToolProbe, type ToolchainProbeOut } from "../ipc/protocol";
import { opErrorLabel } from "@/lib/status";
import { cn } from "@/lib/utils";

type ToolKey = "java" | "maven" | "node" | "npm" | "pnpm" | "yarn";

/** 客户端镜像后端 manifest 默认版本（§4.3 版本来源第 3 级）。 */
const DEFAULT_VERSION: Record<ToolKey, string> = {
  java: "21",
  maven: "3.9",
  node: "20",
  npm: "20",
  pnpm: "9",
  yarn: "1",
};

const CORE_TOOLS: { key: ToolKey; label: string; rec: string }[] = [
  { key: "java", label: "JDK", rec: "21 LTS" },
  { key: "maven", label: "Maven", rec: "3.9" },
  { key: "node", label: "Node.js", rec: "20 LTS" },
];

/** npm/pnpm/yarn 只在当前工作区有 node 服务时出现（§15.1）。 */
const PKG_TOOLS: { key: ToolKey; label: string; rec: string }[] = [
  { key: "npm", label: "npm", rec: "随 Node" },
  { key: "pnpm", label: "pnpm", rec: "9" },
  { key: "yarn", label: "Yarn", rec: "1" },
];

type PendingOp = { opId: string; verb: "install" | "upgrade" | "pin" };

export function EnvPage() {
  const { state: session } = useSession();
  const ws = useWorkspace();
  const yaml = useYaml();
  const ops = useOperations();
  const { toast } = useToast();

  const [probe, setProbe] = useState<ToolchainProbeOut | null>(
    session.app?.probe ? { ...session.app.probe, managers: null } as unknown as ToolchainProbeOut : null,
  );
  const [probing, setProbing] = useState(false);
  const [versionDraft, setVersionDraft] = useState<Partial<Record<ToolKey, string>>>({});
  const [managerPick, setManagerPick] = useState<"auto" | "mise" | "winget">("auto");
  const [pending, setPending] = useState<Partial<Record<ToolKey, PendingOp>>>({});
  const pendingRef = useRef(pending);
  pendingRef.current = pending;
  const handledOps = useRef(new Set<string>());

  const refresh = async () => {
    setProbing(true);
    try {
      setProbe(await apiToolchainProbe());
    } catch (e) {
      toast(e instanceof IpcFailure ? e.message : String(e), "err");
    } finally {
      setProbing(false);
    }
  };

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // operation 终态：成功 → 重新探测（固定时同时刷新 yaml hash）；失败 → 给出下一步
  useEffect(() => {
    for (const op of ops.operations) {
      if (op.state !== "succeeded" && op.state !== "failed") continue;
      if (handledOps.current.has(op.operation_id)) continue;
      const entry = Object.entries(pendingRef.current).find(([, p]) => p?.opId === op.operation_id);
      if (!entry) continue;
      handledOps.current.add(op.operation_id);
      const tool = entry[0] as ToolKey;
      const { verb } = entry[1] as PendingOp;
      setPending((prev) => ({ ...prev, [tool]: null }));
      if (op.state === "succeeded") {
        void refresh();
        if (verb === "pin") void yaml.actions.reload();
        toast(`${tool} ${verb === "upgrade" ? "升级" : verb === "pin" ? "固定" : "安装"}完成`, "ok");
      } else {
        const label = opErrorLabel(op.error_code);
        toast(op.message ? `${label}（${op.message}）` : label, "err");
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ops.operations]);

  const wsTc = ws.state.spec?.toolchain ?? null;
  const managers: ManagerAvailability | null = probe?.managers ?? null;
  // npm/pnpm/yarn 入口只在当前工作区有 node 服务时出现（§15.1）
  const needsPkg =
    ws.state.workspaceId != null &&
    ws.state.spec != null &&
    Object.values(ws.state.spec.services).some((s) => s.kind === "node");

  const requiredVersion = (key: ToolKey): string | null => {
    if (!wsTc) return null;
    if (key === "java") return wsTc.java ?? null;
    if (key === "maven") return wsTc.maven ?? null;
    if (key === "node") return wsTc.node ?? null;
    if (key === "npm" || key === "pnpm" || key === "yarn") {
      return wsTc.package_manager === key ? key : null;
    }
    return null;
  };

  const startOp = async (key: ToolKey, verb: PendingOp["verb"]) => {
    if (pending[key]) return; // 禁止重复安装同一个工具（§15.1）
    const required = requiredVersion(key);
    const version = (verb === "pin" ? null : versionDraft[key]?.trim()) || required || undefined;
    const opts = {
      version: verb === "pin" ? probe?.[key]?.version ?? undefined : version,
      manager: managerPick,
      persist: verb === "pin",
      baseHash: verb === "pin" ? yaml.state.hash : null,
    };
    try {
      const out = verb === "upgrade" ? await apiToolchainUpgrade(key, opts) : await apiToolchainInstall(key, opts);
      handledOps.current.delete(out.operation_id);
      setPending((prev) => ({ ...prev, [key]: { opId: out.operation_id, verb } }));
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel((e as IpcFailure).code) : String(e), "err");
    }
  };

  const tools = [...CORE_TOOLS, ...(needsPkg ? PKG_TOOLS : [])];

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto p-4">
        <div className="mb-3 flex flex-wrap items-center gap-2">
          <h2 className="text-[0.95rem] font-semibold text-[var(--t1,#222326)]">环境与工具链</h2>
          <span className="text-[0.75rem] text-[var(--t3,#8a8f98)]">缺失工具可一键安装（mise / winget）</span>
          <div className="ml-auto flex items-center gap-2">
            {managers && (
              <>
                <Badge variant={managers.mise ? "default" : "outline"}>mise {managers.mise ? "可用" : "未安装"}</Badge>
                <Badge variant={managers.winget ? "default" : "outline"}>winget {managers.winget ? "可用" : "未安装"}</Badge>
              </>
            )}
            {wsTc?.manager && wsTc.manager !== "auto" && (
              <Badge variant="secondary">工作区指定 {wsTc.manager}</Badge>
            )}
            <Button variant="outline" size="sm" onClick={() => void refresh()} disabled={probing}>
              {probing ? <Loader2 className="size-4 animate-spin" /> : <RefreshCw className="size-4" />}
              重新探测
            </Button>
          </div>
        </div>

        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {tools.map((t) => (
            <ToolCard
              key={t.key}
              meta={t}
              found={probe?.[t.key] ?? null}
              required={requiredVersion(t.key)}
              defaultVersion={DEFAULT_VERSION[t.key]}
              versionDraft={versionDraft[t.key] ?? ""}
              onVersionDraft={(v) => setVersionDraft((prev) => ({ ...prev, [t.key]: v }))}
              managerPick={managerPick}
              managers={managers}
              onManagerPick={setManagerPick}
              pending={pending[t.key] ?? null}
              opState={pending[t.key] ? ops.get(pending[t.key]!.opId) : null}
              canPin={ws.state.workspaceId != null && yaml.state.hash !== ""}
              onInstall={() => void startOp(t.key, "install")}
              onUpgrade={() => void startOp(t.key, "upgrade")}
              onPin={() => void startOp(t.key, "pin")}
            />
          ))}
        </div>

        {!ws.state.workspaceId && (
          <p className="mt-3 text-[0.75rem] text-[var(--t3,#8a8f98)]">
            打开工作区后，可将版本要求固定到 supertask.yaml（需 base_hash，冲突时不写入）。
          </p>
        )}
      </div>
    </div>
  );
}

type ToolCardProps = {
  meta: { key: ToolKey; label: string; rec: string };
  found: ToolProbe | null;
  required: string | null;
  defaultVersion: string;
  versionDraft: string;
  onVersionDraft: (v: string) => void;
  managerPick: "auto" | "mise" | "winget";
  managers: ManagerAvailability | null;
  onManagerPick: (m: "auto" | "mise" | "winget") => void;
  pending: PendingOp | null;
  opState: ReturnType<ReturnType<typeof useOperations>["get"]>;
  canPin: boolean;
  onInstall: () => void;
  onUpgrade: () => void;
  onPin: () => void;
};

function ToolCard(p: ToolCardProps) {
  const isFound = p.found?.found === true;
  const busy = p.pending != null;
  const versionSource = p.required
    ? `工作区要求 ${p.required}`
    : `默认 ${p.defaultVersion}（可修改）`;
  return (
    <Card className="flex flex-col gap-2 p-3 transition-colors duration-150 hover:border-[var(--line-strong,#d0d6e0)]">
      <div className="flex items-center gap-3">
        <div
          className={cn(
            "flex size-9 items-center justify-center rounded-[var(--r-sm,8px)]",
            p.found == null
              ? "bg-[var(--surface-2,#f3f4f5)] text-[var(--t3,#8a8f98)]"
              : isFound
                ? "bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]"
                : "bg-[var(--st-warn-tint,#fff8e1)] text-[var(--st-warn,#9a6700)]",
          )}
        >
          {p.found == null ? (
            <Wrench className="size-5" />
          ) : isFound ? (
            <CheckCircle2 className="size-5" />
          ) : (
            <XCircle className="size-5" />
          )}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="font-semibold text-[var(--t1,#222326)]">{p.meta.label}</span>
            {p.required && <Badge variant="secondary">要求 {p.required}</Badge>}
          </div>
          <div className="truncate font-mono text-[0.66rem] text-[var(--t3,#8a8f98)]">
            {p.found == null
              ? "探测中…"
              : isFound
                ? `${p.found.version ?? "已安装"} · ${p.found.path ?? ""}`
                : `缺失 · 建议 ${p.meta.rec}`}
          </div>
        </div>
      </div>

      <div className="text-[0.7rem] text-[var(--t3,#8a8f98)]">版本来源：{versionSource}</div>

      {/* 操作区：安装（缺失）/ 升级 + 固定（已装）；运行中显示 operation 状态并禁用同工具按钮 */}
      {busy ? (
        <div className="flex items-center gap-2 rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f3f4f5)] px-2 py-1.5 text-[0.75rem] text-[var(--t2,#62666d)]">
          <Loader2 className="size-3.5 animate-spin" />
          <span className="min-w-0 flex-1 truncate">
            {p.pending!.verb === "upgrade" ? "升级中" : p.pending!.verb === "pin" ? "固定中" : "安装中"}
            {p.opState?.message ? ` · ${p.opState.message}` : ""}
            {p.opState?.progress != null ? ` ${Math.round(p.opState.progress * 100)}%` : ""}
          </span>
        </div>
      ) : (
        <div className="flex flex-wrap items-center gap-2">
          {isFound ? (
            <>
              <Button variant="outline" size="sm" onClick={p.onUpgrade} disabled={!p.managers || (!p.managers.mise && !p.managers.winget)}>
                <ArrowUpCircle className="size-3.5" /> 升级
              </Button>
              {p.canPin && (
                <Button variant="ghost" size="sm" onClick={p.onPin} title={`把当前版本写入工作区 toolchain`}>
                  <Pin className="size-3.5" /> 固定当前版本
                </Button>
              )}
            </>
          ) : (
            <>
              <Input
                className="h-8 w-24 font-mono text-[0.75rem]"
                value={p.versionDraft}
                placeholder={p.defaultVersion}
                onChange={(e) => p.onVersionDraft(e.target.value)}
                aria-label={`${p.meta.label} 版本`}
              />
              <select
                className="h-8 rounded-[var(--r-sm,8px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] px-1.5 text-[0.75rem] text-[var(--t1,#222326)]"
                value={p.managerPick}
                onChange={(e) => p.onManagerPick(e.target.value as "auto" | "mise" | "winget")}
                aria-label="安装 provider"
              >
                <option value="auto">自动选择</option>
                <option value="mise" disabled={p.managers != null && !p.managers.mise}>
                  mise{p.managers && !p.managers.mise ? "（未安装）" : ""}
                </option>
                <option value="winget" disabled={p.managers != null && !p.managers.winget}>
                  winget{p.managers && !p.managers.winget ? "（未安装）" : ""}
                </option>
              </select>
              <Button variant="default" size="sm" onClick={p.onInstall} disabled={p.managers != null && !p.managers.mise && !p.managers.winget}>
                <Download className="size-3.5" /> 安装
              </Button>
              {p.canPin && p.required && (
                <Button variant="ghost" size="sm" onClick={p.onPin} title={`安装并固定为工作区要求（${p.required}）`}>
                  <Pin className="size-3.5" /> 安装并固定
                </Button>
              )}
            </>
          )}
        </div>
      )}

      {/* 失败的下一步提示（§15.1：权限/版本/网络/PATH 显示具体动作） */}
      {p.opState?.state === "failed" && (
        <div className="rounded-[var(--r-sm,8px)] bg-[var(--st-danger-tint,#fdeeee)] px-2 py-1.5 text-[0.72rem] leading-relaxed text-[var(--st-danger,#c03535)]">
          {opErrorLabel(p.opState.error_code)}
          {p.opState.message ? <div className="mt-0.5 text-[var(--t3,#8a8f98)]">{p.opState.message}</div> : null}
        </div>
      )}
    </Card>
  );
}
