import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CheckCircle2, ChevronDown, ChevronRight, Loader2, Lock, RefreshCw, XCircle, KeyRound, ArrowUpFromLine, Download } from "lucide-react";
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
  const [managerPick, setManagerPick] = useState<"auto" | "mise" | "winget">("auto");
  const [pending, setPending] = useState<Partial<Record<ToolKey, PendingOp>>>({});
  const pendingRef = useRef(pending);
  pendingRef.current = pending;
  const handledOps = useRef(new Set<string>());

  // D1：后端会话内 TTL 缓存——进页走缓存，手动「重新探测」才强制
  const refresh = async (force = false) => {
    setProbing(true);
    try {
      setProbe(await apiToolchainProbe(force));
    } catch (e) {
      toast(e instanceof IpcFailure ? e.message : String(e), "err");
    } finally {
      setProbing(false);
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
      void refresh(); // active 标记可能随钉扎变化
      toast(t("pages.env.pinInstalled", { version }), "ok");
    } catch (e) {
      toast(e instanceof IpcFailure ? errorDisplayText(e.code, e.message) : String(e), "err");
    }
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

  const versionOptionsFor = (key: ToolKey, required: string | null): string[] => {
    const opts = [...(versions?.[key] ?? [])];
    if (opts.length === 0) opts.push(DEFAULT_VERSION[key]);
    if (required && !opts.includes(required)) opts.unshift(required);
    return opts;
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
              installs={(probe?.installs ?? []).filter((i) => i.tool === tool.key)}
              required={requiredVersion(tool.key)}
              versionOptions={versionOptionsFor(tool.key, requiredVersion(tool.key))}
              defaultVersion={DEFAULT_VERSION[tool.key]}
              versionDraft={versionDraft[tool.key] ?? ""}
              onVersionDraft={(v) => setVersionDraft((prev) => ({ ...prev, [tool.key]: v }))}
              managerPick={managerPick}
              managers={managers}
              onManagerPick={setManagerPick}
              pending={pending[tool.key] ?? null}
              opState={pending[tool.key] ? ops.get(pending[tool.key]!.opId) : null}
              canPin={ws.state.workspaceId != null && yaml.state.hash !== ""}
              onPinInstall={(v) => void pinInstall(tool.key, v)}
              onInstall={() => void startOp(tool.key, "install")}
              onUpgrade={() => void startOp(tool.key, "upgrade")}
              onPin={() => void startOp(tool.key, "pin")}
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

  useEffect(() => {
    setDraft(net ?? {});
    setNoProxyText((net?.proxy?.no_proxy ?? []).join(", "));
  }, [ws.state.workspaceId, yaml.state.hash]); // eslint-disable-line react-hooks/exhaustive-deps

  const set = (patch: NetworkSpec) => setDraft((prev) => ({ ...prev, ...patch }));

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
    <Card className="mt-3 p-4">
      <div className="mb-3 flex items-center gap-2">
        <h3 className="text-[0.875rem] font-semibold text-[var(--t1,#222326)]">{t("pages.env.networkTitle")}</h3>
        <Button variant="success" size="sm" className="ml-auto gap-1" onClick={() => void save()} disabled={busy || !hasWs}>
          {t("common.save")}
        </Button>
      </div>
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
    </Card>
  );
}

type ToolCardProps = {
  meta: { key: ToolKey; label: string; rec?: string; recKey?: string };
  found: ToolProbe | null;
  /** P1：本机已装安装枚举（java 多版本注册表/目录扫描 + nvm 目录布局），已按工具过滤。 */
  installs: DiscoveredInstall[];
  required: string | null;
  /** S1：可选版本下拉数据（钉扎版本置顶；后端白名单 ∪ mise ls-remote）。 */
  versionOptions: string[];
  defaultVersion: string;
  versionDraft: string;
  onVersionDraft: (v: string) => void;
  managerPick: "auto" | "mise" | "winget";
  managers: ManagerAvailability | null;
  onManagerPick: (m: "auto" | "mise" | "winget") => void;
  pending: PendingOp | null;
  opState: ReturnType<ReturnType<typeof useOperations>["get"]>;
  canPin: boolean;
  onPinInstall: (version: string) => void;
  onInstall: () => void;
  onUpgrade: () => void;
  onPin: () => void;
};

