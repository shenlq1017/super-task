import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
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
const PKG_TOOLS: { key: ToolKey; label: string; recKey: string }[] = [
  { key: "npm", label: "npm", recKey: "withNode" },
  { key: "pnpm", label: "pnpm", recKey: "9" },
  { key: "yarn", label: "Yarn", recKey: "1" },
];

type PendingOp = { opId: string; verb: "install" | "upgrade" | "pin" };

export function EnvPage() {
  const { state: session } = useSession();
  const ws = useWorkspace();
  const yaml = useYaml();
  const ops = useOperations();
  const { toast } = useToast();
  const { t } = useTranslation();

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
        toast(t("pages.env.opDone", { tool, verb: t(`pages.env.verb_${verb}`) }), "ok");
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
          <h2 className="text-[0.95rem] font-semibold text-[var(--t1,#222326)]">{t("pages.env.title")}</h2>
          <span className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.env.subtitle")}</span>
          <div className="ml-auto flex items-center gap-2">
            {managers && (
              <>
                <Badge variant={managers.mise ? "default" : "outline"}>
                  mise {managers.mise ? t("pages.env.available") : t("pages.env.notInstalled")}
                </Badge>
                <Badge variant={managers.winget ? "default" : "outline"}>
                  winget {managers.winget ? t("pages.env.available") : t("pages.env.notInstalled")}
                </Badge>
              </>
            )}
            {wsTc?.manager && wsTc.manager !== "auto" && (
              <Badge variant="secondary">{t("pages.env.workspaceManager", { manager: wsTc.manager })}</Badge>
            )}
            <Button variant="soft" size="sm" onClick={() => void refresh()} disabled={probing}>
              {probing ? <Loader2 className="size-4 animate-spin" /> : <RefreshCw className="size-4" />}
              {t("pages.env.reprobe")}
            </Button>
          </div>
        </div>

        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {tools.map((tool) => (
            <ToolCard
              key={tool.key}
              meta={tool}
              found={probe?.[tool.key] ?? null}
              required={requiredVersion(tool.key)}
              defaultVersion={DEFAULT_VERSION[tool.key]}
              versionDraft={versionDraft[tool.key] ?? ""}
              onVersionDraft={(v) => setVersionDraft((prev) => ({ ...prev, [tool.key]: v }))}
              managerPick={managerPick}
              managers={managers}
              onManagerPick={setManagerPick}
              pending={pending[tool.key] ?? null}
              opState={pending[tool.key] ? ops.get(pending[tool.key]!.opId) : null}
              canPin={ws.state.workspaceId != null && yaml.state.hash !== ""}
              onInstall={() => void startOp(tool.key, "install")}
              onUpgrade={() => void startOp(tool.key, "upgrade")}
              onPin={() => void startOp(tool.key, "pin")}
            />
          ))}
        </div>

        {/* 1.4 §11.2：Gradle 仅信息展示（wrapper 是唯一推荐执行方式），不提供安装 */}
        {probe?.gradle && (
          <Card className="mt-3 flex flex-wrap items-center gap-2 p-3">
            <Wrench className="size-4 shrink-0 text-[var(--t3,#8a8f98)]" />
            <span className="text-[0.88rem] font-semibold text-[var(--t1,#222326)]">Gradle</span>
            {probe.gradle.found ? (
              <>
                <Badge variant="outline">{probe.gradle.version ?? t("pages.env.available")}</Badge>
                {probe.gradle.path && (
                  <span className="truncate font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]">{probe.gradle.path}</span>
                )}
              </>
            ) : (
              <Badge variant="outline">{t("pages.env.gradleMissing")}</Badge>
            )}
            <span className="ml-auto text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.env.gradleHint")}</span>
          </Card>
        )}

        {!ws.state.workspaceId && (
          <p className="mt-3 text-[0.75rem] text-[var(--t3,#8a8f98)]">
            {t("pages.env.pinHint")}
          </p>
        )}
      </div>
    </div>
  );
}

