import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Loader2,
  Lock,
  Network,
  RefreshCw,
  Search,
  XCircle,
  KeyRound,
  ArrowUpFromLine,
  Download,
  Settings2,
} from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useSession } from "../providers/session-provider";
import { useWorkspace } from "../providers/workspace-provider";
import { useYaml } from "@/providers/yaml-provider";
import { useOperations } from "../providers/operation-provider";
import { useToast } from "@/components/ui/toast";
import { useUnsavedEntry } from "@/providers/unsaved-guard";
import { apiToolchainInstall, apiToolchainProbe, apiToolchainUpgrade, apiToolchainVersions, apiYamlSaveForm } from "../ipc/api";
import { IpcFailure, type DiscoveredInstall, type ManagerAvailability, type NetworkSpec, type SuperTaskFile, type ToolProbe, type ToolchainProbeOut } from "../ipc/protocol";
import { opErrorLabel } from "@/lib/status";
import { errorDisplayText } from "@/lib/error-messages";
import { cn } from "@/lib/utils";

type ToolKey = "java" | "maven" | "node" | "npm" | "pnpm" | "yarn" | "bun" | "python" | "go";
type ManagerPick = "auto" | "mise" | "winget";
type ActionDialog = { tool: ToolKey; verb: "install" | "upgrade" } | null;

const LS_MANAGER = "st:env.managerPick";
const LS_NETWORK_COLLAPSED = "st:env.networkCollapsed";

function versionMatches(want: string | null | undefined, have: string | null | undefined): boolean {
  if (!want || !have) return false;
  if (want === have) return true;
  return have.startsWith(want + ".");
}

function loadManagerPick(): ManagerPick {
  try {
    const v = localStorage.getItem(LS_MANAGER);
    if (v === "auto" || v === "mise" || v === "winget") return v;
  } catch {
    /* ignore */
  }
  return "auto";
}

function saveManagerPick(m: ManagerPick) {
  try {
    localStorage.setItem(LS_MANAGER, m);
  } catch {
    /* ignore */
  }
}

function loadNetworkCollapsed(): boolean {
  try {
    return localStorage.getItem(LS_NETWORK_COLLAPSED) !== "0";
  } catch {
    return true;
  }
}

function saveNetworkCollapsed(collapsed: boolean) {
  try {
    localStorage.setItem(LS_NETWORK_COLLAPSED, collapsed ? "1" : "0");
  } catch {
    /* ignore */
  }
}