function ToolCard(p: ToolCardProps) {
  const { t } = useTranslation();
  const isFound = p.found?.found === true;
  const busy = p.pending != null;
  // P1：多安装枚举默认折叠；只装一个时无信息增量，不显示。
  const [installsOpen, setInstallsOpen] = useState(false);
  const showInstalls = p.installs.length > 1;
  // P2：镜像后端 launcher::version_matches——钉扎前缀匹配已装全版本（17 ↔ 17.0.7）
  const versionMatches = (want: string | null, have: string): boolean => {
    if (!want) return false;
    if (want === have) return true;
    return have.startsWith(want + ".");
  };
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
            <KeyRound className="size-5" />
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

      {/* P1：本机已装多版本枚举（只读展示；生效切换走 env_delta，随 P2 接线） */}
      {showInstalls && (
        <div className="rounded-[var(--r-sm,8px)] border border-[var(--line,#e6e6e6)]">
          <button
            type="button"
            className="flex w-full items-center gap-1.5 px-2 py-1.5 text-left text-[0.72rem] text-[var(--t2,#62666d)] hover:bg-[var(--surface-2,#f3f4f5)]"
            aria-expanded={installsOpen}
            onClick={() => setInstallsOpen((v) => !v)}
          >
            {installsOpen ? (
              <ChevronDown className="size-3.5 shrink-0" />
            ) : (
              <ChevronRight className="size-3.5 shrink-0" />
            )}
            <span>{t("pages.env.installsTitle", { count: p.installs.length })}</span>
          </button>
          {installsOpen && (
            <ul className="border-t border-[var(--line,#e6e6e6)]">
              {p.installs.map((i) => {
                const selected = versionMatches(p.required, i.version);
                return (
                  <li
                    key={`${i.version}-${i.home}`}
                    className={cn(
                      "flex items-center gap-1.5 px-2 py-1",
                      selected && "bg-[var(--st-ok-tint,#e9f7ed)]",
                    )}
                  >
                    <span className="shrink-0 font-mono text-[0.72rem] font-medium text-[var(--t1,#222326)]">
                      {i.version}
                    </span>
                    {i.active && <Badge variant="default">{t("pages.env.installActive")}</Badge>}
                    {selected && (
                      <Badge className="border-[rgb(39_166_68_/_0.3)] bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]">
                        {t("pages.env.selectedVersion")}
                      </Badge>
                    )}
                    <Badge variant="outline">{t(INSTALL_SOURCE_KEYS[i.source])}</Badge>
                    <span
                      className="min-w-0 flex-1 truncate text-right font-mono text-[0.64rem] text-[var(--t3,#8a8f98)]"
                      title={i.home}
                    >
                      {i.home}
                    </span>
                    {p.canPin && (
                      <Button
                        variant={selected ? "ghost" : "soft"}
                        size="sm"
                        className="h-6 shrink-0 px-2 text-[0.68rem]"
                        onClick={() => p.onPinInstall(i.version)}
                        title={t("pages.env.installSelectTitle")}
                      >
                        {t("pages.env.installSelect")}
                      </Button>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      )}

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
                <ArrowUpFromLine className="size-3.5" /> {t("pages.env.upgrade")}
              </Button>
              {p.canPin && (
                <Button variant="outline" size="sm" onClick={p.onPin} title={t("pages.env.pinCurrentTitle")}>
                  <Lock className="size-3.5" /> {t("pages.env.pinCurrent")}
                </Button>
              )}
            </>
          ) : (
            <>
              <select
                className="h-8 cursor-pointer rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] px-1.5 font-mono text-[0.75rem] text-[var(--t1,#222326)]"
                value={p.versionDraft || p.required || p.defaultVersion}
                onChange={(e) => p.onVersionDraft(e.target.value)}
                aria-label={t("pages.env.versionAria", { tool: p.meta.label })}
              >
                {p.versionOptions.map((v) => (
                  <option key={v} value={v}>
                    {v}
                  </option>
                ))}
              </select>
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
                  <Lock className="size-3.5" /> {t("pages.env.installPin")}
                </Button>
              )}
            </>
          )}
        </div>
      )}

      {/* F9：两个管理器都缺 → 安装按钮禁用不再是终点，给出下一步指引 */}
      {!busy && p.managers && !p.managers.mise && !p.managers.winget && (
        <div className="rounded-[var(--r-sm,8px)] bg-[var(--st-warn-tint,#fff8e1)] px-2 py-1.5 text-[0.72rem] leading-relaxed text-[var(--st-warn,#9a6700)]">
          {t("pages.env.noManagerHint")}
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
