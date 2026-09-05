import { useEffect, useRef, useState, useCallback, useMemo, type ReactNode } from "react";
import { useOutletContext, NavLink, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button, buttonVariants } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { cn } from "@/lib/utils";
import { copyText } from "@/lib/copy-text";
import { openUrlInBrowser, localPortUrl } from "@/lib/open-url";
import {
  RUN_CARD_MAX_WIDTH,
  RUN_CARD_MIN_WIDTH,
  readRunCardWidthPref,
  writeRunCardWidthPref,
} from "@/lib/workspace-storage";
import { EnvVariablesEditor } from "@/components/env-variables-editor";
import { useRuntime } from "@/providers/runtime-provider";
import { useWorkspace } from "@/providers/workspace-provider";
import { useYaml } from "@/providers/yaml-provider";
import { useToast } from "@/components/ui/toast";
import { LogView } from "@/components/log-view";
import { SpringConfigPanel } from "@/components/spring-config-panel";
import { AiExplainButton } from "@/components/ai-explain";
import { TerminalView } from "@/components/terminal-view";
import { SectionTitle, SectionMeta } from "@/components/section-title";
import {
  STATE_META,
  StatusDot,
  StatusChip,
  GATEWAY_STATE_TINT,
  GATEWAY_STATE_DOT,
  fmtDuration,
  fmtTime,
  healthClass,
  opErrorLabel,
  scriptDotState,
  scriptStateLabel,
  stateLabel,
} from "@/lib/status";
import {
  apiOpenIde,
  apiRuntimeBuild,
  apiPortsInspect,
  apiPortsSuggest,
  apiPortsAssign,
  apiEnvEffective,
  apiScriptCancel,
  apiScriptRun,
  apiDockerProbe,
  apiDockerPs,
  apiGatewayStart,
  apiGatewayStop,
  apiGatewayRestart,
  apiGatewayStatus,
  apiToolchainProbe,
} from "../ipc/api";
import { IpcFailure } from "../ipc/protocol";
import type {
  ContainerSummary,
  DockerProbe,
  EnvEffectiveOut,
  GatewayStatusOut,
  IdeTarget,
  LogSource,
  ScriptRuntimeView,
  ScriptSpec,
  ServiceMetrics,
  ServiceRuntimeView,
  ServiceSpec,
  SuperTaskFile,
  ToolchainProbeOut,
} from "@/ipc/protocol";
import {
  Play,
  Square,
  RotateCw,
  FileText,
  ScrollText,
  SlidersHorizontal,
  HeartPulse,
  BarChart3,
  Globe,
  KeyRound,
  ExternalLink,
  Copy,
  Check,
  Cpu,
  HardDrive,
  Boxes,
  Loader2,
  Container,
  MemoryStick,
  PackagePlus,
  SquareTerminal,
  ChevronDown,
  ChevronRight,
  ArrowLeftRight,
  Layers,
} from "lucide-react";
import type { ShellCtx } from "../app/AppShell";

/* ---------------- helpers ---------------- */

/**
 * 端口冲突态：后端三维归属判定（端口 + 工作目录 + 程序类型）认定占位进程归属外部时，
 * 服务置 Stopped/Exited + last_error 带统一前缀（见 engine::conflict_message）。
 * 前端靠此前缀识别：禁用启动 + 指引去「端口」Tab 更换端口。
 */
function isPortConflict(svc: Pick<ServiceRuntimeView, "state" | "last_error">): boolean {
  if (svc.state !== "stopped" && svc.state !== "exited") return false;
  const msg = svc.last_error ?? "";
  return /端口\s*\S*\s*(被.*占用|已被占用)/.test(msg);
}

function serviceCmd(id: string, s: ServiceSpec): string {
  // Empty arrays are omitted by the Rust IPC serializer; keep command previews
  // usable for older workspaces and for services without extra arguments.
  const extraArgs = s.extra_args ?? [];
  if (s.kind === "compose") {
    // 1.3：compose 服务由引擎执行 `docker compose -f <file> up -d --no-deps <service>`
    return `docker compose up -d --no-deps ${s.service ?? id}`;
  }
  if (s.kind === "node") {
    const dir = s.dir ?? id;
    const script = s.script ?? "dev";
    return `npm --prefix ${dir} run ${script}`;
  }
  // 1.7：python / go / generic（仅展示用途）
  if (s.kind === "python") {
    const mod = s.module ?? "";
    const base = mod ? `python -m ${mod}` : `python ${s.entry ?? ""}`.trimEnd();
    return [base, ...extraArgs].join(" ");
  }
  if (s.kind === "go") {
    return `go run ${s.package ?? "."}${extraArgs.length ? ` ${extraArgs.join(" ")}` : ""}`;
  }
  if (s.kind === "generic") {
    return [s.program ?? "?", ...(s.args ?? []), ...extraArgs].join(" ");
  }
  // 单模块（module "." 或缺省）省略 -pl，与引擎 plan_spring 的行为一致
  const module = s.module ?? "";
  return module === "." || module === "" ? "mvn spring-boot:run" : `mvn -pl ${module} spring-boot:run`;
}

/** 1.7：kind → 徽标文案/颜色映射表（禁止按 kind 写 if 长链的呈现逻辑）。 */
const KIND_BADGE: Record<string, { label: string; color: string }> = {
  "spring-boot": { label: "SPRING", color: "var(--st-accent,#5e6ad2)" },
  node: { label: "NODE", color: "#2E90FA" },
  compose: { label: "COMPOSE", color: "#12B76A" },
  task: { label: "TASK", color: "var(--t3,#8a8f98)" },
  python: { label: "PYTHON", color: "#F79009" },
  go: { label: "GO", color: "#00B8D9" },
  generic: { label: "PROC", color: "#8a8f98" },
};

function KindBadge({ kind, buildTool }: { kind: string; buildTool?: string | null }) {
  const fallback = { label: kind.toUpperCase(), color: "var(--t3,#8a8f98)" };
  const { label, color } = KIND_BADGE[kind] ?? fallback;
  return (
    <>
      <span className="inline-flex h-5 items-center gap-1 rounded-full bg-[var(--surface-2,#f3f4f5)] px-1.5 font-mono text-[10px] font-semibold uppercase leading-none text-[var(--t2,#62666d)]">
        <span className="size-1.5 rounded-full" style={{ background: color }} />
        {label}
      </span>
      {buildTool === "gradle" ? (
        <span className="inline-flex h-5 items-center rounded-full bg-[var(--surface-2,#f3f4f5)] px-1.5 font-mono text-[10px] font-semibold uppercase leading-none text-[var(--t2,#62666d)]">
          gradle
        </span>
      ) : null}
    </>
  );
}

function serviceYamlFragment(id: string, s: ServiceSpec): string {
  const lines: string[] = [`${id}:`, `  kind: ${s.kind}`, `  enabled: ${s.enabled}`];
  if (s.module) lines.push(`  module: ${s.module}`);
  if (s.dir) lines.push(`  dir: ${s.dir}`);
  if (s.port != null) lines.push(`  port: ${s.port}`);
  const envKeys = Object.keys(s.env ?? {});
  if (envKeys.length) {
    lines.push("  env:");
    for (const k of envKeys) lines.push(`    ${k}: ${s.env[k]}`);
  }
  if (s.depends_on?.length) lines.push(`  depends_on: [${s.depends_on.join(", ")}]`);
  if (s.health) lines.push(`  health:\n    type: ${s.health.type}`);
  return lines.join("\n");
}

/* ---------------- IDE 打开菜单（spec §13.3） ---------------- */

const IDE_TARGETS: { id: IdeTarget; labelKey: string }[] = [
  { id: "explorer", labelKey: "pages.run.ideExplorer" },
  { id: "cursor", labelKey: "pages.run.ideCursor" },
  { id: "idea", labelKey: "pages.run.ideIdea" },
  { id: "code", labelKey: "pages.run.ideCode" },
];

/**
 * 「打开」入口：小按钮 + 下拉菜单（固定四目标枚举）。
 * 不依赖服务运行状态，随时可打开；fixed 定位避免被卡片列滚动容器裁剪。
 */
function IdeOpenMenu({ variant }: { variant: "icon" | "button" }) {
  const ws = useWorkspace();
  const { toast } = useToast();
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<IdeTarget | null>(null);
  const [pos, setPos] = useState<{ top: number; right: number } | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const workspaceId = ws.state.workspaceId;

  const toggle = () => {
    if (open) {
      setOpen(false);
      return;
    }
    const r = triggerRef.current?.getBoundingClientRect();
    if (r) setPos({ top: r.bottom + 4, right: window.innerWidth - r.right });
    setOpen(true);
  };

  // Esc 关闭并归还焦点到触发按钮（键盘可达）
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
        triggerRef.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open]);

  const openWith = async (ide: IdeTarget, label: string) => {
    if (!workspaceId || busy) return;
    setBusy(ide);
    try {
      const out = await apiOpenIde(workspaceId, ide);
      toast(t("pages.run.openedWith", { label, path: out.path }), "ok");
      setOpen(false);
      triggerRef.current?.focus();
    } catch (e) {
      if (e instanceof IpcFailure && e.code === "IDE_NOT_FOUND") {
        toast(t("pages.run.ideNotFound", { label }), "warn");
      } else {
        toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
      }
    } finally {
      setBusy(null);
    }
  };

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        onClick={toggle}
        disabled={!workspaceId}
        aria-label={t("pages.run.openWsAria")}
        aria-haspopup="menu"
        aria-expanded={open}
        title={t("pages.run.openWsTitle")}
        className={
          variant === "icon"
            ? buttonVariants({ variant: "outline", size: "icon-xs" })
            : buttonVariants({ variant: "outline", size: "sm" })
        }
      >
        <ExternalLink className="size-3.5" />
        {variant === "button" ? t("common.open") : null}
      </button>

      {open ? (
        <>
          <div className="fixed inset-0 z-[205]" onClick={() => setOpen(false)} aria-hidden />
          <div
            role="menu"
            aria-label={t("pages.run.pickIdeAria")}
            className="fixed z-[210] min-w-[10.5rem] overflow-hidden rounded-[var(--r-md,12px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] p-1 shadow-[var(--shadow-2,0_6px_20px_rgb(16_24_40_/_0.09))]"
            style={{ top: pos?.top ?? 0, right: pos?.right ?? 0 }}
          >
            {IDE_TARGETS.map((target, i) => (
              <button
                key={target.id}
                role="menuitem"
                // eslint-disable-next-line jsx-a11y/no-autofocus -- 菜单打开即聚焦首项，键盘可达
                autoFocus={i === 0}
                disabled={busy !== null}
                onClick={() => void openWith(target.id, t(target.labelKey))}
                className="flex w-full items-center rounded-[var(--r-sm,8px)] px-2.5 py-1.5 text-left text-[0.8rem] text-[var(--t1,#222326)] transition-colors duration-150 hover:bg-[var(--st-accent-tint,#eef0fb)] focus-visible:bg-[var(--st-accent-tint,#eef0fb)] focus-visible:outline-none disabled:opacity-50"
              >
                {t(target.labelKey)}
                {busy === target.id ? <span className="ml-auto text-[0.68rem] text-[var(--t3,#8a8f98)]">{t("pages.run.opening")}</span> : null}
              </button>
            ))}
          </div>
        </>
      ) : null}
    </>
  );
}

/* ---------------- health sparkline ---------------- */