function fmtProbeTime(ts: number | null): string {
  if (ts == null) return "\u2014";
  return new Date(ts).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/** 客户端镜像后端 manifest 默认版本（§4.3 版本来源第 3 级）。 */
const DEFAULT_VERSION: Record<ToolKey, string> = {
  java: "21",
  maven: "3.9",
  node: "20",
  npm: "20",
  pnpm: "9",
  yarn: "1",
  bun: "1",
  python: "3.12",
  go: "1.23",
};

const CORE_TOOLS: { key: ToolKey; label: string; rec: string }[] = [
  { key: "java", label: "JDK", rec: "21 LTS" },
  { key: "maven", label: "Maven", rec: "3.9" },
  { key: "node", label: "Node.js", rec: "20 LTS" },
  // 1.7 §5：python / go（探测 + 一键安装，链路复用 mise/winget）
  { key: "python", label: "Python", rec: "3.12" },
  { key: "go", label: "Go", rec: "1.23" },
];

/** npm/pnpm/yarn/bun 只在当前工作区有 node 服务时出现（§15.1）。 */
const PKG_TOOLS: { key: ToolKey; label: string; recKey: string }[] = [
  { key: "npm", label: "npm", recKey: "withNode" },
  { key: "pnpm", label: "pnpm", recKey: "9" },
  { key: "yarn", label: "Yarn", recKey: "1" },
  { key: "bun", label: "Bun", recKey: "1" },
];

type PendingOp = { opId: string; verb: "install" | "upgrade" | "pin" };

/** P1：安装来源徽标文案 key（与后端 InstallSource 序列化值一一对应）。 */
const INSTALL_SOURCE_KEYS: Record<DiscoveredInstall["source"], string> = {
  registry: "pages.env.srcRegistry",
  directory: "pages.env.srcDirectory",
  env_var: "pages.env.srcEnvVar",
  nvm_dir: "pages.env.srcNvmDir",
};


/** Searchable floating version list (avoids native select crowding the card). */
function VersionCombobox(props: {
  value: string;
  options: string[];
  onChange: (v: string) => void;
  ariaLabel: string;
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return props.options;
    return props.options.filter((o) => o.toLowerCase().includes(q));
  }, [props.options, query]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    window.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  useEffect(() => {
    if (open) {
      setQuery("");
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  return (
    <div ref={rootRef} className="relative min-w-0 flex-1">
      <button
        type="button"
        disabled={props.disabled}
        aria-label={props.ariaLabel}
        aria-expanded={open}
        aria-haspopup="listbox"
        onClick={() => setOpen((v) => !v)}
        className={cn(
          "flex h-9 w-full items-center gap-1.5 rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] px-2.5 font-mono text-[0.8rem] text-[var(--t1,#222326)]",
          "hover:border-[var(--st-accent,#5e6ad2)] focus-visible:outline-2 focus-visible:outline-[var(--st-accent,#5e6ad2)]",
          "disabled:cursor-not-allowed disabled:opacity-50",
        )}
      >
        <span className="min-w-0 flex-1 truncate text-left">{props.value || "\u2014"}</span>
        <ChevronDown className={cn("size-3.5 shrink-0 text-[var(--t3,#8a8f98)] transition-transform", open && "rotate-180")} />
      </button>
      {open && (
        <div
          role="listbox"
          className="absolute left-0 right-0 z-50 mt-1 overflow-hidden rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] shadow-lg ring-1 ring-black/5"
        >
          <div className="flex items-center gap-1.5 border-b border-[var(--line,#e6e6e6)] px-2 py-1.5">
            <Search className="size-3.5 shrink-0 text-[var(--t3,#8a8f98)]" />
            <input
              ref={inputRef}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t("pages.env.versionSearch")}
              className="min-w-0 flex-1 bg-transparent text-[0.78rem] text-[var(--t1,#222326)] outline-none placeholder:text-[var(--t3,#8a8f98)]"
              aria-label={t("pages.env.versionSearch")}
            />
          </div>
          <ul className="max-h-48 overflow-auto py-1">
            {filtered.length === 0 ? (
              <li className="px-3 py-2 text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.env.versionEmpty")}</li>
            ) : (
              filtered.map((v) => {
                const selected = v === props.value;
                return (
                  <li key={v}>
                    <button
                      type="button"
                      role="option"
                      aria-selected={selected}
                      className={cn(
                        "flex w-full items-center gap-2 px-3 py-1.5 text-left font-mono text-[0.78rem] hover:bg-[var(--surface-2,#f3f4f5)]",
                        selected && "bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]",
                      )}
                      onClick={() => {
                        props.onChange(v);
                        setOpen(false);
                      }}
                    >
                      <span className="min-w-0 flex-1 truncate">{v}</span>
                      {selected && <Check className="size-3.5 shrink-0" />}
                    </button>
                  </li>
                );
              })
            )}
          </ul>
        </div>
      )}
    </div>
  );
}

function ToolSkeleton() {
  return (
    <Card className="flex flex-col gap-3 p-3">
      <div className="flex items-center gap-3">
        <div className="size-9 animate-pulse rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f3f4f5)]" />
        <div className="min-w-0 flex-1 space-y-2">
          <div className="h-3.5 w-20 animate-pulse rounded bg-[var(--surface-2,#f3f4f5)]" />
          <div className="h-2.5 w-40 animate-pulse rounded bg-[var(--surface-2,#f3f4f5)]" />
        </div>
      </div>
      <div className="h-8 animate-pulse rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f3f4f5)]" />
    </Card>
  );
}

export function EnvPage() {
  const { state: session } = useSession();
  const ws = useWorkspace();
  const yaml = useYaml();
  const ops = useOperations();
  const { toast } = useToast();
  const { t } = useTranslation();

  const [probe, setProbe] = useState<ToolchainProbeOut | null>(
    session.app?.probe ? { ...session.app.probe, managers: null } : null,
  );
  const [probing, setProbing] = useState(false);
  /** S1：每工具可选版本（后端白名单 ∪ mise ls-remote）；拉取失败降级为仅默认版本。 */
  const [versions, setVersions] = useState<Record<string, string[]> | null>(null);
  const [versionDraft, setVersionDraft] = useState<Partial<Record<ToolKey, string>>>({});
  const [managerPick, setManagerPickState] = useState<ManagerPick>(loadManagerPick);
  const [pending, setPending] = useState<Partial<Record<ToolKey, PendingOp | null>>>({});
  const [lastProbeAt, setLastProbeAt] = useState<number | null>(() => (session.app?.probe ? Date.now() : null));
  const [probeError, setProbeError] = useState<string | null>(null);
  const [actionDialog, setActionDialog] = useState<ActionDialog>(null);
  const [installsDialogTool, setInstallsDialogTool] = useState<ToolKey | null>(null);
  const pendingRef = useRef(pending);
  pendingRef.current = pending;
  const handledOps = useRef(new Set<string>());
  const refreshGen = useRef(0);

  const setManagerPick = (m: ManagerPick) => {
    setManagerPickState(m);
    saveManagerPick(m);
  };

  // D1：后端会话内 TTL 缓存——进页走缓存，手动「重新探测」才强制
  const refresh = async (force = false) => {
    const gen = ++refreshGen.current;
    setProbing(true);
    try {
      const next = await apiToolchainProbe(force);
      if (gen !== refreshGen.current) return;
      setProbe(next);
      setProbeError(null);
      setLastProbeAt(Date.now());
    } catch (e) {
      if (gen !== refreshGen.current) return;
      const msg = e instanceof IpcFailure ? e.message : String(e);
      setProbeError(msg);
      toast(msg, "err");
    } finally {
      if (gen === refreshGen.current) setProbing(false);
    }
  };

  const loadVersions = async () => {
    try {
      setVersions((await apiToolchainVersions()).tools);
    } catch {
      // 锦上添花项：失败时下拉只给默认版本，不弹错误
    }
  };

  useEffect(() => {
    void refresh();
    void loadVersions();
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
        void refresh(true);
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
    // 1.7：python/go 钉扎（major.minor）
    if (key === "python") return wsTc.python ?? null;
    if (key === "go") return wsTc.go ?? null;
    if (key === "npm" || key === "pnpm" || key === "yarn" || key === "bun") {
      return wsTc.package_manager === key ? key : null;
    }
    return null;
  };

  /** P2：点击已装版本即选用——写 toolchain[node|java] 并持久化，启动时经 apply_pinned_version_env 解析。 */
  const pinInstall = async (key: ToolKey, version: string) => {
    const spec = ws.state.spec;
    const hash = yaml.state.hash;
    if (!spec || !ws.state.workspaceId || !hash) return; // canPin 前置
    const field = key === "java" ? ("java" as const) : key === "node" ? ("node" as const) : null;
    if (!field) return;
    const next: SuperTaskFile = {
      ...spec,
      toolchain: { ...(spec.toolchain ?? {}), [field]: version },
    };
    try {
      await apiYamlSaveForm(next, hash);
      await yaml.actions.reload();
      void refresh(true); // active 标记可能随钉扎变化
      toast(t("pages.env.pinInstalled", { version }), "ok");
    } catch (e) {
      toast(e instanceof IpcFailure ? errorDisplayText(e.code, e.message) : String(e), "err");
    }
  };

  const startOp = async (key: ToolKey, verb: PendingOp["verb"], overrideVersion?: string) => {
    if (pending[key]) return; // 禁止重复安装同一个工具（§15.1）
    const required = requiredVersion(key);
    const version =
      (verb === "pin" ? null : (overrideVersion ?? versionDraft[key])?.trim()) || required || undefined;
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
      setActionDialog(null);
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel((e as IpcFailure).code) : String(e), "err");
    }
  };

  const versionOptionsFor = (key: ToolKey, required: string | null): string[] => {
    const opts = [...(versions?.[key] ?? [])];
    if (opts.length === 0) opts.push(DEFAULT_VERSION[key]);
    if (required && !opts.includes(required)) opts.unshift(required);
    return opts;
  };

  const tools = [...CORE_TOOLS, ...(needsPkg ? PKG_TOOLS : [])];

  const health = useMemo(() => {
    let found = 0;
    let total = 0;
    for (const tool of tools) {
      total += 1;
      if (probe?.[tool.key]?.found) found += 1;
    }
    return { found, total };
  }, [tools, probe]);

  const showSkeletons = probing && probe == null;
  const canPinWs = ws.state.workspaceId != null && yaml.state.hash !== "";

  const actionTool = actionDialog?.tool ?? null;
  const actionMeta = actionTool ? tools.find((x) => x.key === actionTool) : null;
  const actionRequired = actionTool ? requiredVersion(actionTool) : null;
  const actionVersion =
    actionTool != null
      ? versionDraft[actionTool] || actionRequired || DEFAULT_VERSION[actionTool]
      : "";
  const installsForDialog =
    installsDialogTool != null
      ? (probe?.installs ?? []).filter((i) => i.tool === installsDialogTool)
      : [];

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto p-4">
        <Card className="mb-3 p-3 sm:p-4">
          <div className="flex flex-wrap items-start gap-3">
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center gap-2">
                <h2 className="text-[0.95rem] font-semibold text-[var(--t1,#222326)]">{t("pages.env.title")}</h2>
                <span className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.env.subtitle")}</span>
              </div>
              <div className="mt-2 flex flex-wrap items-center gap-2">
                <span
                  className={cn(
                    "inline-flex h-6 items-center gap-1.5 rounded-full px-2.5 text-[0.72rem] font-semibold",
                    probe == null
                      ? "bg-[var(--surface-2,#f3f4f5)] text-[var(--t3,#8a8f98)]"
                      : health.found === health.total
                        ? "bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]"
                        : "bg-[var(--st-warn-tint,#fff8e1)] text-[var(--st-warn,#9a6700)]",
                  )}
                  title={t("pages.env.healthTitle")}
                >
                  <span
                    aria-hidden
                    className="size-1.5 rounded-full"
                    style={{
                      background:
                        probe == null
                          ? "var(--t3)"
                          : health.found === health.total
                            ? "var(--st-ok)"
                            : "var(--st-warn)",
                    }}
                  />
                  {probe == null
                    ? t("pages.env.probing")
                    : t("pages.env.healthSummary", { found: health.found, total: health.total })}
                </span>
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
                <span className="font-mono text-[0.7rem] text-[var(--t3,#8a8f98)]">
                  {t("pages.env.lastProbe", { time: fmtProbeTime(lastProbeAt) })}
                </span>
              </div>
            </div>
            <Button
              variant="soft"
              size="sm"
              className="shrink-0 gap-1"
              onClick={() => void refresh(true)}
              disabled={probing}
              title={t("pages.env.forceRefreshTitle")}
            >
              {probing ? <Loader2 className="size-4 animate-spin" /> : <RefreshCw className="size-4" />}
              {t("pages.env.reprobe")}
            </Button>
          </div>
        </Card>

        {probeError && probe == null && !probing && (
          <Card className="mb-3 flex flex-col items-start gap-2 border-[rgb(192_53_53_/_0.25)] bg-[var(--st-danger-tint,#fdeeee)] p-4">
            <div className="flex items-center gap-2 text-[0.875rem] font-semibold text-[var(--st-danger,#c03535)]">
              <XCircle className="size-4 shrink-0" />
              {t("pages.env.probeFailedTitle")}
            </div>
            <p className="text-[0.78rem] leading-relaxed text-[var(--t2,#62666d)]">{probeError}</p>
            <p className="text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.env.probeFailedHint")}</p>
            <Button variant="soft" size="sm" className="gap-1" onClick={() => void refresh(true)}>
              <RefreshCw className="size-3.5" /> {t("common.retry")}
            </Button>
          </Card>
        )}

        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
          {showSkeletons
            ? tools.map((tool) => <ToolSkeleton key={tool.key} />)
            : tools.map((tool) => (
                <ToolCard
                  key={tool.key}
                  meta={tool}
                  found={probe?.[tool.key] ?? null}
                  installs={(probe?.installs ?? []).filter((i) => i.tool === tool.key)}
                  required={requiredVersion(tool.key)}
                  defaultVersion={DEFAULT_VERSION[tool.key]}
                  pending={pending[tool.key] ?? null}
                  opState={pending[tool.key] ? ops.get(pending[tool.key]!.opId) : null}
                  canPin={canPinWs}
                  probing={probing && probe == null}
                  onOpenInstall={() => {
                    if (!versionDraft[tool.key]) {
                      setVersionDraft((prev) => ({
                        ...prev,
                        [tool.key]: requiredVersion(tool.key) || DEFAULT_VERSION[tool.key],
                      }));
                    }
                    setActionDialog({ tool: tool.key, verb: "install" });
                  }}
                  onOpenUpgrade={() => {
                    if (!versionDraft[tool.key]) {
                      setVersionDraft((prev) => ({
                        ...prev,
                        [tool.key]:
                          requiredVersion(tool.key) ||
                          probe?.[tool.key]?.version ||
                          DEFAULT_VERSION[tool.key],
                      }));
                    }
                    setActionDialog({ tool: tool.key, verb: "upgrade" });
                  }}
                  onOpenInstalls={() => setInstallsDialogTool(tool.key)}
                  onPin={() => void startOp(tool.key, "pin")}
                  onPinDetected={() => {
                    const v = probe?.[tool.key]?.version;
                    if (v && (tool.key === "java" || tool.key === "node")) void pinInstall(tool.key, v);
                    else void startOp(tool.key, "pin");
                  }}
                />
              ))}
        </div>

        {/* 1.4 §11.2：Gradle 仅信息展示（wrapper 是唯一推荐执行方式），不提供安装 */}
        {probe?.gradle && (
          <Card className="mt-3 flex flex-wrap items-center gap-2 p-3">
            <KeyRound className="size-4 shrink-0 text-[var(--t3,#8a8f98)]" />
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

        {/* 1.7 §7：网络（代理 + 镜像）——写入 workspace network 段，启动时注入 env */}
        <NetworkCard />
      </div>

      <Dialog open={actionDialog != null} onOpenChange={(o) => !o && setActionDialog(null)}>
        <DialogContent className="sm:max-w-md" showCloseButton>
          <DialogHeader>
            <DialogTitle>
              {actionDialog?.verb === "upgrade"
                ? t("pages.env.dialogUpgradeTitle", { tool: actionMeta?.label ?? actionTool })
                : t("pages.env.dialogInstallTitle", { tool: actionMeta?.label ?? actionTool })}
            </DialogTitle>
            <DialogDescription>
              {actionDialog?.verb === "upgrade" ? t("pages.env.dialogUpgradeDesc") : t("pages.env.dialogInstallDesc")}
            </DialogDescription>
          </DialogHeader>
          {actionTool && (
            <div className="space-y-3">
              <div>
                <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.env.versionLabel")}</label>
                <div className="mt-1">
                  <VersionCombobox
                    value={actionVersion}
                    options={versionOptionsFor(actionTool, actionRequired)}
                    onChange={(v) => setVersionDraft((prev) => ({ ...prev, [actionTool]: v }))}
                    ariaLabel={t("pages.env.versionAria", { tool: actionMeta?.label ?? actionTool })}
                  />
                </div>
                <p className="mt-1 text-[0.7rem] text-[var(--t3,#8a8f98)]">
                  {actionRequired
                    ? t("pages.env.sourceRequired", { version: actionRequired })
                    : t("pages.env.sourceDefault", { version: DEFAULT_VERSION[actionTool] })}
                </p>
              </div>
              <div>
                <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.env.managerAria")}</label>
                <select
                  className="mt-1 h-9 w-full cursor-pointer rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] px-2 text-[0.8rem] text-[var(--t1,#222326)]"
                  value={managerPick}
                  onChange={(e) => setManagerPick(e.target.value as ManagerPick)}
                  aria-label={t("pages.env.managerAria")}
                >
                  <option value="auto">{t("pages.env.managerAuto")}</option>
                  <option value="mise" disabled={managers != null && !managers.mise}>
                    mise{managers && !managers.mise ? t("pages.env.notInstalledSuffix") : ""}
                  </option>
                  <option value="winget" disabled={managers != null && !managers.winget}>
                    winget{managers && !managers.winget ? t("pages.env.notInstalledSuffix") : ""}
                  </option>
                </select>
                <p className="mt-1 text-[0.7rem] text-[var(--t3,#8a8f98)]">{t("pages.env.managerRemember")}</p>
              </div>
              {managers && !managers.mise && !managers.winget && (
                <div className="rounded-[var(--r-sm,8px)] bg-[var(--st-warn-tint,#fff8e1)] px-2.5 py-2 text-[0.75rem] leading-relaxed text-[var(--st-warn,#9a6700)]">
                  {t("pages.env.noManagerHint")}
                </div>
              )}
            </div>
          )}
          <DialogFooter className="gap-2 sm:justify-end">
            <Button variant="outline" size="sm" onClick={() => setActionDialog(null)}>
              {t("common.cancel")}
            </Button>
            {actionDialog?.verb === "install" && canPinWs && actionRequired && (
              <Button
                variant="ghost"
                size="sm"
                className="gap-1"
                disabled={managers != null && !managers.mise && !managers.winget}
                onClick={() => void startOp(actionTool!, "pin")}
                title={t("pages.env.installPinTitle", { version: actionRequired })}
              >
                <Lock className="size-3.5" /> {t("pages.env.installPin")}
              </Button>
            )}
            <Button
              variant={actionDialog?.verb === "upgrade" ? "warn" : "default"}
              size="sm"
              className="gap-1"
              disabled={
                !actionTool ||
                !!pending[actionTool] ||
                (managers != null && !managers.mise && !managers.winget)
              }
              onClick={() => {
                if (!actionDialog) return;
                void startOp(actionDialog.tool, actionDialog.verb, actionVersion);
              }}
            >
              {actionDialog?.verb === "upgrade" ? (
                <>
                  <ArrowUpFromLine className="size-3.5" /> {t("pages.env.upgrade")}
                </>
              ) : (
                <>
                  <Download className="size-3.5" /> {t("pages.env.install")}
                </>
              )}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={installsDialogTool != null} onOpenChange={(o) => !o && setInstallsDialogTool(null)}>
        <DialogContent className="sm:max-w-lg" showCloseButton>
          <DialogHeader>
            <DialogTitle>
              {t("pages.env.installsDialogTitle", {
                tool: tools.find((x) => x.key === installsDialogTool)?.label ?? installsDialogTool,
                count: installsForDialog.length,
              })}
            </DialogTitle>
            <DialogDescription>{t("pages.env.installsDialogDesc")}</DialogDescription>
          </DialogHeader>
          <ul className="max-h-72 space-y-1 overflow-auto">
            {installsForDialog.map((i) => {
              const selected = versionMatches(requiredVersion(installsDialogTool!), i.version);
              return (
                <li
                  key={`${i.version}-${i.home}`}
                  className={cn(
                    "flex flex-wrap items-center gap-1.5 rounded-[var(--r-sm,8px)] border border-[var(--line,#e6e6e6)] px-2.5 py-2",
                    selected && "border-[rgb(39_166_68_/_0.35)] bg-[var(--st-ok-tint,#e9f7ed)]",
                  )}
                >
                  <span className="shrink-0 font-mono text-[0.8rem] font-semibold text-[var(--t1,#222326)]">{i.version}</span>
                  {i.active && <Badge variant="default">{t("pages.env.installActive")}</Badge>}
                  {selected && (
                    <Badge className="border-[rgb(39_166_68_/_0.3)] bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]">
                      {t("pages.env.selectedVersion")}
                    </Badge>
                  )}
                  <Badge variant="outline">{t(INSTALL_SOURCE_KEYS[i.source])}</Badge>
                  <span className="min-w-0 flex-1 truncate font-mono text-[0.68rem] text-[var(--t3,#8a8f98)]" title={i.home}>
                    {i.home}
                  </span>
                  {canPinWs && (
                    <Button
                      variant={selected ? "ghost" : "soft"}
                      size="sm"
                      className="h-7 shrink-0 px-2 text-[0.7rem]"
                      onClick={() => {
                        void pinInstall(installsDialogTool!, i.version);
                        setInstallsDialogTool(null);
                      }}
                      title={t("pages.env.installSelectTitle")}
                    >
                      {t("pages.env.installSelect")}
                    </Button>
                  )}
                </li>
              );
            })}
          </ul>
        </DialogContent>
      </Dialog>
    </div>
  );
}