type ToolCardProps = {
  meta: { key: ToolKey; label: string; rec?: string; recKey?: string };
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
  const { t } = useTranslation();
  const isFound = p.found?.found === true;
  const busy = p.pending != null;
  const versionSource = p.required
    ? t("pages.env.sourceRequired", { version: p.required })
    : t("pages.env.sourceDefault", { version: p.defaultVersion });
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
            {p.required && <Badge variant="secondary">{t("pages.env.requiredBadge", { version: p.required })}</Badge>}
          </div>
          <div className="truncate font-mono text-[0.66rem] text-[var(--t3,#8a8f98)]">
            {p.found == null
              ? t("pages.env.probing")
              : isFound
                ? `${p.found.version ?? t("pages.env.installed")} · ${p.found.path ?? ""}`
                : `${t("pages.env.missing")} · ${t("pages.env.recommended", { version: p.meta.recKey === "withNode" ? t("pages.env.recWithNode") : p.meta.recKey })}`}
          </div>
        </div>
      </div>

      <div className="text-[0.7rem] text-[var(--t3,#8a8f98)]">{t("pages.env.versionSource")} {versionSource}</div>

      {/* 操作区：安装（缺失）/ 升级 + 固定（已装）；运行中显示 operation 状态并禁用同工具按钮 */}
      {busy ? (
        <div className="flex items-center gap-2 rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f3f4f5)] px-2 py-1.5 text-[0.75rem] text-[var(--t2,#62666d)]">
          <Loader2 className="size-3.5 animate-spin" />
          <span className="min-w-0 flex-1 truncate">
            {t(`pages.env.busy_${p.pending!.verb}`)}
            {p.opState?.message ? ` · ${p.opState.message}` : ""}
            {p.opState?.progress != null ? ` ${Math.round(p.opState.progress * 100)}%` : ""}
          </span>
        </div>
      ) : (
        <div className="flex flex-wrap items-center gap-2">
          {isFound ? (
            <>
              <Button variant="warn" size="sm" onClick={p.onUpgrade} disabled={!p.managers || (!p.managers.mise && !p.managers.winget)}>
                <ArrowUpCircle className="size-3.5" /> {t("pages.env.upgrade")}
              </Button>
              {p.canPin && (
                <Button variant="outline" size="sm" onClick={p.onPin} title={t("pages.env.pinCurrentTitle")}>
                  <Pin className="size-3.5" /> {t("pages.env.pinCurrent")}
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
                aria-label={t("pages.env.versionAria", { tool: p.meta.label })}
              />
              <select
                className="h-8 cursor-pointer rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] px-1.5 text-[0.75rem] text-[var(--t1,#222326)]"
                value={p.managerPick}
                onChange={(e) => p.onManagerPick(e.target.value as "auto" | "mise" | "winget")}
                aria-label={t("pages.env.managerAria")}
              >
                <option value="auto">{t("pages.env.managerAuto")}</option>
                <option value="mise" disabled={p.managers != null && !p.managers.mise}>
                  mise{p.managers && !p.managers.mise ? t("pages.env.notInstalledSuffix") : ""}
                </option>
                <option value="winget" disabled={p.managers != null && !p.managers.winget}>
                  winget{p.managers && !p.managers.winget ? t("pages.env.notInstalledSuffix") : ""}
                </option>
              </select>
              <Button variant="default" size="sm" onClick={p.onInstall} disabled={p.managers != null && !p.managers.mise && !p.managers.winget}>
                <Download className="size-3.5" /> {t("pages.env.install")}
              </Button>
              {p.canPin && p.required && (
                <Button variant="ghost" size="sm" onClick={p.onPin} title={t("pages.env.installPinTitle", { version: p.required })}>
                  <Pin className="size-3.5" /> {t("pages.env.installPin")}
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