function HealthSparkline({ ok }: { ok: boolean | null | undefined }) {
  const [hist, setHist] = useState<number[]>([]);
  useEffect(() => {
    const id = setInterval(() => {
      setHist((h) => [...h.slice(-23), ok === true ? 1 : ok === false ? 0 : -1]);
    }, 1500);
    return () => clearInterval(id);
  }, [ok]);
  const w = 560;
  const h = 88;
  const pts = hist
    .map((v, i) => {
      const x = hist.length <= 1 ? w : (i / (hist.length - 1)) * w;
      const y = v < 0 ? h - 6 : v === 1 ? 8 : h - 8;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  return (
    <svg viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" className="h-[88px] w-full">
      <path d={hist.length > 1 ? `M${pts}` : ""} fill="none" stroke="var(--st-accent,#5e6ad2)" strokeWidth={1.5} />
      {hist.map((v, i) => {
        const x = hist.length <= 1 ? w : (i / (hist.length - 1)) * w;
        const y = v < 0 ? h - 6 : v === 1 ? 8 : h - 8;
        const cls = v >= 0 ? (v === 1 ? "var(--st-ok,#27a644)" : "var(--st-warn,#9a6700)") : "var(--t3,#8a8f98)";
        return <circle key={i} cx={x} cy={y} r={i === hist.length - 1 ? 3 : 2} fill={cls} />;
      })}
    </svg>
  );
}

/* ---------------- port link ---------------- */

/** 可点击端口：拉起系统默认浏览器访问 http://localhost:<port>/；服务未运行时为纯文本不可点击 */
function PortLink({ port, className, disabled }: { port: number; className?: string; disabled?: boolean }) {
  const { t } = useTranslation();
  const { toast } = useToast();
  if (disabled) {
    return <span className={cn("inline-flex items-center gap-1 font-mono", className)}>{port}</span>;
  }
  return (
    <button
      type="button"
      className={cn(
        "group/port inline-flex cursor-pointer items-center gap-1 font-mono transition-colors duration-150 hover:underline",
        className,
      )}
      title={t("pages.run.openPortTitle", { port })}
      onClick={(e) => {
        e.stopPropagation();
        void openUrlInBrowser(localPortUrl(port)).then((ok) => {
          if (!ok) toast(t("pages.run.openPortFailed", { port }), "err");
        });
      }}
    >
      {port}
      <ExternalLink className="size-3 opacity-40 transition-opacity duration-150 group-hover/port:opacity-100" />
    </button>
  );
}

/* ---------------- service card ---------------- */

function ServiceCard({
  id,
  svc,
  spec,
  selected,
  onOpen,
}: {
  id: string;
  svc: ServiceRuntimeView;
  spec: ServiceSpec | undefined;
  selected: boolean;
  onOpen: () => void;
}) {
  const runtime = useRuntime();
  const { t } = useTranslation();
  const isRunning = svc.state === "running";
  const isBusy = svc.state === "starting" || svc.state === "stopping" || svc.state === "building";
  const external = isRunning && svc.managed === false;
  const portConflict = isPortConflict(svc);
  // 停止是杀整棵进程树的破坏性操作，二次确认；中断 starting 不弹
  const [confirmStop, setConfirmStop] = useState(false);

  const depsText = spec?.depends_on?.length
    ? t("pages.run.dependsOn", { deps: spec.depends_on.join(", ") })
    : t("pages.run.noDeps");
  // 2.2 restart：退避等待/自动重启进行中，或当前实例由第 n 次自动重启拉起
  const restartBadge = svc.restart_attempt != null && (
    <span
      className="block h-5 shrink-0 rounded-full bg-[var(--st-warn-tint,#fff8e1)] px-1.5 font-mono text-[10px] leading-5 text-[var(--st-warn,#9a6700)]"
      title={t("pages.run.restartAttempt", { n: svc.restart_attempt })}
    >
      ⟳ {t("pages.run.restartAttempt", { n: svc.restart_attempt })}
    </span>
  );
  const foot = (
    <span className="flex min-w-0 items-center gap-1 text-[11px] text-[var(--t3)]">
      {restartBadge}
      {svc.last_error ? (
        <span className="block truncate text-[11px] text-[var(--st-danger)]" title={svc.last_error}>⚠ {svc.last_error}</span>
      ) : (
        <span
          className="block h-5 max-w-full truncate rounded-full bg-[var(--surface-2)] px-1.5 font-mono text-[10px] leading-5 text-[var(--t3)]"
          title={depsText}
        >
          {depsText}
        </span>
      )}
    </span>
  );

  // Rail color follows runtime status (not selection). Selection is elevation/border only.
  const railColor =
    svc.state === "running" || svc.state === "unhealthy"
      ? STATE_META[svc.state].color
      : svc.state === "exited"
        ? "var(--st-danger)"
        : isBusy
          ? "var(--st-warn-dot)"
          : "var(--line-strong)";

  return (
    <div
      onClick={onOpen}
      className={cn(
        "group/svc relative flex min-h-[6.1rem] shrink-0 @container cursor-pointer flex-col overflow-hidden rounded-[var(--r-md)] border bg-[var(--surface)] px-3 py-2.5 transition-all duration-150 ease-[var(--st-ease)]",
        selected
          ? "border-[var(--line-strong)] shadow-[var(--shadow-1),var(--st-select-ring)]"
          : "border-[var(--line)] hover:border-[var(--line-strong)] hover:bg-[var(--surface-2)]",
      )}
    >
      <span
        aria-hidden
        className={cn(
          "absolute left-0 top-0 h-full w-[3px] origin-left transition-transform duration-200 ease-[var(--st-ease)]",
          selected || isRunning || isBusy || svc.state === "exited" || svc.state === "unhealthy"
            ? "scale-x-100"
            : "scale-x-0 group-hover/svc:scale-x-100",
        )}
        style={{ background: railColor }}
      />
      <div className="flex items-center gap-2">
        <StatusDot state={svc.state} size={8} />
        <span className="min-w-0 truncate text-[0.92rem] font-semibold tracking-tight text-[var(--t1)]" title={id}>
          {id}
        </span>
        <span className="inline-flex shrink-0 items-center gap-2 @max-[300px]:hidden">
          <KindBadge kind={svc.kind} buildTool={spec?.build_tool} />
        </span>
        {external ? (
          <span className="inline-flex h-5 items-center shrink-0 rounded-full bg-[var(--surface-2)] px-1.5 font-mono text-[10px] font-semibold uppercase leading-none text-[var(--t2)] @max-[300px]:hidden" title={t("pages.run.externalTitle")}>
            {t("pages.run.externalShort")}
          </span>
        ) : null}
        {portConflict ? (
          <span
            className="inline-flex h-5 items-center shrink-0 rounded-full border border-[var(--st-danger-ring)] bg-[var(--st-danger-tint)] px-1.5 text-[10px] font-semibold leading-none text-[var(--st-danger)] @max-[300px]:hidden"
            title={svc.last_error ?? t("pages.run.portConflictHint")}
          >
            {t("pages.run.portConflict")}
          </span>
        ) : null}
        <div className="ml-auto flex gap-1 opacity-60 transition-opacity group-hover/svc:opacity-100" onClick={(e) => e.stopPropagation()}>
          <span className="inline-flex @max-[300px]:hidden">
            <IdeOpenMenu variant="icon" />
          </span>
          {isRunning ? (
            <button
              type="button"
              className="grid size-7 cursor-pointer place-items-center rounded-[var(--r-sm)] border border-[var(--st-warn-line)] bg-[var(--st-warn-tint)] text-[var(--st-warn)] transition-colors duration-150 hover:border-[var(--st-warn)]/50 disabled:cursor-not-allowed disabled:opacity-50 @max-[250px]:hidden"
              title={external ? t("pages.run.restartExternalTitle") : t("common.restart")}
              disabled={isBusy}
              onClick={() => runtime.actions.restartOne(id)}
            >
              {isBusy ? <Loader2 className="size-3.5 animate-spin" /> : <RotateCw className="size-3.5" />}
            </button>
          ) : null}
          <button
            type="button"
            className={cn(
              "grid size-7 cursor-pointer place-items-center rounded-[var(--r-sm)] border transition-colors duration-150 disabled:cursor-not-allowed disabled:opacity-50",
              isRunning || svc.state === "starting"
                ? "border-transparent text-[var(--st-danger)] hover:border-[var(--st-danger-ring)] hover:bg-[var(--st-danger-tint)]"
                : "border-transparent text-[var(--t3)] hover:border-[var(--line-strong)] hover:bg-[var(--surface-2)] hover:text-[var(--st-ok-deep)]",
            )}
            title={isRunning ? t("common.stop") : portConflict ? (svc.last_error ?? t("pages.run.portConflictHint")) : t("common.start")}
            disabled={isBusy || portConflict}
            onClick={() =>
              isRunning || svc.state === "starting"
                ? isRunning
                  ? setConfirmStop(true)
                  : runtime.actions.stopOne(id)
                : runtime.actions.startOne(id)
            }
          >
            {isBusy ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : isRunning || svc.state === "starting" ? (
              <Square className="size-3.5" />
            ) : (
              <Play className="size-3.5" />
            )}
          </button>
        </div>
      </div>

      <ConfirmDialog
        open={confirmStop}
        title={t("pages.run.stopConfirmTitle", { id })}
        description={
          external
            ? t("pages.run.stopConfirmExternal")
            : t("pages.run.stopConfirmDesc")
        }
        confirmText={t("pages.run.stopService")}
        cancelText={t("common.cancel")}
        destructive
        onConfirm={() => {
          setConfirmStop(false);
          runtime.actions.stopOne(id);
        }}
        onCancel={() => setConfirmStop(false)}
      />

      <div className="mt-2 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-[var(--t2)]">
        <StatusChip
          state={svc.state}
          extra={isRunning && svc.started_at_ms ? fmtDuration(svc.started_at_ms) : undefined}
        />
        {svc.port ? <PortLink port={svc.port} disabled={!isRunning} className="font-mono text-[var(--t1)]" /> : null}
        {svc.pid ? (
          <span className="font-mono text-[var(--t3)] @max-[250px]:hidden">pid {svc.pid}</span>
        ) : svc.kind === "compose" ? (
          <span
            className="rounded-full bg-[var(--surface-2)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--t3)]"
            title={t("pages.run.containerManagedTitle")}
          >
            {t("pages.run.containerManaged")}
          </span>
        ) : null}
      </div>

      <div className="mt-2 min-h-[1.05rem] overflow-hidden">{foot}</div>
    </div>
  );
}

/* ---------------- ports panel（自「环境」拆出：端口检查 / 建议 / 改端口） ---------------- */

type PortCheck = {
  port: number;
  inUse: boolean;
  message: string;
};

function PortsPanel({ id, compact }: { id: string; compact: boolean }) {
  const ws = useWorkspace();
  const yaml = useYaml();
  const runtime = useRuntime();
  const { toast } = useToast();
  const { t } = useTranslation();
  const spec = ws.state.spec;
  const svc = spec?.services[id];
  const rtSvc = runtime.state.services[id];
  const portConflict = rtSvc ? isPortConflict(rtSvc) : false;
  const isRunning = rtSvc?.state === "running";
  const [portDraft, setPortDraft] = useState<string>(String(svc?.port ?? ""));
  const [portBusy, setPortBusy] = useState(false);
  const [portCheck, setPortCheck] = useState<PortCheck | null>(null);
  const [portCandidates, setPortCandidates] = useState<number[]>([]);

  useEffect(() => {
    setPortDraft(String(svc?.port ?? ""));
    setPortCheck(null);
    setPortCandidates([]);
  }, [id, svc?.port]);

  if (!svc) return null;

  const portNumber = () => {
    const n = Number(portDraft);
    return { n, valid: Number.isInteger(n) && n >= 1024 && n <= 65535 };
  };
  const { valid: portValid } = portNumber();

  const inspectPorts = async () => {
    if (!ws.state.workspaceId || portBusy) return;
    const { n, valid } = portNumber();
    if (!valid) {
      setPortCheck({ port: n, inUse: false, message: t("pages.run.portRangeFirst") });
      return;
    }
    setPortBusy(true);
    try {
      const out = await apiPortsInspect(ws.state.workspaceId, id, n);
      const item = out.items[0];
      if (!item) {
        setPortCheck({ port: n, inUse: false, message: t("pages.run.portUnknown") });
      } else if (item.in_use) {
        setPortCheck({
          port: item.port,
          inUse: true,
          message: t("pages.run.portInUseBy", {
            port: item.port,
            name: item.process_name ?? (item.pid != null ? `PID ${item.pid}` : t("pages.run.pidUnknown")),
            managed: item.managed ? t("pages.run.managedBySt") : t("pages.run.externalProcess"),
          }),
        });
      } else {
        setPortCheck({
          port: item.port,
          inUse: false,
          message: t("pages.run.portAvailable", { port: item.port, current: isRunning && svc.port === item.port ? t("pages.run.currentService") : "" }),
        });
      }
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setPortBusy(false);
    }
  };

  const suggestPorts = async () => {
    if (!ws.state.workspaceId || portBusy) return;
    setPortBusy(true);
    try {
      const out = await apiPortsSuggest(ws.state.workspaceId, id);
      setPortCandidates(out.candidates);
      // 有候选只展示可点击 chip，不再用文字再列一遍端口
      setPortCheck(
        out.candidates.length
          ? null
          : { port: portNumber().n, inUse: false, message: t("pages.run.noCandidates") },
      );
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setPortBusy(false);
    }
  };

  const editPortDraft = (v: string) => {
    setPortDraft(v);
    setPortCheck(null);
    setPortCandidates([]);
  };

  const assignPort = async (restart: boolean) => {
    if (!spec || !ws.state.workspaceId || !yaml.state.hash || portBusy) return;
    const { n, valid } = portNumber();
    if (!valid) {
      toast(t("pages.templates.portInvalid"), "warn");
      return;
    }
    setPortBusy(true);
    try {
      const out = await apiPortsAssign(ws.state.workspaceId, id, n, yaml.state.hash, restart);
      if (out.restart_required) {
        setPortCheck({ port: n, inUse: false, message: t("pages.run.restartRequired") });
      } else {
        await Promise.all([yaml.actions.reload(), ws.actions.refreshSpec()]);
        setPortCheck({ port: n, inUse: false, message: out.notes.length ? out.notes.join("；") : t("pages.run.portSaved") });
        toast(t("pages.run.portSaved"), "ok");
      }
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setPortBusy(false);
    }
  };

  return (
    <div className={cn("flex flex-col gap-5 p-4", compact && "gap-4 p-3")}>
      <section className="flex flex-col gap-2">
        <SectionTitle
          icon={<ArrowLeftRight />}
          title={t("pages.run.portSection")}
          meta={
            svc.port != null ? (
              <SectionMeta>{t("pages.run.currentPort", { port: svc.port })}</SectionMeta>
            ) : undefined
          }
        />
        {portConflict && rtSvc?.last_error ? (
          <p className="rounded-[var(--r-sm,8px)] border border-red-200 bg-[var(--st-danger-tint,#fdecec)] px-2 py-1.5 text-[0.72rem] font-medium text-[#DC2626]">
            ⚠ {rtSvc.last_error}：{t("pages.run.portConflictHint")}
          </p>
        ) : null}
        <div className="flex flex-wrap items-center gap-2">
          <Input
            type="number"
            value={portDraft}
            onChange={(e) => editPortDraft(e.target.value)}
            className="h-8 max-w-[7.5rem] font-mono text-sm"
            placeholder={t("pages.run.portSection")}
            aria-label={t("pages.run.servicePortAria")}
          />
          <Button variant="soft" size="sm" onClick={() => void inspectPorts()} disabled={!ws.state.workspaceId || portBusy || !portValid}>
            {t("pages.run.check")}
          </Button>
          <Button variant="outline" size="sm" onClick={() => void suggestPorts()} disabled={!ws.state.workspaceId || portBusy}>
            {t("pages.run.suggest")}
          </Button>
          <Button size="sm" variant="success" onClick={() => void assignPort(false)} disabled={!ws.state.workspaceId || portBusy || !portValid}>
            {t("common.save")}
          </Button>
          {isRunning ? (
            <Button size="sm" variant="warn" onClick={() => void assignPort(true)} disabled={portBusy || !portValid}>
              {t("pages.run.changePortRestart")}
            </Button>
          ) : null}
        </div>
        {portCandidates.length > 0 ? (
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.run.suggestedPorts")}</span>
            {portCandidates.map((p) => (
              <button
                key={p}
                type="button"
                title={t("pages.run.fillPort", { port: p })}
                className="cursor-pointer rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] px-2 py-0.5 font-mono text-[0.72rem] font-medium text-[var(--t1,#222326)] transition-colors hover:border-[var(--st-accent,#5e6ad2)] hover:bg-[var(--st-accent-tint,#eef0fb)] hover:text-[var(--st-accent,#5e6ad2)]"
                onClick={() => {
                  setPortDraft(String(p));
                  setPortCheck(null);
                  setPortCandidates([]);
                }}
              >
                :{p}
              </button>
            ))}
          </div>
        ) : portCheck ? (
          <p
            className={cn(
              "text-[0.72rem]",
              portCheck.inUse ? "text-[var(--st-danger,#c03535)]" : "text-[var(--t2,#62666d)]",
            )}
          >
            {portCheck.message}
          </p>
        ) : null}
      </section>
    </div>
  );
}

/* ---------------- env panel（「变量」Tab：环境变量编辑 + 生效快照 + Spring 配置） ---------------- */

function EnvPanel({ id, compact }: { id: string; compact: boolean }) {
  const ws = useWorkspace();
  const yaml = useYaml();
  const runtime = useRuntime();
  const { toast } = useToast();
  const { t } = useTranslation();
  const spec = ws.state.spec;
  const svc = spec?.services[id];
  const [envDraft, setEnvDraft] = useState<Record<string, string>>(() => svc?.env ?? {});
  const [effEnv, setEffEnv] = useState<EnvEffectiveOut | null>(null);
  const svcRt = runtime.state.services[id];
  const effEnvState = svcRt?.state;
  const effEnvStartedAt = svcRt?.started_at_ms ?? null;
  const isSpring = svc?.kind === "spring-boot";

  useEffect(() => {
    setEnvDraft(svc?.env ?? {});
    // yaml 重载会替换 spec 对象，svc?.env 引用变化即重置草稿
  }, [id, svc?.env]);

  // 生效环境快照：启动/重启后自动刷新（引擎自报，未启动过为空快照）
  useEffect(() => {
    const wid = ws.state.workspaceId;
    if (!wid) return;
    let alive = true;
    apiEnvEffective(wid, id)
      .then((out) => {
        if (alive) setEffEnv(out);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [ws.state.workspaceId, id, effEnvState, effEnvStartedAt]);

  if (!svc) return null;

  const saveEnv = async () => {
    if (!spec) return;
    const next: SuperTaskFile = {
      ...spec,
      services: {
        ...spec.services,
        [id]: { ...svc, env: envDraft },
      },
    };
    const ok = await yaml.actions.saveForm(next);
    if (ok) {
      await ws.actions.refreshSpec();
      toast(t("pages.run.envSaved"), "ok");
    } else {
      toast(yaml.state.error ?? t("operations.savedFailed"), "err");
    }
  };

  return (
    <div className={cn("flex flex-col gap-5 p-4", compact && "gap-4 p-3")}>
      {/* 可编辑项前置：环境变量是本 Tab 的高频操作，只读的项目配置沉底 */}
      <EnvVariablesEditor
        value={envDraft}
        onChange={setEnvDraft}
        onSave={saveEnv}
        saveDisabled={!ws.state.workspaceId}
      />

      <Separator />

      <section className="flex flex-col gap-2">
        <SectionTitle
          icon={<KeyRound />}
          title={t("pages.run.effectiveEnv")}
          meta={
            effEnv?.captured_at_ms ? (
              <SectionMeta>{t("pages.run.effectiveEnvAt", { time: fmtTime(effEnv.captured_at_ms) })}</SectionMeta>
            ) : undefined
          }
        />
        {effEnv && effEnv.entries.length > 0 ? (
          <div className="flex flex-col gap-1">
            {effEnv.entries.map((e) => (
              <div
                key={e.key}
                className="flex items-center gap-2 rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f7f8fa)] px-2 py-1"
              >
                <span className="w-[38%] shrink-0 truncate font-mono text-[0.78rem] font-medium text-[var(--t1,#222326)]" title={e.key}>{e.key}</span>
                <span className="min-w-0 flex-1 truncate font-mono text-[0.78rem] text-[var(--t2,#62666d)]" title={e.value}>{e.value}</span>
                <span
                  className="ml-auto shrink-0 rounded-full bg-[var(--surface,#fff)] px-1.5 py-0.5 text-[10px] leading-none text-[var(--t3,#8a8f98)] ring-1 ring-[var(--line,#e2e5eb)]"
                  title={t("pages.run.envSourceTitle")}
                >
                  {t(`pages.run.envSource.${e.source}`, { defaultValue: e.source })}
                </span>
              </div>
            ))}
          </div>
        ) : (
          <p className="text-[0.72rem] text-[var(--t2,#62666d)]">
            {svc.kind === "compose"
              ? t("pages.run.effectiveEnvCompose")
              : svcRt?.managed === false
                ? t("pages.run.effectiveEnvExternal")
                : t("pages.run.effectiveEnvEmpty")}
          </p>
        )}
      </section>

      {isSpring ? (
        <>
          <Separator />
          <SpringConfigPanel
            key={id}
            workspaceId={ws.state.workspaceId ?? null}
            serviceId={id}
            specPort={svc.port ?? null}
            svcEnv={svc?.env}
            effEnvEntries={effEnv?.entries}
            compact={compact}
          />
        </>
      ) : null}
    </div>
  );
}

/* ---------------- runtime panel（「环境」Tab：按 kind 展示运行时工具并切换版本） ---------------- */

type RuntimeToolKey = "java" | "maven" | "node" | "python" | "go";

/** kind → 运行时工具（顺序即展示序）；不在表内的 kind 走提示文案。 */
const RUNTIME_TOOLS: Record<string, { key: RuntimeToolKey; label: string }[]> = {
  "spring-boot": [
    { key: "java", label: "JDK" },
    { key: "maven", label: "Maven" },
  ],
  node: [{ key: "node", label: "Node.js" }],
  python: [{ key: "python", label: "Python" }],
  go: [{ key: "go", label: "Go" }],
};

/** 运行详情的版本选择只保存在服务 env，避免影响同一工作区的其他服务。 */
const SERVICE_RUNTIME_ENV_KEY: Record<RuntimeToolKey, string> = {
  java: "SUPERTASK_JAVA_VERSION",
  maven: "SUPERTASK_MAVEN_VERSION",
  node: "SUPERTASK_NODE_VERSION",
  python: "SUPERTASK_PYTHON_VERSION",
  go: "SUPERTASK_GO_VERSION",
};

function versionMajor(version: string): string {
  return version.trim().replace(/^v/i, "").split(/[.\-_+]/, 1)[0]?.toLowerCase() ?? "";
}

function versionMatches(want: string | null | undefined, have: string): boolean {
  if (!want) return false;
  const normalizedWant = want.trim();
  const normalizedHave = have.trim();
  return normalizedWant.toLowerCase() === normalizedHave.toLowerCase() || normalizedHave.startsWith(`${normalizedWant}.`);
}

/**
 * 运行环境：探测本机工具版本 + 当前服务的版本选择，可切换。
 * 切换只写当前 service.env；缺失版本不进入此处的下拉，也不会隐式安装。
 */
function RuntimePanel({ id }: { id: string }) {
  const ws = useWorkspace();
  const yaml = useYaml();
  const rt = useRuntime();
  const { toast } = useToast();
  const { t } = useTranslation();
  const wsId = ws.state.workspaceId;
  const kind = ws.state.spec?.services[id]?.kind ?? rt.state.services[id]?.kind ?? null;
  const tools = kind ? (RUNTIME_TOOLS[kind] ?? null) : null;
  const wsTc = ws.state.spec?.toolchain ?? null;
  const svcRunning = rt.state.services[id]?.state === "running";

  const [probe, setProbe] = useState<ToolchainProbeOut | null>(null);
  const [probing, setProbing] = useState(true);
  /** tool → 下拉草稿版本 */
  const [draft, setDraft] = useState<Partial<Record<RuntimeToolKey, string>>>({});
  /** tool → 正在保存的服务级版本选择 */
  const [switching, setSwitching] = useState<Partial<Record<RuntimeToolKey, boolean>>>({});

  const refresh = useCallback(async (force = false) => {
    setProbing(true);
    try {
      setProbe(await apiToolchainProbe(force));
    } catch {
      /* 面板级只读信息：失败按未探测展示，可点刷新重试 */
    } finally {
      setProbing(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const workspaceVersion = (tool: RuntimeToolKey): string | null => {
    if (!wsTc) return null;
    if (tool === "java") return wsTc.java ?? null;
    if (tool === "maven") return wsTc.maven ?? null;
    if (tool === "node") return wsTc.node ?? null;
    if (tool === "python") return wsTc.python ?? null;
    if (tool === "go") return wsTc.go ?? null;
    return null;
  };

  const serviceVersion = (tool: RuntimeToolKey): string | null => {
    const value = ws.state.spec?.services[id]?.env?.[SERVICE_RUNTIME_ENV_KEY[tool]];
    return value?.trim() || null;
  };

  const switchVersion = async (tool: RuntimeToolKey, version: string) => {
    const spec = ws.state.spec;
    const svc = spec?.services[id];
    if (!wsId || !yaml.state.hash || !spec || !svc || switching[tool]) return;
    setSwitching((prev) => ({ ...prev, [tool]: true }));
    try {
      const next: SuperTaskFile = {
        ...spec,
        services: {
          ...spec.services,
          [id]: { ...svc, env: { ...svc.env, [SERVICE_RUNTIME_ENV_KEY[tool]]: version } },
        },
      };
      const ok = await yaml.actions.saveForm(next);
      if (!ok) {
        toast(yaml.state.error ?? t("operations.savedFailed"), "err");
        return;
      }
      await ws.actions.refreshSpec();
      toast(t("pages.run.runtimeSwitched", { tool, version }), "ok");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setSwitching((prev) => ({ ...prev, [tool]: false }));
    }
  };

  if (!tools) {
    return (
      <div className="flex flex-col gap-3 p-4">
        <p className="rounded-[var(--r-md,12px)] border border-dashed border-[var(--line-strong,#d0d6e0)] p-5 text-center text-[0.8rem] text-[var(--t3,#8a8f98)]">
          {kind === "compose"
            ? t("pages.run.runtimeComposeHint")
            : t("pages.run.runtimeGenericHint")}
        </p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-[0.72rem] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
          {t("pages.run.runtimeTitle")}
        </span>
        <span className="min-w-0 flex-1 text-[0.72rem] text-[var(--t2,#62666d)]">{t("pages.run.runtimeSubtitle")}</span>
        <div className="flex shrink-0 items-center gap-2">
          {svcRunning ? (
            <Button size="sm" variant="warn" onClick={() => void rt.actions.restartOne(id)}>
              <RotateCw className="size-3.5" /> {t("pages.run.runtimeRestart")}
            </Button>
          ) : null}
          <Button size="sm" variant="soft" disabled={probing} onClick={() => void refresh(true)}>
            {probing ? <Loader2 className="size-3.5 animate-spin" /> : <RotateCw className="size-3.5" />}
            {t("pages.env.reprobe")}
          </Button>
        </div>
      </div>

      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        {tools.map((tool) => {
          const found = probe?.[tool.key] ?? null;
          // PATH 未命中时仍使用后端的本机安装枚举。Windows GUI 进程可能
          // 继承旧 PATH，但注册表/安装目录探测仍能确认工具已安装。
          const discovered = (probe?.installs ?? []).filter((i) => i.tool === tool.key);
          const selected = serviceVersion(tool.key) ?? workspaceVersion(tool.key);
          const candidates: { version: string; active: boolean }[] = discovered.map((install) => ({
            version: install.version,
            active: install.active,
          }));
          if (found?.found && found.version && !candidates.some((candidate) => candidate.version === found.version)) {
            candidates.push({ version: found.version, active: true });
          }
          // 每个主版本只保留一个本机安装：优先服务已选版本，其次 PATH 当前版本。
          const byMajor = new Map<string, (typeof candidates)[number]>();
          for (const candidate of candidates) {
            const major = versionMajor(candidate.version);
            const previous = byMajor.get(major);
            if (!previous || versionMatches(selected, candidate.version) || (!previous.active && candidate.active)) {
              byMajor.set(major, candidate);
            }
          }
          const options = Array.from(byMajor.values()).map((candidate) => candidate.version);
          const selectedLocal = options.find((version) => versionMatches(selected, version)) ?? null;
          const draftV = draft[tool.key] ?? selectedLocal ?? options[0] ?? "";
          const busy = switching[tool.key] === true;
          return (
            <div
              key={tool.key}
              className="flex flex-col gap-2 rounded-[var(--r-md,12px)] border border-[var(--line,#e6e6e6)] p-3 transition-colors duration-150 hover:border-[var(--line-strong,#d0d6e0)]"
            >
              <div className="flex min-w-0 flex-wrap items-center gap-2">
                <span className="text-[0.88rem] font-semibold text-[var(--t1,#222326)]">{tool.label}</span>
                {selected ? <Badge variant="secondary">{t("pages.run.runtimePinned", { version: selected })}</Badge> : null}
                {options.length > 0 ? (
                  <span className="font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.run.runtimeInstalled")}</span>
                ) : probing ? (
                  <span className="font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.run.runtimeProbing")}</span>
                ) : (
                  <span className="font-mono text-[0.72rem] text-[var(--st-warn,#9a6700)]">{t("pages.run.runtimeMissing")}</span>
                )}
              </div>
              {busy ? (
                <div className="flex items-center gap-2 rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f3f4f5)] px-2 py-1.5 text-[0.75rem] text-[var(--t2,#62666d)]">
                  <Loader2 className="size-3.5 shrink-0 animate-spin" />
                  <span className="min-w-0 flex-1 truncate">
                    {t("pages.run.runtimeSwitching")}
                  </span>
                </div>
              ) : (
                <div className="flex flex-wrap items-center gap-2">
                  {options.length > 0 ? (
                    <>
                      <select
                        className="h-8 min-w-0 cursor-pointer rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] px-1.5 font-mono text-[0.75rem] text-[var(--t1,#222326)]"
                        value={draftV}
                        onChange={(e) => setDraft((prev) => ({ ...prev, [tool.key]: e.target.value }))}
                        aria-label={t("pages.env.versionAria", { tool: tool.label })}
                      >
                        {options.map((v) => (
                          <option key={v} value={v}>{v}</option>
                        ))}
                      </select>
                      <Button
                        size="sm"
                        variant="default"
                        disabled={!wsId || !yaml.state.hash || options.length < 2 || versionMatches(selected, draftV)}
                        title={t("pages.run.runtimeSwitchTitle", { tool: tool.label, version: draftV })}
                        onClick={() => void switchVersion(tool.key, draftV)}
                      >
                        {t("pages.run.runtimeSwitch")}
                      </Button>
                    </>
                  ) : (
                    <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.run.runtimeNoLocalVersions")}</span>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>

      <p className="text-[0.72rem] leading-relaxed text-[var(--t3,#8a8f98)]">{t("pages.run.runtimeHint")}</p>
    </div>
  );
}

/* ---------------- health panel ---------------- */

/** 长 URL 拆成「路径优先 + host 次要」，完整串放 title / 复制。hint 为 i18n key。 */
function healthTarget(h: ServiceSpec["health"] | undefined, port: number | null | undefined) {
  if (!h || h.type === "none") {
    return { kind: "none" as const, path: "unset", pathValue: "", host: "", full: "", hintKey: "pages.run.healthHintNone" };
  }
  if (h.type === "tcp") {
    const p = port ?? null;
    return {
      kind: "tcp" as const,
      path: p != null ? `:${p}` : "noPort",
      pathValue: p != null ? `:${p}` : "",
      host: "tcp",
      full: p != null ? `tcp://127.0.0.1:${p}` : "",
      hintKey: "pages.run.healthHintTcp",
    };
  }
  const raw = (h.http ?? "").trim();
  if (!raw) {
    return { kind: "http" as const, path: "—", pathValue: "—", host: "", full: "", hintKey: "pages.run.healthHintHttp" };
  }
  try {
    const u = new URL(raw);
    const path = `${u.pathname || "/"}${u.search}`;
    return { kind: "http" as const, path, pathValue: path, host: u.host, full: raw, hintKey: "pages.run.healthHintHttp" };
  } catch {
    return { kind: "http" as const, path: raw, pathValue: raw, host: port != null ? `port ${port}` : "", full: raw, hintKey: "pages.run.healthHintHttp" };
  }
}

function HealthPanel({ svc, spec }: { svc: ServiceRuntimeView; spec: ServiceSpec | undefined }) {
  const { t } = useTranslation();
  const h = spec?.health;
  const last = svc.health;
  const target = healthTarget(h, svc.port);
  const { toast } = useToast();
  const watching = svc.state === "running" || svc.state === "unhealthy";
  // 状态枚举：unset / ok / bad / probing / paused（文案走 pages.run.health*）
  const statusKey: "unset" | "ok" | "bad" | "probing" | "paused" = !h || h.type === "none"
    ? "unset"
    : last
      ? last.ok ? "ok" : "bad"
      : watching ? "probing" : "paused";
  const failReason = svc.last_error ? svc.last_error : watching ? null : t("pages.run.healthPausedReason");

  return (
    <div className="flex flex-col gap-4 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <span
          className={cn(
            "rounded-[var(--r-sm,8px)] px-2.5 py-1 text-[0.82rem] font-semibold",
            statusKey === "ok" && "bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]",
            statusKey === "bad" && "bg-[var(--st-danger-tint,#fdecec)] text-[var(--st-danger,#dc2626)]",
            (statusKey === "unset" || statusKey === "paused" || statusKey === "probing") &&
              "bg-[var(--surface-2,#f3f4f5)] text-[var(--t2,#62666d)]",
          )}
        >
          {t(`pages.run.health_${statusKey}`)}
        </span>
        <span className="inline-flex h-5 items-center gap-1 rounded-full bg-[var(--surface-2,#f3f4f5)] px-2 text-[11px] font-medium leading-none text-[var(--t2,#62666d)]">
          <span
            className={cn(
              "size-1.5 rounded-full",
              watching ? "animate-pulse bg-[var(--st-ok,#27a644)]" : "bg-[var(--t3,#8a8f98)]",
            )}
          />
          {watching ? "watching" : "paused"}
        </span>
        {h ? (
          <Badge variant="outline" className="font-mono text-[10px] uppercase">
            {h.type}
          </Badge>
        ) : null}
        {h ? (
          <span className="font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]">
            {h.interval_secs}s / {h.timeout_secs}s
          </span>
        ) : null}
      </div>

      <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] p-3">
        <div className="mb-1 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
          {t("pages.run.healthTarget")}
          <span className="ml-auto font-normal normal-case tracking-normal">{t(target.hintKey)}</span>
        </div>
        <div className="flex min-w-0 items-start gap-2">
          <div className="min-w-0 flex-1">
            <div className="truncate font-mono text-[0.95rem] font-semibold text-[var(--t1,#222326)]" title={target.full || target.path}>
              {target.path === "unset" ? t("pages.run.healthUnset") : target.path === "noPort" ? t("pages.run.healthNoPort") : target.path}
            </div>
            {target.host ? (
              <div className="mt-0.5 truncate font-mono text-[0.72rem] text-[var(--t2,#62666d)]" title={target.full}>
                {target.host === "tcp" ? t("pages.run.healthTcpHost") : target.host}
              </div>
            ) : null}
          </div>
          {target.full ? (
            <Button
              size="sm"
              variant="outline"
              className="shrink-0 gap-1"
              onClick={() => {
                void navigator.clipboard?.writeText(target.full);
                toast(t("pages.run.healthCopied"), "ok");
              }}
            >
              <Copy className="size-3.5" /> {t("common.copy")}
            </Button>
          ) : null}
        </div>
      </div>

      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] p-3">
          <div className="mb-2 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
            {t("pages.run.recentResult")}
            <span className="ml-auto flex items-center gap-2 text-[10px] font-normal normal-case leading-none tracking-normal">
              <span><span className="mr-1 inline-block size-2 rounded-sm bg-[var(--st-ok,#27a644)]" />{t("pages.run.legendOk")}</span>
              <span><span className="mr-1 inline-block size-2 rounded-sm bg-[var(--st-warn,#9a6700)]" />{t("pages.run.legendSlow")}</span>
              <span><span className="mr-1 inline-block size-2 rounded-sm bg-[var(--st-danger,#dc2626)]" />{t("pages.run.legendFail")}</span>
            </span>
          </div>
          <HealthSparkline ok={last?.ok} />
          <div className="mt-2 flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-1 text-[13px]">
            <span className={cn("font-medium", healthClass(last?.ok))}>
              {last ? (last.ok ? t("pages.run.health_ok") : t("pages.run.health_bad")) : t("pages.run.noResultYet")}
            </span>
            {last?.detail ? (
              <span className="min-w-0 truncate font-mono text-[12px] text-[var(--t2,#62666d)]" title={last.detail}>
                {last.detail}
              </span>
            ) : null}
            {last?.at_ms ? (
              <span className="font-mono text-[11px] text-[var(--t3,#8a8f98)]">{fmtTime(last.at_ms)}</span>
            ) : null}
          </div>
        </div>
        <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] p-3">
          <div className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">{t("pages.run.failReason")}</div>
          <p
            className={cn(
              "text-[0.8rem] leading-relaxed",
              failReason && svc.last_error ? "text-[var(--st-danger,#dc2626)]" : "text-[var(--t2,#62666d)]",
            )}
          >
            {failReason ?? t("pages.run.failNone")}
          </p>
        </div>
      </div>
    </div>
  );
}

/* ---------------- config panel ---------------- */

function ConfigPanel({ id }: { id: string }) {
  const ws = useWorkspace();
  const { t } = useTranslation();
  const spec = ws.state.spec?.services[id];
  if (!spec) return null;
  const text = serviceYamlFragment(id, spec);
  return (
    <div className="flex flex-col gap-3 p-4">
      <div className="flex items-center justify-between">
        <div className="text-sm font-medium">{t("pages.run.rawFragment")}</div>
        <Button variant="outline" size="sm" asChild className="gap-1">
          <NavLink to="/config">{t("pages.run.editInConfig")}</NavLink>
        </Button>
      </div>
      <pre className="overflow-auto rounded-lg border border-[var(--line,#e6e6e6)] bg-[#FBFBFC] p-3 font-mono text-[12px] leading-relaxed text-[var(--t2,#62666d)]">
        {text}
      </pre>
      <p className="text-[11px] text-[var(--t3,#8a8f98)]">
        {t("pages.run.rawFragmentHint")}
      </p>
    </div>
  );
}

/* ---------------- detail (service) ---------------- */

function fmtMemBytes(n: number | null | undefined): string {
  if (n == null) return "—";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MiB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GiB`;
}

function ServiceDetail({ id, compact }: { id: string; compact: boolean }) {
  const rt = useRuntime();
  const ws = useWorkspace();
  const runtime = useRuntime();
  const { t } = useTranslation();
  const [tab, setTab] = useState<"logs" | "vars" | "ports" | "runtime" | "health" | "config" | "metrics" | "terminal" | "container" | "proxy">("logs");
  const [confirmStop, setConfirmStop] = useState(false);
  const [building, setBuilding] = useState(false);
  const [copied, setCopied] = useState(false);
  const { toast } = useToast();
  const svc = rt.state.services[id];
  const spec = ws.state.spec?.services[id];

  if (!svc) return null;
  const isRunning = svc.state === "running";
  const isBusy = svc.state === "starting" || svc.state === "stopping" || svc.state === "building";
  const external = isRunning && svc.managed === false;
  const portConflict = isPortConflict(svc);
  const isCompose = svc.kind === "compose";
  const source: LogSource = { kind: "service", id };
  const dockerTop = ws.state.spec?.docker ?? null;
  const cmd = spec
    ? isCompose
      ? // 1.3 §5.2：up 必带 --no-deps，顺序唯一真源是 SuperTask 依赖图
        `docker compose${dockerTop?.compose_file ? ` -f ${dockerTop.compose_file}` : ""}${dockerTop?.project_name ? ` -p ${dockerTop.project_name}` : ""} up -d --no-deps ${spec.service ?? id}`
      : serviceCmd(id, spec)
    : "";
  const jarService = spec?.kind === "spring-boot" && spec.launch === "jar";
  const composeService = spec?.kind === "compose";
  const buildJar = async () => {
    const workspaceId = ws.state.workspaceId;
    if (!workspaceId || (!jarService && !composeService) || building) return;
    setBuilding(true);
    try {
      await apiRuntimeBuild(workspaceId, id);
      toast(t("pages.run.buildStarted", { id }), "info");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setBuilding(false);
    }
  };

  const stack = isCompose
    ? t("pages.run.stackCompose", { service: spec?.service ?? id })
    : svc.kind === "node"
      ? t("pages.run.stackNode", { pm: spec?.package_manager ?? "npm" })
      : spec?.module && spec.module !== "."
        ? t("pages.run.stackSpringModule", { module: spec.module })
        : t("pages.run.stackSpring");
  const topo = spec?.depends_on?.length ? t("pages.run.dependsOn", { deps: spec.depends_on.join(", ") }) : t("pages.run.noDeps");

  type DetailTab = "logs" | "vars" | "ports" | "runtime" | "health" | "config" | "metrics" | "terminal" | "container" | "proxy";
  const tabs: { k: DetailTab; label: string; icon: typeof FileText }[] = [
    { k: "logs", label: t("nav.logs"), icon: ScrollText },
    { k: "terminal", label: t("pages.run.tabTerminal"), icon: SquareTerminal },
    // 「环境」Tab 拆分：变量（env vars + 生效快照）、端口（检查/建议/改端口）、环境（运行时工具版本）
    { k: "vars", label: t("pages.run.tabEnv"), icon: KeyRound },
    { k: "ports", label: t("pages.run.tabPorts"), icon: ArrowLeftRight },
    { k: "runtime", label: t("pages.run.tabRuntime"), icon: Layers },
    { k: "health", label: t("pages.run.tabHealth"), icon: HeartPulse },
    { k: "config", label: t("nav.config"), icon: SlidersHorizontal },
    { k: "metrics", label: t("pages.run.tabMetrics"), icon: BarChart3 },
    // 1.3：容器 Tab 仅 compose 服务显示（镜像/容器 ID/healthcheck/退出码，只读）
    ...(isCompose ? [{ k: "container" as DetailTab, label: t("nav.docker"), icon: Container }] : []),
    // 1.6：代理 Tab 仅网关已配置且启用时显示（本服务视角：网关状态 + 指向本服务的路由）
    ...(rt.state.gateway ? [{ k: "proxy" as DetailTab, label: t("pages.run.lockProxy"), icon: Globe }] : []),
  ];

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* head */}
      <div className={cn("flex items-center gap-3 border-b border-[var(--line)] px-4 py-3.5", compact && "px-3 py-2.5")}>
        <div className="flex min-w-0 flex-1 items-center gap-2.5 overflow-x-clip">
          <StatusDot state={svc.state} size={10} />
          <h1 className="min-w-0 truncate text-[1.15rem] font-bold tracking-tight text-[var(--t1)]">{id}</h1>
          <KindBadge kind={svc.kind} buildTool={spec?.build_tool} />
          {external ? (
            <span
              className="inline-flex h-5 items-center shrink-0 rounded-full bg-[var(--surface-2)] px-2 text-[11px] font-medium leading-none text-[var(--t2)]"
              title={t("pages.run.externalMonitorTitle")}
            >
              {t("pages.run.externalMonitor")}
            </span>
          ) : null}
          {portConflict ? (
            <span
              className="inline-flex h-5 items-center shrink-0 rounded-full border border-[var(--st-danger-ring)] bg-[var(--st-danger-tint)] px-2 text-[11px] font-medium leading-none text-[var(--st-danger)]"
              title={svc.last_error ?? t("pages.run.portConflictHint")}
            >
              {t("pages.run.portConflict")}
            </span>
          ) : null}
          <StatusChip
            state={svc.state}
            extra={isRunning && svc.started_at_ms ? fmtDuration(svc.started_at_ms) : undefined}
          />
          {isRunning && rt.state.metrics[id]?.cpu_percent != null ? (
            <span
              className="inline-flex h-5 items-center shrink-0 gap-1 rounded-full bg-[var(--surface-2)] px-2 text-[11px] font-medium leading-none text-[var(--t2)]"
              title={t("pages.run.metaCpu")}
            >
              <Cpu className="size-3" aria-hidden />
              {rt.state.metrics[id]?.cpu_percent?.toFixed(1)}%
            </span>
          ) : null}
          {isRunning && rt.state.metrics[id]?.memory_bytes != null ? (
            <span
              className="inline-flex h-5 items-center shrink-0 gap-1 rounded-full bg-[var(--surface-2)] px-2 text-[11px] font-medium leading-none text-[var(--t2)]"
              title={t("pages.run.metaMemory")}
            >
              <MemoryStick className="size-3" aria-hidden />
              {fmtMemBytes(rt.state.metrics[id]?.memory_bytes)}
            </span>
          ) : null}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <IdeOpenMenu variant="button" />
          {jarService || composeService ? (
            <Button size="sm" variant="secondary" className="gap-1" onClick={() => void buildJar()} disabled={building || isBusy}>
              {building ? <Loader2 className="size-3.5 animate-spin" /> : <PackagePlus className="size-3.5" />}
              {building ? t("pages.run.building") : jarService ? t("pages.run.buildJar") : t("pages.docker.buildImage")}
            </Button>
          ) : null}
          {isRunning ? (
            <Button
              size="sm"
              variant="warn"
              className="gap-1"
              disabled={isBusy}
              title={external ? t("pages.run.restartExternalTitle") : undefined}
              onClick={() => runtime.actions.restartOne(id)}
            >
              {isBusy ? <Loader2 className="size-3.5 animate-spin" /> : <RotateCw className="size-3.5" />} {t("common.restart")}
            </Button>
          ) : null}
          {isRunning || svc.state === "starting" ? (
            <Button
              size="sm"
              variant="destructive"
              className="gap-1"
              disabled={isBusy}
              onClick={() => (isRunning ? setConfirmStop(true) : runtime.actions.stopOne(id))}
            >
              {isBusy ? <Loader2 className="size-3.5 animate-spin" /> : <Square className="size-3.5" />} {t("common.stop")}
            </Button>
          ) : (
            <Button
              size="sm"
              variant="default"
              className="gap-1"
              disabled={isBusy || portConflict}
              title={portConflict ? (svc.last_error ?? t("pages.run.portConflictHint")) : undefined}
              onClick={() => runtime.actions.startOne(id)}
            >
              {isBusy ? <Loader2 className="size-3.5 animate-spin" /> : <Play className="size-3.5" />}{" "}
              {svc.state === "exited" || svc.last_error ? t("pages.run.retryStart") : t("common.start")}
            </Button>
          )}
        </div>
      </div>

      <ConfirmDialog
        open={confirmStop}
        title={t("pages.run.stopConfirmTitle", { id })}
        description={
          external
            ? t("pages.run.stopConfirmExternal")
            : t("pages.run.stopConfirmDesc")
        }
        confirmText={t("pages.run.stopService")}
        cancelText={t("common.cancel")}
        destructive
        onConfirm={() => {
          setConfirmStop(false);
          runtime.actions.stopOne(id);
        }}
        onCancel={() => setConfirmStop(false)}
      />

      {/* command line：深色终端风，与浅色 meta 条形成层次 */}
      <div className="mx-4 mt-3.5 flex items-center gap-2.5 rounded-[var(--r-md)] border border-[var(--st-cmd-border)] bg-[var(--st-cmd-bg)] py-2 pl-3.5 pr-1.5 font-mono shadow-[var(--shadow-1)]">
        <span className="shrink-0 font-bold text-[var(--st-cmd-prompt)]" aria-hidden>
          $
        </span>
        <span className="min-w-0 flex-1 truncate text-[0.78rem] leading-5 text-[var(--st-cmd-fg)]" title={cmd}>
          {cmd}
        </span>
        <button
          className={cn(
            "flex shrink-0 items-center gap-1 rounded-[var(--r-sm)] px-1.5 py-1 text-[0.68rem] transition-all duration-150",
            copied
              ? "text-[var(--st-ok)]"
              : "text-[var(--st-cmd-muted)] hover:bg-white/10 hover:text-[var(--st-cmd-fg)]",
          )}
          title={copied ? t("common.copied") : t("pages.run.copyCmd")}
          onClick={() => {
            void copyText(cmd).then((ok) => {
              if (!ok) {
                toast(t("pages.run.copyCmdFailed"), "err");
                return;
              }
              setCopied(true);
              window.setTimeout(() => setCopied(false), 2500);
            });
          }}
        >
          {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
        </button>
      </div>

      {/* meta strip：单行不换行；拓扑字段 flex-1 占剩余宽度截断（见 Meta truncate），其余字段 shrink-0 不被挤压 */}
      <div className="mx-4 mt-2.5 flex flex-nowrap items-center gap-x-[1.1rem] overflow-hidden rounded-[var(--r-md)] border border-[var(--line)] bg-[var(--surface-2)] px-3.5 py-2.5">
        {svc.port != null ? (
          <Meta k={t("pages.run.metaPort")} v={<PortLink port={svc.port} disabled={!isRunning} className="font-semibold text-[var(--st-info)]" />} />
        ) : (
          <Meta k={t("pages.run.metaPort")} v="—" />
        )}
        <Meta k={t("pages.run.metaStack")} v={stack} />
        <Meta k={t("pages.run.metaTopo")} v={topo} muted truncate title={topo} />
        <Meta
          k="PID"
          mono
          v={svc.pid != null ? `${svc.pid}` : isCompose ? t("pages.run.containerManaged") : "—"}
        />
        <Meta k={t("pages.run.metaUptime")} mono ok={isRunning} v={svc.started_at_ms ? fmtDuration(svc.started_at_ms) : "—"} />
      </div>

      {/* segbar */}
      <div className="flex items-center gap-2 overflow-x-auto border-b border-[var(--line)] px-3 py-2.5">
        <div className="inline-flex items-center gap-0.5 rounded-[var(--r-sm)] bg-[var(--surface-2)] p-0.5">
          {tabs.map((tabItem) => (
            <button
              key={tabItem.k}
              onClick={() => setTab(tabItem.k)}
              className={cn(
                "flex items-center gap-1 rounded-[6px] px-3 py-1.5 text-[0.76rem] font-semibold transition-all duration-150",
                tab === tabItem.k
                  ? "bg-[var(--surface)] text-[var(--t1)] shadow-[var(--shadow-1),inset_0_0_0_1px_var(--line)]"
                  : "text-[var(--t2)] hover:bg-[var(--surface-3)] hover:text-[var(--t1)]",
              )}
            >
              <tabItem.icon className="size-3.5" /> {tabItem.label}
            </button>
          ))}
        </div>
      </div>

      {/* panels：日志等宽区域内部滚动，不外层滚动 */}
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        {tab === "logs" ? (
          <LogView
            source={source}
            className="min-h-0 flex-1"
            height="100%"
            extraActions={({ lines }) => (
              <AiExplainButton
                lines={lines}
                source={source}
                serviceKind={svc.kind}
                servicePort={svc.port}
                serviceState={svc.state}
              />
            )}
          />
        ) : null}
        {tab === "terminal" ? (
          <TerminalView
            workspaceId={ws.state.workspaceId ?? ""}
            serviceId={id}
            className="min-h-0 flex-1"
          />
        ) : null}
        {tab === "vars" ? (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <EnvPanel id={id} compact={compact} />
          </div>
        ) : null}
        {tab === "ports" ? (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <PortsPanel id={id} compact={compact} />
          </div>
        ) : null}
        {tab === "runtime" ? (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <RuntimePanel key={id} id={id} />
          </div>
        ) : null}
        {tab === "health" ? (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <HealthPanel svc={svc} spec={spec} />
          </div>
        ) : null}
        {tab === "config" ? (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <ConfigPanel id={id} />
          </div>
        ) : null}
        {tab === "metrics" ? (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <MetricsPanel id={id} metric={rt.state.metrics[id] ?? null} compose={isCompose} />
          </div>
        ) : null}
        {tab === "container" ? (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <ContainerPanel id={id} />
          </div>
        ) : null}
        {tab === "proxy" ? (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <ProxyPanel id={id} />
          </div>
        ) : null}
      </div>
    </div>
  );
}

function MetricsPanel({ id, metric, compose = false }: { id: string; metric: ServiceMetrics | null; compose?: boolean }) {
  const { t } = useTranslation();
  const emptyHint = compose
    ? t("pages.run.metricsComposeHint")
    : t("pages.run.metricsEmptyHint", { id });
  return (
    <div className="grid grid-cols-1 gap-3 p-4 sm:grid-cols-3">
      <MetricTile icon={<Cpu className="size-4" />} label="CPU" value={metric?.cpu_percent == null ? "—" : `${metric.cpu_percent.toFixed(1)}%`} />
      <MetricTile icon={<HardDrive className="size-4" />} label={t("pages.run.metaMemory")} value={fmtMemBytes(metric?.memory_bytes)} />
      <MetricTile icon={<Boxes className="size-4" />} label={t("pages.run.metaProcTree")} value={metric?.process_count == null ? "—" : t("pages.run.procCount", { n: metric.process_count })} />
      <div className="sm:col-span-3 text-[0.72rem] text-[var(--t3,#8a8f98)]">
        {metric ? t("pages.run.lastSample", { time: new Date(metric.sampled_at_ms).toLocaleTimeString() }) : emptyHint}
      </div>
    </div>
  );
}

/* ---------------- container panel（1.3 compose 服务「容器」Tab） ---------------- */

/**
 * compose 服务容器信息（只读，数据来自 docker.probe + docker.ps）：
 * 镜像、容器 ID、compose healthcheck 状态（只展示，不参与状态机）、最近退出码。
 */
function ContainerPanel({ id }: { id: string }) {
  const ws = useWorkspace();
  const rt = useRuntime();
  const { t } = useTranslation();
  const [probe, setProbe] = useState<DockerProbe | null>(null);
  const [containers, setContainers] = useState<ContainerSummary[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    const workspaceId = ws.state.workspaceId;
    if (!workspaceId) return;
    setLoading(true);
    setError(null);
    try {
      const p = await apiDockerProbe(false);
      setProbe(p);
      setContainers(p.found && p.running ? (await apiDockerPs(workspaceId)).containers : null);
    } catch (e) {
      setError(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e));
    } finally {
      setLoading(false);
    }
  }, [ws.state.workspaceId]);

  useEffect(() => {
    void load();
  }, [load]);

  const spec = ws.state.spec?.services[id];
  const svc = rt.state.services[id];
  const composeService = spec?.service ?? id;
  const container = containers?.find((c) => c.service === composeService) ?? null;

  const engineOffline =
    probe && (!probe.found || !probe.running)
      ? !probe.found
        ? t("pages.run.dockerNotFoundHint")
        : t("pages.run.dockerDownHint")
      : null;

  if (loading && containers == null && !error) {
    return (
      <div className="flex items-center gap-2 p-4 text-[0.8rem] text-[var(--t2,#62666d)]">
        <Loader2 className="size-3.5 animate-spin" /> {t("pages.run.queryingContainer")}
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4 p-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2 text-sm font-medium">
          <Container className="size-4 text-[#12B76A]" /> {t("pages.run.containerInfo")}
          <span className="font-mono text-[11px] text-[var(--t3,#8a8f98)]">compose: {composeService}</span>
        </div>
        <Button variant="soft" size="sm" className="gap-1" disabled={loading} onClick={() => void load()}>
          {loading ? <Loader2 className="size-3.5 animate-spin" /> : <RotateCw className="size-3.5" />} {t("common.refresh")}
        </Button>
      </div>

      {error ? (
        <p className="text-[0.8rem] text-[var(--st-danger,#dc2626)]">{error}</p>
      ) : engineOffline ? (
        <div className="rounded-[var(--r-md,12px)] border border-dashed border-[var(--line-strong,#d0d6e0)] p-5 text-center text-[0.8rem] text-[var(--t3,#8a8f98)]">
          {engineOffline}
        </div>
      ) : !container ? (
        <div className="rounded-[var(--r-md,12px)] border border-dashed border-[var(--line-strong,#d0d6e0)] p-5 text-center text-[0.8rem] text-[var(--t3,#8a8f98)]">
          {t("pages.run.noContainer", { service: composeService })}
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] p-3">
            <div className="mb-1 text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">{t("pages.docker.colImage")}</div>
            <div className="break-all font-mono text-[0.85rem] font-semibold text-[var(--t1,#222326)]">{container.image}</div>
          </div>
          <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] p-3">
            <div className="mb-1 text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">{t("pages.docker.colContainerId")}</div>
            <div className="font-mono text-[0.85rem] font-semibold text-[var(--t1,#222326)]" title={container.container_id}>
              {container.container_id.startsWith("sha256:") ? container.container_id.slice(7, 19) : container.container_id.slice(0, 12)}
            </div>
          </div>
          <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] p-3">
            <div className="mb-1 text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
              {t("pages.run.healthcheck")}
              <span className="ml-1 font-normal normal-case tracking-normal">{t("pages.run.healthcheckNote")}</span>
            </div>
            {container.health ? (
              <span
                className={cn(
                  "inline-flex h-5 items-center rounded-full px-2 font-mono text-[11px] font-semibold leading-none",
                  container.health === "healthy"
                    ? "bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]"
                    : container.health === "unhealthy"
                      ? "bg-[var(--st-danger-tint,#fdecec)] text-[var(--st-danger,#dc2626)]"
                      : "bg-[var(--st-warn-tint,#fff8e1)] text-[var(--st-warn,#9a6700)]",
                )}
              >
                {container.health}
              </span>
            ) : (
              <span className="text-[0.8rem] text-[var(--t3,#8a8f98)]">{t("pages.run.healthcheckNone")}</span>
            )}
          </div>
          <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] p-3">
            <div className="mb-1 text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">{t("pages.run.lastExitCode")}</div>
            {svc?.last_exit ? (
              <span
                className={cn(
                  "font-mono text-[0.85rem] font-semibold",
                  svc.last_exit.code === 0 ? "text-[var(--st-ok-deep,#1e7e35)]" : "text-[var(--st-danger,#dc2626)]",
                )}
              >
                {svc.last_exit.code}
              </span>
            ) : (
              <span className="text-[0.8rem] text-[var(--t3,#8a8f98)]">—</span>
            )}
          </div>
        </div>
      )}

      <p className="text-[0.72rem] leading-relaxed text-[var(--t3,#8a8f98)]">
        {t("pages.run.containerFootnote")}
      </p>
    </div>
  );
}

function MetricTile({ icon, label, value }: { icon: ReactNode; label: string; value: string }) {
  return (
    <div className="rounded-[var(--r-md,12px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] p-3">
      <div className="flex items-center gap-2 text-[0.72rem] text-[var(--t3,#8a8f98)]">{icon}{label}</div>
      <div className="mt-2 font-mono text-[1.1rem] font-semibold text-[var(--t1,#222326)]">{value}</div>
    </div>
  );
}

function Meta({
  k,
  v,
  accent,
  muted,
  mono,
  ok,
  truncate,
  title,
}: {
  k: string;
  v: ReactNode;
  accent?: boolean;
  muted?: boolean;
  mono?: boolean;
  ok?: boolean;
  /** 值可能很长（如依赖拓扑）：按容器宽度单行截断出省略号，完整内容走 title */
  truncate?: boolean;
  title?: string;
}) {
  return (
    <span
      className={cn(
        "relative inline-flex items-center gap-[0.45rem] pl-[1.1rem] first:pl-0 [&:not(:first-child)]:before:absolute [&:not(:first-child)]:before:left-0 [&:not(:first-child)]:before:top-1/2 [&:not(:first-child)]:before:h-3 [&:not(:first-child)]:before:w-px [&:not(:first-child)]:before:-translate-y-1/2 [&:not(:first-child)]:before:bg-[var(--line-strong,#d0d6e0)] [&:not(:first-child)]:before:content-['']",
        // 单行 meta 条内：普通字段不收缩；truncate 字段吃掉剩余宽度后自身截断（flex 换行按 max-content，wrap 容器里长字段必然独占整行）
        truncate ? "min-w-0 flex-1 basis-0" : "shrink-0",
      )}
    >
      {/* k/v 同字号同行高（leading-none + items-center），只靠颜色/字重区分，保证基线齐平；k 恒单行不收缩，压缩只发生在 v 上 */}
      <span className="shrink-0 whitespace-nowrap text-[0.74rem] leading-none text-[var(--t3,#8a8f98)]">{k}</span>
      <span
        title={title}
        className={cn(
          "text-[0.74rem] font-medium leading-none",
          (mono || accent || ok) && "font-mono",
          accent && "font-semibold text-[var(--st-info)]",
          ok && "font-semibold text-[var(--st-ok-deep)]",
          muted && !accent && !ok && "font-normal text-[var(--t2)]",
          !accent && !ok && !muted && "text-[var(--t1)]",
          truncate && "truncate",
        )}
      >
        {v}
      </span>
    </span>
  );
}

/* ---------------- script card + detail ---------------- */

/**
 * 引擎同一工作区只有一份脚本槽（ScriptRuntimeView）：
 * id 匹配才有 runtime 视图，其余脚本一律按 idle 展示。
 */
function useScriptView(id: string) {
  const rt = useRuntime();
  const { toast } = useToast();
  const { t } = useTranslation();
  const view: ScriptRuntimeView | null = rt.state.script?.id === id ? rt.state.script : null;
  const isRunning = view?.state === "running";
  // 引擎限制：同时只能跑一个脚本（SCRIPT_BUSY）
  const anyRunning = rt.state.script?.state === "running";

  const run = async () => {
    try {
      await apiScriptRun(id);
      toast(t("pages.run.scriptStarted", { id }), "info");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };
  const cancel = async () => {
    try {
      await apiScriptCancel(id);
      toast(t("pages.run.stopSignalSent"), "info");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  return { view, isRunning, anyRunning, run, cancel };
}

function ScriptCard({ id, spec, selected, onOpen }: { id: string; spec: ScriptSpec; selected: boolean; onOpen: () => void }) {
  const { view, isRunning, anyRunning, run, cancel } = useScriptView(id);
  const { t } = useTranslation();
  const [confirmStop, setConfirmStop] = useState(false);

  const scriptState = scriptDotState(view ?? { state: "idle", last_exit: null, last_error: null });
  const foot = view?.last_error ? (
    <span className="block truncate text-[11px] text-[var(--st-danger)]" title={view.last_error}>⚠ {view.last_error}</span>
  ) : isRunning && view?.pid ? (
    <span className="font-mono text-[11px] text-[var(--t3)]">pid {view.pid}</span>
  ) : (
    <span className="text-[11px] text-[var(--t3)]">{t("pages.run.scriptTask", { n: spec.cmds.length })}</span>
  );

  return (
    <div
      onClick={onOpen}
      className={cn(
        "group/scr relative flex min-h-[6.1rem] shrink-0 cursor-pointer flex-col overflow-hidden rounded-[var(--r-md)] border bg-[var(--surface)] px-3 py-2.5 transition-all duration-150 ease-[var(--st-ease)]",
        selected
          ? "border-[var(--line-strong)] shadow-[var(--shadow-1),var(--st-select-ring)]"
          : "border-[var(--line)] hover:border-[var(--line-strong)] hover:bg-[var(--surface-2)]",
      )}
    >
      <div className="flex items-center gap-2">
        <StatusDot state={scriptState} size={8} />
        <span className="min-w-0 truncate text-[0.92rem] font-semibold tracking-tight text-[var(--t1)]" title={id}>{id}</span>
        <KindBadge kind="task" />
        <div className="ml-auto flex gap-1 opacity-60 transition-opacity group-hover/scr:opacity-100" onClick={(e) => e.stopPropagation()}>
          {isRunning ? (
            <button
              type="button"
              className="grid size-7 cursor-pointer place-items-center rounded-[var(--r-sm)] border border-transparent text-[var(--st-danger)] transition-colors duration-150 hover:border-[var(--st-danger-ring)] hover:bg-[var(--st-danger-tint)] disabled:cursor-not-allowed disabled:opacity-50"
              title={t("pages.run.stopScriptTitle")}
              onClick={() => setConfirmStop(true)}
            >
              <Square className="size-3.5" />
            </button>
          ) : (
            <button
              type="button"
              className="grid size-7 cursor-pointer place-items-center rounded-[var(--r-sm)] border border-transparent text-[var(--t3)] transition-colors duration-150 hover:border-[var(--line-strong)] hover:bg-[var(--surface-2)] hover:text-[var(--st-ok-deep)] disabled:cursor-not-allowed disabled:opacity-50"
              title={anyRunning ? t("pages.run.scriptBusy") : t("pages.run.runScriptTitle")}
              disabled={anyRunning}
              onClick={() => void run()}
            >
              <Play className="size-3.5" />
            </button>
          )}
        </div>
      </div>
      <div className="mt-2 truncate text-[11px] leading-snug text-[var(--t2)]">{spec.desc ?? spec.cmds.join(" ; ")}</div>
      <div className="mt-auto flex min-h-[1.05rem] items-center gap-2 overflow-hidden pt-1">
        <StatusChip state={scriptState} label={view ? scriptStateLabel(view) : stateLabel(scriptState)} />
        <span className="min-w-0 truncate">{foot}</span>
      </div>

      <ConfirmDialog
        open={confirmStop}
        title={t("pages.run.stopScriptConfirmTitle", { id })}
        description={t("pages.run.stopScriptConfirmDesc")}
        confirmText={t("pages.run.stopScript")}
        cancelText={t("common.cancel")}
        destructive
        onConfirm={() => {
          setConfirmStop(false);
          void cancel();
        }}
        onCancel={() => setConfirmStop(false)}
      />
    </div>
  );
}

/** 1.6 代理 Tab：本服务视角的网关切片——网关状态 + 指向本服务的路由（只读）。
 * 路由编辑统一在 /gateway 页（单一编辑入口）；这里复用 gateway.status /
 * gateway.start|stop|restart 与 `pages.gateway.*` 词条。 */
function ProxyPanel({ id }: { id: string }) {
  const rt = useRuntime();
  const ws = useWorkspace();
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { toast } = useToast();
  const gw = rt.state.gateway;
  const [status, setStatus] = useState<GatewayStatusOut | null>(null);
  const [busy, setBusy] = useState(false);
  const wsId = ws.state.workspaceId;
  const svcState = rt.state.services[id]?.state;

  const refresh = useCallback(async () => {
    if (!wsId) return;
    try {
      setStatus(await apiGatewayStatus(wsId));
    } catch {
      // 面板级只读信息：失败静默，网关/服务状态变化时会重试
    }
  }, [wsId]);

  useEffect(() => {
    void refresh();
  }, [refresh, id, gw?.state, svcState]);

  if (!gw || !wsId) return null;

  const act = async (fn: () => Promise<unknown>, okKey: string) => {
    setBusy(true);
    try {
      await fn();
      toast(t(okKey), "ok");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setBusy(false);
    }
  };

  const routes = status?.routes.filter((r) => r.target === id) ?? [];
  const gwActive = gw.state === "running" || gw.state === "starting" || gw.state === "unhealthy";

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-4 p-4">
      {/* 网关状态卡 */}
      <div className="rounded-[var(--r-lg,16px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] p-4">
        <div className="flex flex-wrap items-center gap-2">
          <Globe className="size-4 shrink-0 text-[var(--primary,#5E6AD2)]" />
          <span className="text-[0.9rem] font-semibold text-[var(--t1,#222326)]">{t("nav.gateway")}</span>
          <span className="font-mono text-[0.78rem] text-[var(--t2,#62666d)]">
            {gw.kind} · :{gw.port}
          </span>
          <span
            className={cn(
              "inline-flex h-5 items-center gap-1 rounded-full px-2 font-mono text-[10px] font-semibold leading-none",
              GATEWAY_STATE_TINT[gw.state],
            )}
          >
            <span className={cn("size-1.5 rounded-full", GATEWAY_STATE_DOT[gw.state])} />
            {t(`pages.gateway.state_${gw.state}`)}
          </span>
          <div className="ml-auto flex items-center gap-2">
            {gwActive ? (
              <>
                <Button variant="warn" size="sm" disabled={busy} onClick={() => void act(() => apiGatewayRestart(wsId), "pages.gateway.restartSent")}>
                  <RotateCw className="size-3.5" /> {t("pages.gateway.restart")}
                </Button>
                <Button variant="destructive" size="sm" disabled={busy} onClick={() => void act(() => apiGatewayStop(wsId), "pages.gateway.stopSent")}>
                  <Square className="size-3.5" /> {t("pages.gateway.stop")}
                </Button>
              </>
            ) : (
              <Button variant="default" size="sm" disabled={busy} onClick={() => void act(() => apiGatewayStart(wsId), "pages.gateway.startSent")}>
                {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Play className="size-3.5" />}
                {t("pages.gateway.start")}
              </Button>
            )}
          </div>
        </div>
        {gw.last_error ? (
          <div className="mt-2 rounded-[var(--r-sm,8px)] bg-[#fdecec] px-3 py-2 text-[0.78rem] text-[#dc2626]">{gw.last_error}</div>
        ) : null}
      </div>

      {/* 指向本服务的路由（只读；编辑在 /gateway） */}
      <div className="rounded-[var(--r-lg,16px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] p-4">
        <div className="flex items-center gap-2">
          <span className="text-[0.9rem] font-semibold text-[var(--t1,#222326)]">{t("pages.run.proxyRoutes")}</span>
          <span className="font-mono text-[11px] text-[var(--t3,#8a8f98)]">{routes.length}</span>
          <Button variant="outline" size="sm" className="ml-auto" onClick={() => navigate("/gateway")}>
            <Globe className="size-3.5" />
            {t("pages.run.proxyOpenGateway")}
          </Button>
        </div>
        {routes.length === 0 ? (
          <p className="mt-3 text-[0.8rem] text-[var(--t3,#8a8f98)]">{t("pages.run.proxyNoRoutes")}</p>
        ) : (
          <div className="mt-1 flex flex-col divide-y divide-[var(--line,#e6e6e6)]">
            {routes.map((r, i) => (
              <div key={i} className="flex flex-wrap items-center gap-2 py-2 text-[0.8rem]">
                {r.upstream_alive != null ? (
                  <span
                    className={cn("size-2 shrink-0 rounded-full", r.upstream_alive ? "bg-[#27a644]" : "bg-[#c3c6cc]")}
                    title={r.upstream_alive ? t("pages.gateway.upstreamAlive") : t("pages.gateway.upstreamDown")}
                  />
                ) : (
                  <span className="size-2 shrink-0" />
                )}
                <span className="rounded bg-[var(--surface-2,#f3f4f5)] px-1.5 py-0.5 font-mono text-[0.72rem] text-[var(--t2,#62666d)]">
                  {r.host ?? t("pages.run.proxyCatchAll")}
                </span>
                <span className="font-mono text-[var(--t1,#222326)]">{r.path}</span>
                <span className="text-[var(--t3,#8a8f98)]">→</span>
                <span className="font-mono text-[var(--t2,#62666d)]">:{r.target_port ?? "?"}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function ScriptDetail({ id }: { id: string }) {
  const ws = useWorkspace();
  const { view, isRunning, anyRunning, run, cancel } = useScriptView(id);
  const { t } = useTranslation();
  const [confirmStop, setConfirmStop] = useState(false);
  const [copied, setCopied] = useState(false);
  const { toast } = useToast();
  const spec = ws.state.spec?.scripts[id];
  if (!spec) return null;

  const source: LogSource = { kind: "script", id };
  const scriptState = scriptDotState(view ?? { state: "idle", last_exit: null, last_error: null });

  const copyCmds = () => {
    void copyText(spec.cmds.join("\n")).then((ok) => {
      if (!ok) {
        toast(t("pages.run.copyCmdFailed"), "err");
        return;
      }
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2500);
    });
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* head */}
      <div className="flex items-center gap-3 border-b border-[var(--line)] px-4 py-3.5">
        <div className="flex min-w-0 flex-1 items-center gap-2.5 overflow-x-clip">
          <StatusDot state={scriptState} size={10} />
          <h1 className="min-w-0 truncate text-[1.15rem] font-bold tracking-tight text-[var(--t1)]">{id}</h1>
          <KindBadge kind="task" />
          <StatusChip
            state={scriptState}
            label={scriptStateLabel(view ?? { state: "idle", last_exit: null, last_error: null })}
          />
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {isRunning ? (
            <Button size="sm" variant="destructive" className="gap-1" onClick={() => setConfirmStop(true)}>
              <Square className="size-3.5" /> {t("common.stop")}
            </Button>
          ) : (
            <Button
              size="sm"
              variant="default"
              className="gap-1"
              disabled={anyRunning}
              title={anyRunning ? t("errors.SCRIPT_BUSY") : undefined}
              onClick={() => void run()}
            >
              <Play className="size-3.5" /> {view?.last_exit ? t("pages.run.rerun") : t("pages.run.run")}
            </Button>
          )}
        </div>
      </div>

      <ConfirmDialog
        open={confirmStop}
        title={t("pages.run.stopScriptConfirmTitle", { id })}
        description={t("pages.run.stopScriptConfirmDesc")}
        confirmText={t("pages.run.stopScript")}
        cancelText={t("common.cancel")}
        destructive
        onConfirm={() => {
          setConfirmStop(false);
          void cancel();
        }}
        onCancel={() => setConfirmStop(false)}
      />

      {/* command block：cmds 顺序执行，深色终端风 */}
      <div className="mx-4 mt-3.5 rounded-[var(--r-md)] border border-[var(--st-cmd-border)] bg-[var(--st-cmd-bg)] py-2 pl-3.5 pr-1.5 font-mono shadow-[var(--shadow-1)]">
        <div className="flex items-start gap-2.5">
          <div className="min-w-0 flex-1">
            {spec.cmds.map((c, i) => (
              <div key={i} className="flex min-w-0 items-baseline gap-2 py-0.5">
                <span className="shrink-0 font-bold text-[var(--st-cmd-prompt)]" aria-hidden>
                  $
                </span>
                <span className="min-w-0 flex-1 break-all text-[0.78rem] leading-5 text-[var(--st-cmd-fg)]" title={c}>
                  {c}
                </span>
              </div>
            ))}
          </div>
          <button
            className={cn(
              "flex shrink-0 items-center gap-1 rounded-[var(--r-sm)] px-1.5 py-1 text-[0.68rem] transition-all duration-150",
              copied ? "text-[var(--st-ok)]" : "text-[var(--st-cmd-muted)] hover:bg-white/10 hover:text-[var(--st-cmd-fg)]",
            )}
            title={copied ? t("common.copied") : t("pages.run.copyCmd")}
            onClick={copyCmds}
          >
            {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
          </button>
        </div>
      </div>

      {/* meta strip */}
      <div className="mx-4 mt-2.5 flex flex-wrap items-center gap-x-[1.1rem] gap-y-1.5 rounded-[var(--r-md)] border border-[var(--line)] bg-[var(--surface-2)] px-3.5 py-2.5">
        <Meta k={t("pages.run.metaCmds")} v={t("pages.run.cmdCount", { n: spec.cmds.length })} />
        <Meta k={t("pages.run.metaCwd")} v={spec.cwd ?? t("pages.run.wsRoot")} muted />
        <Meta k={t("pages.run.metaTimeout")} v={`${spec.timeout_secs ?? 1800}s`} />
        <Meta k="PID" v={view?.pid != null ? `${view.pid}` : "—"} />
        <Meta k={t("pages.run.metaLog")} v={`script:${id}`} mono />
        <Meta
          k={t("pages.run.metaLastExit")}
          v={view?.last_exit ? t("pages.run.exitCode", { code: view.last_exit.code }) : "—"}
          ok={view?.last_exit?.code === 0}
        />
      </div>

      {/* logs：与日志页共用 LogView，来源 script:{id} */}
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden px-4 pb-4 pt-3">
        <LogView
          source={source}
          className="min-h-0 flex-1"
          height="100%"
          extraActions={({ lines }) => <AiExplainButton lines={lines} source={source} />}
        />
      </div>
    </div>
  );
}

/* ---------------- RunPage ---------------- */

/* ---------------- 1.7 §8.1 服务分组（纯呈现层；组序 = YAML 首现序，未分组最后） ---------------- */

const GROUP_COLLAPSED_KEY = "st:runGroupCollapsed";

function readGroupCollapsedPref(): Record<string, boolean> {
  try {
    return JSON.parse(localStorage.getItem(GROUP_COLLAPSED_KEY) ?? "{}");
  } catch {
    return {};
  }
}

type ServiceGroup = { key: string; label: string; ids: string[] };

function buildGroups(serviceIds: string[], spec: Record<string, ServiceSpec> | undefined, ungroupedLabel: string): ServiceGroup[] {
  const map = new Map<string, string[]>();
  for (const id of serviceIds) {
    const g = spec?.[id]?.group?.trim() ?? "";
    const key = g || "__ungrouped__";
    const list = map.get(key);
    if (list) list.push(id);
    else map.set(key, [id]);
  }
  // YAML 首现序：serviceIds 本身即 spec 序；未分组强制排最后
  const keys = [...map.keys()].sort((a, b) => {
    if (a === "__ungrouped__") return 1;
    if (b === "__ungrouped__") return -1;
    return 0;
  });
  return keys.map((k) => ({ key: k, label: k === "__ungrouped__" ? ungroupedLabel : k, ids: map.get(k)! }));
}

/**
 * 统计行「总数 · 运行 N」：整串走 font-mono 会让 CJK 词回退系统字体、与数字基线错位，
 * 这里文字用 UI 字体、数字单独 tabular-nums，并给运行数一个状态圆点，保证同高对齐。
 */
function RunningCounts({ total, running, className }: { total: number; running: number; className?: string }) {
  const { t } = useTranslation();
  return (
    <span className={cn("inline-flex items-center gap-1.5 leading-none text-[var(--t3,#8a8f98)]", className)}>
      <span className="font-mono tabular-nums text-[var(--t2,#62666d)]">{total}</span>
      <span aria-hidden className="text-[var(--line-strong,#d0d6e0)]">·</span>
      <span
        aria-hidden
        className={cn("size-1.5 shrink-0 rounded-full", running > 0 ? "bg-[var(--st-ok,#27a644)]" : "bg-[var(--line-strong,#d0d6e0)]")}
      />
      <span>{t("pages.run.runningShort")}</span>
      <span className={cn("font-mono tabular-nums", running > 0 ? "text-[var(--st-ok,#27a644)]" : "text-[var(--t2,#62666d)]")}>{running}</span>
    </span>
  );
}

function GroupHeader({
  label,
  ids,
  states,
  collapsed,
  onToggle,
  onStart,
  onStop,
}: {
  label: string;
  ids: string[];
  states: Record<string, string>;
  collapsed: boolean;
  onToggle: () => void;
  onStart: () => void;
  onStop: () => void;
}) {
  const { t } = useTranslation();
  const running = ids.filter((id) => states[id] === "running" || states[id] === "starting").length;
  return (
    <div className="flex items-center gap-1.5 px-1 pb-1 pt-2">
      <button
        type="button"
        onClick={onToggle}
        className="flex min-w-0 cursor-pointer items-center gap-1 rounded-[var(--r-sm,8px)] px-1 py-0.5 text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)] transition-colors duration-150 hover:bg-[var(--surface-2,#f3f4f5)] hover:text-[var(--t1,#222326)]"
        title={collapsed ? t("pages.run.expandList") : t("pages.run.collapseList")}
      >
        {collapsed ? <ChevronRight className="size-3.5" /> : <ChevronDown className="size-3.5" />}
        <span className="truncate">{label}</span>
      </button>
      <RunningCounts total={ids.length} running={running} className="text-[10px]" />
      <span className="ml-auto flex gap-1">
        <button
          type="button"
          onClick={onStart}
          disabled={running === ids.length}
          className="grid size-6 cursor-pointer place-items-center rounded-[var(--r-sm,8px)] border border-transparent text-[var(--t3,#8a8f98)] transition-colors duration-150 hover:border-[var(--st-accent-tint-line,#c9cff0)] hover:bg-[var(--st-accent-tint,#eef0fb)] hover:text-[var(--st-accent,#5e6ad2)] disabled:cursor-not-allowed disabled:opacity-50"
          title={t("pages.run.groupStart")}
        >
          <Play className="size-3" />
        </button>
        <button
          type="button"
          onClick={onStop}
          disabled={running === 0}
          className="grid size-6 cursor-pointer place-items-center rounded-[var(--r-sm,8px)] border border-transparent text-[var(--st-danger,#dc2626)] transition-colors duration-150 hover:border-[#FECACA] hover:bg-[var(--st-danger-tint,#fdecec)] disabled:cursor-not-allowed disabled:opacity-50"
          title={t("pages.run.groupStop")}
        >
          <Square className="size-3" />
        </button>
      </span>
    </div>
  );
}

export function RunPage() {
  const rt = useRuntime();
  const ws = useWorkspace();
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { compact } = useOutletContext<ShellCtx>();
  const [selected, setSelected] = useState<{ kind: "service" | "script"; id: string } | null>(null);

  const serviceIds = Object.keys(rt.state.services);
  const scriptIds = ws.state.spec ? Object.keys(ws.state.spec.scripts) : [];
  const running = serviceIds.filter((i) => rt.state.services[i].state === "running").length;
  const [cardWidth, setCardWidth] = useState(() => readRunCardWidthPref());

  // 1.7 §8.1：分组（spec.group 字段；无分组时渲染与旧版一致）
  const groups = useMemo(
    () => buildGroups(serviceIds, ws.state.spec?.services, t("pages.run.ungrouped")),
    // serviceIds 随 rt.state.services 引用变化而稳定，spec 变化重建
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [rt.state.services, ws.state.spec, t],
  );
  const hasGroups = groups.length > 1 || (groups.length === 1 && groups[0].key !== "__ungrouped__");
  const [groupCollapsed, setGroupCollapsed] = useState<Record<string, boolean>>(() => readGroupCollapsedPref());
  const [groupStopTarget, setGroupStopTarget] = useState<ServiceGroup | null>(null);

  const toggleGroup = (key: string) => {
    setGroupCollapsed((prev) => {
      const next = { ...prev, [key]: !prev[key] };
      try {
        localStorage.setItem(GROUP_COLLAPSED_KEY, JSON.stringify(next));
      } catch {
        /* 隐私模式等场景忽略 */
      }
      return next;
    });
  };

  const startGroup = async (ids: string[]) => {
    for (const id of ids) {
      try {
        await rt.actions.startOne(id);
      } catch {
        /* 单个失败继续其余（结果在卡片状态呈现） */
      }
    }
  };

  const stopGroup = async (ids: string[]) => {
    for (const id of ids) {
      try {
        await rt.actions.stopOne(id);
      } catch {
        /* 同上 */
      }
    }
  };

  const clampCardWidth = (w: number) => Math.min(RUN_CARD_MAX_WIDTH, Math.max(RUN_CARD_MIN_WIDTH, w));

  // 服务列表宽度拖拽：按下后由 window 收集 move/up，避免拖出把手丢失事件
  const onResizeHandlePointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = cardWidth;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    const onMove = (ev: PointerEvent) => {
      setCardWidth(clampCardWidth(startWidth + ev.clientX - startX));
    };
    const onUp = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      const next = clampCardWidth(startWidth + ev.clientX - startX);
      setCardWidth(next);
      writeRunCardWidthPref(next);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const sel =
    selected && selected.kind === "service" && rt.state.services[selected.id]
      ? selected
      : selected && selected.kind === "script" && ws.state.spec?.scripts[selected.id]
        ? selected
        : serviceIds[0]
          ? { kind: "service" as const, id: serviceIds[0] }
          : null;

  return (
    <div className="flex min-h-0 flex-1 overflow-hidden bg-[var(--bg)]">
      {/* card column：仅卡片列表自身滚动；宽度可拖拽（min/max 见 workspace-storage） */}
      <section
        className={cn(
          "flex shrink-0 flex-col border-r border-[var(--line)] bg-[var(--bg)] p-3",
          compact && "p-2",
        )}
        style={{ width: cardWidth }}
      >
        <div className="flex items-center gap-2 px-1 pb-2.5 pt-1">
              <span className="text-[11px] font-semibold uppercase tracking-[0.08em] text-[var(--t3)]">{t("pages.run.servicesHeader")}</span>
              <RunningCounts total={serviceIds.length} running={running} className="text-[11px]" />
            </div>
            <div className="flex min-h-0 flex-1 flex-col gap-2.5 overflow-y-auto">
              {serviceIds.length === 0 ? (
                <div className="shrink-0 rounded-lg border border-dashed border-[var(--line,#e6e6e6)] p-6 text-center text-sm text-[var(--t3,#8a8f98)]">
                  {t("pages.run.noServices")}
                </div>
              ) : hasGroups ? (
                groups.map((grp) => (
                  <div key={grp.key} className="flex shrink-0 flex-col gap-2">
                    <GroupHeader
                      label={grp.label}
                      ids={grp.ids}
                      states={Object.fromEntries(grp.ids.map((id) => [id, rt.state.services[id]?.state ?? "stopped"]))}
                      collapsed={!!groupCollapsed[grp.key]}
                      onToggle={() => toggleGroup(grp.key)}
                      onStart={() => void startGroup(grp.ids)}
                      onStop={() => setGroupStopTarget(grp)}
                    />
                    {!groupCollapsed[grp.key] &&
                      grp.ids.map((id) => (
                        <ServiceCard
                          key={id}
                          id={id}
                          svc={rt.state.services[id]}
                          spec={ws.state.spec?.services[id]}
                          selected={sel?.kind === "service" && sel.id === id}
                          onOpen={() => setSelected({ kind: "service", id })}
                        />
                      ))}
                  </div>
                ))
              ) : (
                serviceIds.map((id) => (
                  <ServiceCard
                    key={id}
                    id={id}
                    svc={rt.state.services[id]}
                    spec={ws.state.spec?.services[id]}
                    selected={sel?.kind === "service" && sel.id === id}
                    onOpen={() => setSelected({ kind: "service", id })}
                  />
                ))
              )}

              {scriptIds.length > 0 ? (
                <>
                  <div className="flex shrink-0 items-center gap-2 px-1 pb-2 pt-4">
                    <span className="text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">{t("pages.run.scriptsHeader")}</span>
                    <span className="font-mono text-[11px] text-[var(--t3,#8a8f98)]">{scriptIds.length}</span>
                  </div>
                  {scriptIds.map((id) => (
                    <ScriptCard
                      key={id}
                      id={id}
                      spec={ws.state.spec!.scripts[id]}
                      selected={sel?.kind === "script" && sel.id === id}
                      onOpen={() => setSelected({ kind: "script", id })}
                    />
                  ))}
                </>
              ) : null}

              {/* 1.6：网关状态行（独立于 services 列表；点击跳转 /gateway） */}
              {rt.state.gateway ? (
                <>
                  <div className="flex shrink-0 items-center gap-2 px-1 pb-2 pt-4">
                    <span className="text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">{t("pages.gateway.title")}</span>
                    <span className="font-mono text-[11px] text-[var(--t3,#8a8f98)]">
                      :{rt.state.gateway.port}
                    </span>
                  </div>
                  <button
                    type="button"
                    onClick={() => navigate("/gateway")}
                    className="flex w-full shrink-0 cursor-pointer items-center gap-2 rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] px-3 py-2 text-left transition-colors duration-150 hover:bg-[var(--surface-2,#f3f4f5)]"
                  >
                    <span
                      className={cn(
                        "size-2 shrink-0 rounded-full",
                        rt.state.gateway.state === "running"
                          ? "bg-[#27a644]"
                          : rt.state.gateway.state === "starting" || rt.state.gateway.state === "stopping"
                            ? "bg-[#d9a514]"
                            : rt.state.gateway.state === "exited" || rt.state.gateway.state === "unhealthy"
                              ? "bg-[#dc2626]"
                              : "bg-[#8a8f98]",
                      )}
                    />
                    <span className="truncate font-mono text-[0.8rem] text-[var(--t1,#222326)]">
                      {rt.state.gateway.kind}
                    </span>
                    <span className="ml-auto text-[0.7rem] text-[var(--t3,#8a8f98)]">
                      {t(`pages.gateway.state_${rt.state.gateway.state}`)}
                    </span>
                  </button>
                </>
              ) : null}
            </div>
      </section>

      {/* 宽度拖拽把手：压住右侧 border，hover / 拖拽时高亮 */}
      <div
        role="separator"
        aria-orientation="vertical"
        onPointerDown={onResizeHandlePointerDown}
        className="group relative -ml-px z-10 w-1.5 shrink-0 cursor-col-resize"
      >
        <div className="absolute inset-y-0 left-0 w-px bg-[var(--line-strong)] opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-active:bg-[var(--st-accent)] group-active:opacity-100" />
      </div>

      {/* detail：外层不滚动，滚动交给日志框体 / 各 Tab 面板 */}
      <section className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-[var(--bg)] px-4 pb-4 pt-3">
        {!sel ? (
          <div className="flex h-full items-center justify-center text-sm text-[var(--t3)]">{t("pages.run.selectDetail")}</div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[var(--r-lg)] border border-[var(--line)] bg-[var(--surface)] shadow-[var(--shadow-1)]">
            {sel.kind === "service" ? <ServiceDetail id={sel.id} compact={compact} /> : <ScriptDetail id={sel.id} />}
          </div>
        )}
      </section>

      {rt.state.error ? (
        <div
          className="fixed inset-0 z-[200] grid place-items-center bg-black/40"
          onClick={() => rt.actions.clearError()}
        >
          <div
            className="w-[420px] rounded-xl border border-[var(--st-danger-ring)] bg-[var(--surface)] p-5 shadow-xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="mb-2 text-sm font-semibold text-[var(--st-danger)]">{t("pages.run.startFailed")}</div>
            <p className="mb-4 whitespace-pre-wrap break-words text-sm text-[var(--t1)]">
              {rt.state.error}
            </p>
            <div className="flex justify-end">
              <Button onClick={() => rt.actions.clearError()}>{t("pages.run.ack")}</Button>
            </div>
          </div>
        </div>
      ) : null}

      {/* 1.7 §8.1：组级停止确认 */}
      <ConfirmDialog
        open={groupStopTarget !== null}
        title={t("pages.run.groupStopConfirmTitle", { group: groupStopTarget?.label ?? "" })}
        description={t("pages.run.groupStopConfirmDesc", { count: groupStopTarget?.ids.length ?? 0 })}
        destructive
        onConfirm={() => {
          const ids = groupStopTarget?.ids ?? [];
          setGroupStopTarget(null);
          void stopGroup(ids);
        }}
        onCancel={() => setGroupStopTarget(null)}
      />
    </div>
  );
}