/** 1.7 §7：网络卡（此前网络配置只有 spec 无 UI）。保存走 yaml.saveForm（带 base_hash）。 */
function NetworkCard() {
  const ws = useWorkspace();
  const yaml = useYaml();
  const { t } = useTranslation();
  const { toast } = useToast();
  const spec = ws.state.spec;
  const net = spec?.network ?? null;
  const [draft, setDraft] = useState<NetworkSpec>(() => net ?? {});
  // no_proxy 是数组，文本态单独存草稿（逗号分隔），避免逐键输入被数组规整吃掉
  const [noProxyText, setNoProxyText] = useState(() => (net?.proxy?.no_proxy ?? []).join(", "));
  const [busy, setBusy] = useState(false);
  const [collapsed, setCollapsed] = useState(loadNetworkCollapsed);

  useEffect(() => {
    setDraft(net ?? {});
    setNoProxyText((net?.proxy?.no_proxy ?? []).join(", "));
  }, [ws.state.workspaceId, yaml.state.hash]); // eslint-disable-line react-hooks/exhaustive-deps

  const set = (patch: NetworkSpec) => setDraft((prev) => ({ ...prev, ...patch }));

  const toggleCollapsed = () => {
    setCollapsed((c) => {
      const next = !c;
      saveNetworkCollapsed(next);
      return next;
    });
  };

  const save = async (): Promise<boolean> => {
    if (!spec || !ws.state.workspaceId) return false;
    setBusy(true);
    try {
      const noProxy = noProxyText.split(",").map((s) => s.trim()).filter(Boolean);
      const network: NetworkSpec = {
        ...draft,
        proxy: { ...(draft.proxy ?? {}), no_proxy: noProxy.length ? noProxy : undefined },
      };
      await apiYamlSaveForm({ ...spec, network }, yaml.state.hash);
      await yaml.actions.reload();
      toast(t("pages.env.networkSaved"), "ok");
      return true;
    } catch (e) {
      toast(e instanceof IpcFailure ? errorDisplayText(e.code, e.message) : String(e), "err");
      return false;
    } finally {
      setBusy(false);
    }
  };

  // 未保存守卫：网络草稿与 spec 有差异即视为脏（含 no_proxy 文本）
  useUnsavedEntry(
    "env.network",
    () =>
      JSON.stringify(draft) !== JSON.stringify(net ?? {}) ||
      noProxyText !== (net?.proxy?.no_proxy ?? []).join(", "),
    save,
  );

  const hasWs = ws.state.workspaceId != null;
  const proxy = draft.proxy ?? {};
  const inputCls = "mt-1 w-full rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] px-2 py-1 font-mono text-[0.75rem] text-[var(--t1,#222326)] focus-visible:outline-2 focus-visible:outline-[var(--st-accent,#5e6ad2)]";
  return (
    <Card className="mt-3 overflow-hidden p-4" data-env-network="1">
      <div className="mb-3 flex items-center gap-2">
        <button type="button" className="inline-flex items-center gap-1 text-[var(--t2,#62666d)]" aria-expanded={!collapsed} onClick={toggleCollapsed}>
          {collapsed ? <ChevronRight className="size-4" /> : <ChevronDown className="size-4" />}
          <Network className="size-4" />
        </button>
        <h3 className="text-[0.875rem] font-semibold text-[var(--t1,#222326)]">{t("pages.env.networkTitle")}</h3>
        <Badge variant="soon">{t("pages.env.advanced")}</Badge>
        <Button variant="success" size="sm" className="ml-auto gap-1" onClick={() => void save()} disabled={busy || !hasWs || collapsed}>
          {t("common.save")}
        </Button>
      </div>
      {!collapsed && (
      <>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
        <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
          {t("pages.env.proxyMode")}
          <select
            className={cn(inputCls, "cursor-pointer")}
            value={proxy.mode ?? "off"}
            onChange={(e) => set({ proxy: { ...proxy, mode: e.target.value as "off" | "system" | "custom" } })}
          >
            <option value="off">{t("pages.env.proxyModeOff")}</option>
            <option value="system">{t("pages.env.proxyModeSystem")}</option>
            <option value="custom">{t("pages.env.proxyModeCustom")}</option>
          </select>
        </label>
        <label className={cn("text-[0.75rem] text-[var(--t3,#8a8f98)]", (proxy.mode ?? "off") === "off" && "opacity-50")}>
          HTTP
          <input
            className={inputCls}
            value={proxy.http ?? ""}
            placeholder="http://127.0.0.1:7890"
            disabled={(proxy.mode ?? "off") === "off"}
            onChange={(e) => set({ proxy: { ...proxy, http: e.target.value || null } })}
          />
        </label>
        <label className={cn("text-[0.75rem] text-[var(--t3,#8a8f98)]", (proxy.mode ?? "off") === "off" && "opacity-50")}>
          HTTPS
          <input
            className={inputCls}
            value={proxy.https ?? ""}
            placeholder="http://127.0.0.1:7890"
            disabled={(proxy.mode ?? "off") === "off"}
            onChange={(e) => set({ proxy: { ...proxy, https: e.target.value || null } })}
          />
        </label>
        <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
          {t("pages.env.noProxy")}
          <input
            className={inputCls}
            value={noProxyText}
            placeholder="localhost, 127.0.0.1, .corp.com"
            onChange={(e) => setNoProxyText(e.target.value)}
          />
        </label>
        <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
          {t("pages.env.mavenMirror")}
          <input
            className={inputCls}
            value={draft.maven?.mirror ?? ""}
            placeholder="https://maven.aliyun.com/repository/public"
            onChange={(e) => set({ maven: { mirror: e.target.value || null } })}
          />
        </label>
        <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
          {t("pages.env.npmRegistry")}
          <input
            className={inputCls}
            value={draft.npm?.registry ?? ""}
            placeholder="https://registry.npmjs.org"
            onChange={(e) => set({ npm: { registry: e.target.value || null } })}
          />
        </label>
        <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
          PIP_INDEX_URL
          <input
            className={inputCls}
            value={draft.python?.index_url ?? ""}
            placeholder="https://pypi.tuna.tsinghua.edu.cn/simple"
            onChange={(e) => set({ python: { index_url: e.target.value || null } })}
          />
        </label>
        <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
          GOPROXY
          <input
            className={inputCls}
            value={draft.go?.goproxy ?? ""}
            placeholder="https://goproxy.cn"
            onChange={(e) => set({ go: { goproxy: e.target.value || null } })}
          />
        </label>
      </div>
      <p className="mt-2 text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.env.networkHint")}</p>
      </>
      )}
    </Card>
  );
}

type ToolCardProps = {
  meta: { key: ToolKey; label: string; rec?: string; recKey?: string };
  found: ToolProbe | null;
  installs: DiscoveredInstall[];
  required: string | null;
  defaultVersion: string;
  pending: PendingOp | null;
  opState: ReturnType<ReturnType<typeof useOperations>["get"]>;
  canPin: boolean;
  probing: boolean;
  onOpenInstall: () => void;
  onOpenUpgrade: () => void;
  onOpenInstalls: () => void;
  onPin: () => void;
  onPinDetected: () => void;
};

function ToolCard(p: ToolCardProps) {
  const { t } = useTranslation();
  const isFound = p.found?.found === true;
  const busy = p.pending != null;
  const detected = p.found?.version ?? null;
  const mismatch = Boolean(p.required && isFound && detected && !versionMatches(p.required, detected));
  const activeInstall = p.installs.find((i) => i.active) ?? p.installs[0] ?? null;
  const canDirectPin = p.meta.key === "java" || p.meta.key === "node";
  const recLabel =
    p.meta.recKey === "withNode" ? t("pages.env.recWithNode") : p.meta.recKey ?? p.meta.rec ?? p.defaultVersion;

  return (
    <Card className="flex flex-col gap-2.5 p-3 transition-colors duration-150 hover:border-[var(--line-strong,#d0d6e0)]">
      <div className="flex items-start gap-3">
        <div
          className={cn(
            "flex size-9 shrink-0 items-center justify-center rounded-[var(--r-sm,8px)]",
            p.found == null
              ? "bg-[var(--surface-2,#f3f4f5)] text-[var(--t3,#8a8f98)]"
              : isFound
                ? mismatch
                  ? "bg-[var(--st-warn-tint,#fff8e1)] text-[var(--st-warn,#9a6700)]"
                  : "bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]"
                : "bg-[var(--st-warn-tint,#fff8e1)] text-[var(--st-warn,#9a6700)]",
          )}
        >
          {p.found == null ? (
            <KeyRound className="size-5" />
          ) : isFound ? (
            mismatch ? <AlertTriangle className="size-5" /> : <CheckCircle2 className="size-5" />
          ) : (
            <XCircle className="size-5" />
          )}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="font-semibold text-[var(--t1,#222326)]">{p.meta.label}</span>
            {p.found == null ? (
              <Badge variant="outline">{t("pages.env.probing")}</Badge>
            ) : isFound ? (
              <Badge
                className={
                  mismatch
                    ? "border-[rgb(154_103_0_/_0.25)] bg-[var(--st-warn-tint,#fff8e1)] text-[var(--st-warn,#9a6700)]"
                    : "border-[rgb(39_166_68_/_0.25)] bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]"
                }
              >
                {t("pages.env.statusFound")}
              </Badge>
            ) : (
              <Badge variant="outline">{t("pages.env.statusMissing")}</Badge>
            )}
            {p.required && <Badge variant="secondary">{t("pages.env.requiredBadge", { version: p.required })}</Badge>}
            {activeInstall && <Badge variant="outline">{t(INSTALL_SOURCE_KEYS[activeInstall.source])}</Badge>}
          </div>
          <div className="mt-1 truncate font-mono text-[0.68rem] text-[var(--t3,#8a8f98)]" title={p.found?.path ?? undefined}>
            {p.found == null
              ? t("pages.env.probing")
              : isFound
                ? `${detected ?? t("pages.env.installed")}${p.found.path ? ` · ${p.found.path}` : ""}`
                : `${t("pages.env.missing")} · ${t("pages.env.recommended", { version: recLabel })}`}
          </div>
          {p.required && isFound && (
            <div className="mt-1 flex flex-wrap items-center gap-1.5 text-[0.7rem]">
              <span className="text-[var(--t3,#8a8f98)]">{t("pages.env.pinVsDetected")}</span>
              <Badge variant="secondary">{t("pages.env.pinnedLabel", { version: p.required })}</Badge>
              <span className="text-[var(--t3,#8a8f98)]">{"\u2192"}</span>
              <Badge
                className={
                  mismatch
                    ? "border-[rgb(154_103_0_/_0.25)] bg-[var(--st-warn-tint,#fff8e1)] text-[var(--st-warn,#9a6700)]"
                    : "border-[rgb(39_166_68_/_0.25)] bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]"
                }
              >
                {t("pages.env.detectedLabel", { version: detected ?? "\u2014" })}
              </Badge>
            </div>
          )}
        </div>
      </div>

      {mismatch && (
        <div className="flex flex-wrap items-center gap-2 rounded-[var(--r-sm,8px)] bg-[var(--st-warn-tint,#fff8e1)] px-2.5 py-2 text-[0.72rem] leading-relaxed text-[var(--st-warn,#9a6700)]">
          <AlertTriangle className="size-3.5 shrink-0" />
          <span className="min-w-0 flex-1">{t("pages.env.mismatchHint", { pinned: p.required, detected })}</span>
          {p.canPin && canDirectPin && detected && (
            <Button
              variant="soft"
              size="sm"
              className="h-7 shrink-0 gap-1 px-2 text-[0.7rem]"
              onClick={p.onPinDetected}
              title={t("pages.env.pinDetectedTitle", { version: detected })}
            >
              <Lock className="size-3" /> {t("pages.env.pinDetected", { version: detected })}
            </Button>
          )}
          <Button variant="outline" size="sm" className="h-7 shrink-0 px-2 text-[0.7rem]" onClick={p.onOpenInstall}>
            {t("pages.env.installRequired")}
          </Button>
        </div>
      )}

      {p.installs.length > 1 && (
        <Button
          variant="ghost"
          size="sm"
          className="h-7 justify-start gap-1 px-2 text-[0.72rem] text-[var(--t2,#62666d)]"
          onClick={p.onOpenInstalls}
        >
          <Settings2 className="size-3.5" />
          {t("pages.env.installsTitle", { count: p.installs.length })}
        </Button>
      )}

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
        <div className="mt-auto flex flex-wrap items-center gap-2">
          {isFound ? (
            <>
              <Button variant="warn" size="sm" className="gap-1" onClick={p.onOpenUpgrade}>
                <ArrowUpFromLine className="size-3.5" /> {t("pages.env.upgrade")}
              </Button>
              {p.canPin && (
                <Button variant="outline" size="sm" className="gap-1" onClick={p.onPin} title={t("pages.env.pinCurrentTitle")}>
                  <Lock className="size-3.5" /> {t("pages.env.pinCurrent")}
                </Button>
              )}
            </>
          ) : (
            <Button variant="default" size="sm" className="gap-1" onClick={p.onOpenInstall} disabled={p.probing}>
              <Download className="size-3.5" /> {t("pages.env.install")}
            </Button>
          )}
        </div>
      )}

      {p.opState?.state === "failed" && (
        <div className="rounded-[var(--r-sm,8px)] bg-[var(--st-danger-tint,#fdeeee)] px-2.5 py-2 text-[0.72rem] leading-relaxed text-[var(--st-danger,#c03535)]">
          <div className="font-semibold">{opErrorLabel(p.opState.error_code)}</div>
          {p.opState.message ? <div className="mt-0.5 text-[var(--t3,#8a8f98)]">{p.opState.message}</div> : null}
          <div className="mt-1.5 text-[var(--t2,#62666d)]">{t("pages.env.failureNextSteps")}</div>
          <div className="mt-1.5 flex flex-wrap gap-1.5">
            <Button variant="soft" size="sm" className="h-7 px-2 text-[0.7rem]" onClick={p.onOpenInstall}>
              {t("pages.env.retryAction")}
            </Button>
          </div>
        </div>
      )}
    </Card>
  );
}
