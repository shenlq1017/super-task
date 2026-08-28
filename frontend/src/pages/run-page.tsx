import { useEffect, useRef, useState, useCallback, type ReactNode } from "react";
import { useOutletContext, NavLink } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button, buttonVariants } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { cn } from "@/lib/utils";
import { copyText } from "@/lib/copy-text";
import { readCardsCollapsedPref, writeCardsCollapsedPref } from "@/lib/workspace-storage";
import { EnvVariablesEditor } from "@/components/env-variables-editor";
import { useRuntime } from "@/providers/runtime-provider";
import { useWorkspace } from "@/providers/workspace-provider";
import { useYaml } from "@/providers/yaml-provider";
import { useToast } from "@/components/ui/toast";
import { LogView } from "@/components/log-view";
import {
  STATE_META,
  StatusDot,
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
  apiScriptCancel,
  apiScriptRun,
  apiDockerProbe,
  apiDockerPs,
} from "../ipc/api";
import { IpcFailure } from "../ipc/protocol";
import type {
  ContainerSummary,
  DockerProbe,
  IdeTarget,
  LogSource,
  ScriptRuntimeView,
  ScriptSpec,
  ServiceMetrics,
  ServiceRuntimeView,
  ServiceSpec,
  SuperTaskFile,
} from "@/ipc/protocol";
import {
  Play,
  Square,
  RotateCw,
  Settings2,
  Activity,
  FileText,
  ExternalLink,
  Copy,
  Check,
  Lock,
  PanelLeftClose,
  PanelLeftOpen,
  Cpu,
  HardDrive,
  Boxes,
  Hammer,
  Loader2,
  Container,
} from "lucide-react";
import type { ShellCtx } from "../app/AppShell";

/* ---------------- helpers ---------------- */

function serviceCmd(id: string, s: ServiceSpec): string {
  if (s.kind === "compose") {
    // 1.3：compose 服务由引擎执行 `docker compose -f <file> up -d --no-deps <service>`
    return `docker compose up -d --no-deps ${s.service ?? id}`;
  }
  if (s.kind === "node") {
    const dir = s.dir ?? id;
    const script = s.script ?? "dev";
    return `npm --prefix ${dir} run ${script}`;
  }
  // 单模块（module "." 或缺省）省略 -pl，与引擎 plan_spring 的行为一致
  const module = s.module ?? "";
  return module === "." || module === "" ? "mvn spring-boot:run" : `mvn -pl ${module} spring-boot:run`;
}

function kindLabel(kind: string): string {
  if (kind === "node") return "NODE";
  if (kind === "compose") return "COMPOSE";
  if (kind === "task") return "TASK";
  return "SPRING";
}

function KindBadge({ kind, buildTool }: { kind: string; buildTool?: string | null }) {
  const color =
    kind === "node"
      ? "#2E90FA"
      : kind === "compose"
        ? "#12B76A"
        : kind === "task"
          ? "var(--t3,#8a8f98)"
          : "var(--st-accent,#5e6ad2)";
  return (
    <>
      <span className="inline-flex items-center gap-1 rounded-full bg-[var(--surface-2,#f3f4f5)] px-1.5 py-0.5 font-mono text-[10px] font-semibold uppercase text-[var(--t2,#62666d)]">
        <span className="size-1.5 rounded-full" style={{ background: color }} />
        {kindLabel(kind)}
      </span>
      {buildTool === "gradle" ? (
        <span className="rounded-full bg-[var(--surface-2,#f3f4f5)] px-1.5 py-0.5 font-mono text-[10px] font-semibold uppercase text-[var(--t2,#62666d)]">
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
        className={cn(buttonVariants({ variant: "outline", size: "sm" }), variant === "icon" && "size-7 px-0")}
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
  const meta = STATE_META[svc.state];
  const isRunning = svc.state === "running";
  const isBusy = svc.state === "starting" || svc.state === "stopping" || svc.state === "building";
  const external = isRunning && svc.managed === false;
  // 停止是杀整棵进程树的破坏性操作，二次确认；中断 starting 不弹
  const [confirmStop, setConfirmStop] = useState(false);

  const foot = svc.last_error
    ? <span className="block truncate text-[11px] text-[var(--st-danger,#dc2626)]" title={svc.last_error}>⚠ {svc.last_error}</span>
    : <span className="flex items-center gap-1 text-[11px] text-[var(--t3,#8a8f98)]">
        <span className="rounded-full bg-[var(--surface-2,#f3f4f5)] px-1.5 py-0.5 font-mono text-[10px]">
          {spec?.depends_on?.length ? t("pages.run.dependsOn", { deps: spec.depends_on.join(", ") }) : t("pages.run.noDeps")}
        </span>
      </span>;

  return (
    <div
      onClick={onOpen}
      className={cn(
        // group/svc 让指示条用 group-hover/svc 触发；transition-all 覆盖颜色/边框/阴影
        "group/svc relative flex h-[5.7rem] cursor-pointer flex-col overflow-hidden rounded-[var(--r-md,12px)] border bg-[var(--surface,#fff)] p-2.5 transition-all duration-150 ease-[var(--st-ease,cubic-bezier(.22,1,.36,1))]",
        selected
          // 原型：选中 = 极淡紫底 + 紫色淡外环（0 0 0 3px rgb(94 106 210 / .1)）
          ? "border-[rgb(94_106_210_/_0.45)] bg-[rgb(94_106_210_/_0.045)] shadow-[0_0_0_3px_rgb(94_106_210_/_0.1)]"
          : "border-[var(--line,#e6e6e6)] hover:border-[var(--line-strong,#d0d6e0)] hover:bg-[var(--surface-2,#f3f4f5)]",
      )}
    >
      {/* 原型：左侧 3px 紫/绿条；hover 滑出；running 持续显示绿；selected 持续显示紫（压过 running） */}
      <span
        aria-hidden
        className={cn(
          "absolute left-0 top-0 h-full w-[3px] origin-left transition-transform duration-200 ease-[cubic-bezier(.22,1,.36,1)]",
          selected || isRunning ? "scale-x-100" : "scale-x-0 group-hover/svc:scale-x-100",
        )}
        style={{
          background: selected
            ? "var(--st-accent,#5e6ad2)"
            : isRunning
              ? "var(--st-ok,#27a644)"
              : "var(--st-accent,#5e6ad2)",
        }}
      />
      <div className="flex items-center gap-2">
        <StatusDot state={svc.state} size={8} />
        <span className="truncate text-[0.88rem] font-semibold text-[var(--t1,#222326)]">{id}</span>
        <KindBadge kind={svc.kind} buildTool={spec?.build_tool} />
        {external ? (
          <span className="shrink-0 rounded-full bg-[var(--surface-2,#f3f4f5)] px-1.5 py-0.5 font-mono text-[10px] font-semibold uppercase text-[var(--t2,#62666d)]" title={t("pages.run.externalTitle")}>
            {t("pages.run.externalShort")}
          </span>
        ) : null}
        <div className="ml-auto flex gap-1 opacity-50 transition-opacity group-hover:opacity-100" onClick={(e) => e.stopPropagation()}>
          <IdeOpenMenu variant="icon" />
          {isRunning ? (
            <button
              type="button"
              className="grid size-[1.8rem] cursor-pointer place-items-center rounded-[var(--r-sm,8px)] border border-[var(--st-warn-line,#f0dcb0)] bg-[var(--st-warn-tint,#fff8e1)] text-[var(--st-warn,#9a6700)] transition-colors duration-150 hover:border-[#E0C080] hover:bg-[rgb(234_179_8_/_0.2)] disabled:cursor-not-allowed disabled:opacity-50"
              title={external ? t("pages.run.restartExternalTitle") : t("common.restart")}
              disabled={isBusy}
              onClick={() => runtime.actions.restartOne(id)}
            >
              <RotateCw className="size-3.5" />
            </button>
          ) : null}
          <button
            type="button"
            className={cn(
              "grid size-[1.8rem] cursor-pointer place-items-center rounded-[var(--r-sm,8px)] border transition-colors duration-150 disabled:cursor-not-allowed disabled:opacity-50",
              isRunning
                ? "border-transparent text-[var(--t3,#8a8f98)] hover:border-[#FECACA] hover:bg-[var(--st-danger-tint,#fdecec)] hover:text-[var(--st-danger,#dc2626)]"
                : "border-transparent text-[var(--t3,#8a8f98)] hover:border-[var(--line-strong,#d0d6e0)] hover:bg-[var(--surface-2,#f3f4f5)] hover:text-[var(--st-accent,#5e6ad2)]",
            )}
            title={isRunning ? t("common.stop") : t("common.start")}
            disabled={isBusy}
            onClick={() =>
              isRunning || svc.state === "starting"
                ? isRunning
                  ? setConfirmStop(true)
                  : runtime.actions.stopOne(id)
                : runtime.actions.startOne(id)
            }
          >
            {isRunning || svc.state === "starting" ? <Square className="size-3.5" /> : <Play className="size-3.5" />}
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

      <div className="mt-1.5 flex items-center gap-2 text-[11px] text-[var(--t2,#62666d)]">
        <span className="font-medium" style={{ color: meta.color }}>{stateLabel(svc.state)}</span>
        {svc.port ? <span className="font-mono">{svc.port}</span> : null}
        {svc.pid ? (
          <span className="font-mono">pid {svc.pid}</span>
        ) : svc.kind === "compose" ? (
          // 1.3 §5.3：compose 服务无宿主进程，pid 恒为 null
          <span
            className="rounded-full bg-[var(--surface-2,#f3f4f5)] px-1.5 py-0.5 font-mono text-[10px]"
            title={t("pages.run.containerManagedTitle")}
          >
            {t("pages.run.containerManaged")}
          </span>
        ) : null}
        {isRunning && svc.started_at_ms ? <span className="font-mono text-[var(--t3,#8a8f98)]">· {fmtDuration(svc.started_at_ms)}</span> : null}
      </div>

      <div className="mt-1.5 min-h-[1.05rem] overflow-hidden">{foot}</div>
    </div>
  );
}

/* ---------------- env panel ---------------- */

type PortCheck = {
  port: number;
  inUse: boolean;
  message: string;
};

function EnvPanel({ id, compact }: { id: string; compact: boolean }) {
  const ws = useWorkspace();
  const yaml = useYaml();
  const runtime = useRuntime();
  const { toast } = useToast();
  const { t } = useTranslation();
  const spec = ws.state.spec;
  const svc = spec?.services[id];
  const isRunning = runtime.state.services[id]?.state === "running";
  const [portDraft, setPortDraft] = useState<string>(String(svc?.port ?? ""));
  const [envDraft, setEnvDraft] = useState<Record<string, string>>(() => svc?.env ?? {});
  const [portBusy, setPortBusy] = useState(false);
  const [portCheck, setPortCheck] = useState<PortCheck | null>(null);
  const [portCandidates, setPortCandidates] = useState<number[]>([]);

  useEffect(() => {
    setPortDraft(String(svc?.port ?? ""));
    setEnvDraft(svc?.env ?? {});
    setPortCheck(null);
    setPortCandidates([]);
  }, [id, svc?.port, JSON.stringify(svc?.env)]);

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
      <section className="flex flex-col gap-2">
        <div className="flex items-center gap-2 text-[0.72rem] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
          <Settings2 className="size-3.5" /> {t("pages.run.portSection")}
          {svc.port != null ? <span className="font-mono text-[10px] font-normal normal-case text-[var(--t2,#62666d)]">{t("pages.run.currentPort", { port: svc.port })}</span> : null}
        </div>
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

      <Separator />

      <EnvVariablesEditor
        value={envDraft}
        onChange={setEnvDraft}
        onSave={saveEnv}
        saveDisabled={!ws.state.workspaceId}
      />
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
        <span className="inline-flex items-center gap-1 rounded-full bg-[var(--surface-2,#f3f4f5)] px-2 py-0.5 text-[11px] font-medium text-[var(--t2,#62666d)]">
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
            <span className="ml-auto flex items-center gap-2 text-[10px] font-normal normal-case">
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

function ServiceDetail({ id, compact }: { id: string; compact: boolean }) {
  const rt = useRuntime();
  const ws = useWorkspace();
  const runtime = useRuntime();
  const { t } = useTranslation();
  const [tab, setTab] = useState<"logs" | "env" | "health" | "config" | "metrics" | "container">("logs");
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

  type DetailTab = "logs" | "env" | "health" | "config" | "metrics" | "container";
  const tabs: { k: DetailTab; label: string; icon: typeof FileText }[] = [
    { k: "logs", label: t("nav.logs"), icon: FileText },
    { k: "env", label: t("pages.run.tabEnv"), icon: Settings2 },
    { k: "health", label: t("pages.run.tabHealth"), icon: Activity },
    { k: "config", label: t("nav.config"), icon: FileText },
    { k: "metrics", label: t("pages.run.tabMetrics"), icon: Activity },
    // 1.3：容器 Tab 仅 compose 服务显示（镜像/容器 ID/healthcheck/退出码，只读）
    ...(isCompose ? [{ k: "container" as DetailTab, label: t("nav.docker"), icon: Container }] : []),
  ];
  const locked = [
    // 版本以界面设计文档（真源）为准：终端 = 1.5 PTY；
    // 容器 Tab 1.3 已对 compose 服务上线，不再列入锁定项；
    // 代理 = 1.6 网关。指标 1.2 已上线为正式 Tab。
    { label: t("pages.run.lockTerminal"), v: "1.5" },
    { label: t("pages.run.lockProxy"), v: "1.6" },
  ];

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* head */}
      <div className={cn("flex items-center gap-2 border-b border-[var(--line,#e6e6e6)] px-4 py-3", compact && "px-3 py-2")}>
        <div className="flex min-w-0 flex-1 items-center gap-2 overflow-x-clip">
          <StatusDot state={svc.state} size={10} />
          <h1 className="min-w-0 truncate text-[1.08rem] font-bold tracking-tight text-[var(--t1,#222326)]">{id}</h1>
          <KindBadge kind={svc.kind} buildTool={spec?.build_tool} />
          {external ? (
            <span
              className="shrink-0 rounded-full bg-[var(--surface-2,#f3f4f5)] px-2 py-0.5 text-[11px] font-medium text-[var(--t2,#62666d)]"
              title={t("pages.run.externalMonitorTitle")}
            >
              {t("pages.run.externalMonitor")}
            </span>
          ) : null}
          <span
            className={cn(
              "shrink-0 rounded-full px-2 py-0.5 text-[11px] font-medium",
              isRunning ? "bg-[var(--ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]" : "bg-[var(--surface-2,#f3f4f5)] text-[var(--t2,#62666d)]",
            )}
          >
            {stateLabel(svc.state)}
            {isRunning && svc.started_at_ms ? ` · ${fmtDuration(svc.started_at_ms)}` : ""}
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <IdeOpenMenu variant="button" />
          {jarService || composeService ? (
            <Button size="sm" variant="secondary" className="gap-1" onClick={() => void buildJar()} disabled={building || isBusy}>
              {building ? <Loader2 className="size-3.5 animate-spin" /> : <Hammer className="size-3.5" />}
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
              <RotateCw className="size-3.5" /> {t("common.restart")}
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
              <Square className="size-3.5" /> {t("common.stop")}
            </Button>
          ) : (
            <Button size="sm" variant="default" className="gap-1" disabled={isBusy} onClick={() => runtime.actions.startOne(id)}>
              <Play className="size-3.5" /> {svc.state === "exited" || svc.last_error ? t("pages.run.retryStart") : t("common.start")}
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
      <div className="mx-4 mt-3 flex items-center gap-2.5 rounded-[var(--r-md,12px)] border border-[#2B2E36] bg-[#191B20] py-2 pl-3.5 pr-1.5 font-mono shadow-[0_1px_2px_rgb(16_24_40_/_0.08)]">
        <span className="shrink-0 font-bold text-[#7B84EA]" aria-hidden>
          $
        </span>
        <span className="min-w-0 flex-1 truncate text-[0.76rem] leading-5 text-[#E7E9EC]" title={cmd}>
          {cmd}
        </span>
        <button
          className={cn(
            "flex shrink-0 items-center gap-1 rounded-[var(--r-sm,8px)] px-1.5 py-1 text-[0.68rem] transition-all duration-150",
            copied
              ? "text-[#4ADE80]"
              : "text-[#9AA0AB] hover:bg-white/10 hover:text-[#E7E9EC]",
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
          {copied ? <span>{t("common.copied")}</span> : null}
        </button>
      </div>

      {/* meta strip */}
      <div className="mx-4 mt-2 flex flex-wrap items-center gap-x-[1.1rem] gap-y-1.5 rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] px-3.5 py-2">
        <Meta k={t("pages.run.metaPort")} v={svc.port != null ? `${svc.port}` : "—"} accent />
        <Meta k={t("pages.run.metaStack")} v={stack} />
        <Meta k={t("pages.run.metaTopo")} v={topo} muted />
        <Meta
          k="PID"
          v={
            svc.pid != null
              ? `${svc.pid}${isRunning ? " · Job Object" : ""}`
              : isCompose
                ? t("pages.run.containerManaged")
                : "—"
          }
        />
        <Meta k={t("pages.run.metaLog")} v={`service:${id}`} mono />
        <Meta k={t("pages.run.metaUptime")} v={svc.started_at_ms ? fmtDuration(svc.started_at_ms) : "—"} ok={isRunning} />
      </div>

      {/* segbar */}
      <div className="flex items-center gap-2 overflow-x-auto border-b border-[var(--line,#e6e6e6)] px-3 py-2">
        <div className="inline-flex items-center gap-0.5 rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f3f4f5)] p-0.5">
          {tabs.map((t) => (
            <button
              key={t.k}
              onClick={() => setTab(t.k)}
              className={cn(
                "flex items-center gap-1 rounded-[6px] px-3 py-1 text-[0.75rem] font-semibold transition-all duration-150",
                tab === t.k
                  ? "bg-[var(--surface,#fff)] text-[var(--t1,#222326)] shadow-[var(--shadow-1,0_1px_2px_rgb(16_24_40_/_0.05)),inset_0_0_0_1px_var(--line,#e6e6e6)]"
                  : "text-[var(--t2,#62666d)] hover:bg-[rgb(0_0_0_/_0.045)] hover:text-[var(--t1,#222326)]",
              )}
            >
              <t.icon className="size-3.5" /> {t.label}
            </button>
          ))}
        </div>
        {locked.map((l) => (
          <span key={l.label} className="inline-flex shrink-0 items-center gap-1 rounded-lg px-2 py-1 text-[0.73rem] text-[var(--t3,#8a8f98)]">
            <span className="grid size-3.5 place-items-center rounded bg-black/5"><Lock className="size-3" /></span>
            {l.label}
            <Badge variant="secondary" className="text-[9px]">{l.v}</Badge>
          </span>
        ))}
      </div>

      {/* panels：日志等宽区域内部滚动，不外层滚动 */}
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        {tab === "logs" ? <LogView source={source} className="min-h-0 flex-1" height="100%" /> : null}
        {tab === "env" ? (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <EnvPanel id={id} compact={compact} />
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
      </div>
    </div>
  );
}

function MetricsPanel({ id, metric, compose = false }: { id: string; metric: ServiceMetrics | null; compose?: boolean }) {
  const { t } = useTranslation();
  const fmtBytes = (n: number | null | undefined) => {
    if (n == null) return "—";
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
    if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MiB`;
    return `${(n / 1024 / 1024 / 1024).toFixed(2)} GiB`;
  };
  const emptyHint = compose
    ? t("pages.run.metricsComposeHint")
    : t("pages.run.metricsEmptyHint", { id });
  return (
    <div className="grid grid-cols-1 gap-3 p-4 sm:grid-cols-3">
      <MetricTile icon={<Cpu className="size-4" />} label="CPU" value={metric?.cpu_percent == null ? "—" : `${metric.cpu_percent.toFixed(1)}%`} />
      <MetricTile icon={<HardDrive className="size-4" />} label={t("pages.run.metaMemory")} value={fmtBytes(metric?.memory_bytes)} />
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
                  "inline-flex items-center rounded-full px-2 py-0.5 font-mono text-[11px] font-semibold",
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
}: {
  k: string;
  v: string;
  accent?: boolean;
  muted?: boolean;
  mono?: boolean;
  ok?: boolean;
}) {
  return (
    <span className="relative inline-flex items-center gap-[0.45rem] pl-[1.1rem] first:pl-0 [&:not(:first-child)]:before:absolute [&:not(:first-child)]:before:left-0 [&:not(:first-child)]:before:top-1/2 [&:not(:first-child)]:before:h-[0.9em] [&:not(:first-child)]:before:w-px [&:not(:first-child)]:before:-translate-y-1/2 [&:not(:first-child)]:before:bg-[var(--line-strong,#d0d6e0)] [&:not(:first-child)]:before:content-['']">
      <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">{k}</span>
      <span
        className={cn(
          "text-[0.78rem] font-medium",
          mono || accent || ok ? "font-mono" : "",
          accent && "font-mono font-bold text-[var(--st-accent,#5e6ad2)]",
          ok && "font-mono font-bold text-[var(--st-ok-deep,#1e7e35)]",
          muted && !accent && !ok && "text-[var(--t2,#62666d)]",
          !accent && !ok && !muted && "font-mono text-[var(--t1,#222326)]",
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

  const foot = isRunning ? (
    <span className="flex items-center gap-1 text-[11px] text-[var(--st-ok-deep,#1e7e35)]">
      {t("states.script_running")}{view?.pid ? <span className="font-mono"> · pid {view.pid}</span> : null}
    </span>
  ) : view?.last_error ? (
    <span className="block truncate text-[11px] text-[var(--st-danger,#dc2626)]" title={view.last_error}>⚠ {view.last_error}</span>
  ) : view?.last_exit ? (
    <span className={cn("text-[11px]", view.last_exit.code === 0 ? "text-[var(--st-ok-deep,#1e7e35)]" : "text-[var(--st-danger,#dc2626)]")}>
      {scriptStateLabel(view)}
    </span>
  ) : (
    <span className="text-[11px] text-[var(--t3,#8a8f98)]">{t("pages.run.scriptTask", { n: spec.cmds.length })}</span>
  );

  return (
    <div
      onClick={onOpen}
      className={cn(
        "group/scr relative flex h-[5.7rem] cursor-pointer flex-col overflow-hidden rounded-xl border bg-[var(--surface,#fff)] p-2.5 transition-all",
        selected ? "border-[var(--st-accent,#5e6ad2)] bg-[color-mix(in_oklch,var(--st-accent,#5e6ad2)_6%,white)] ring-1 ring-[var(--st-accent,#5e6ad2)]/30" : "border-[var(--line,#e6e6e6)] hover:border-[var(--line-strong,#d0d6e0)]",
      )}
    >
      <div className="flex items-center gap-2">
        <StatusDot state={scriptDotState(view ?? { state: "idle", last_exit: null, last_error: null })} size={8} />
        <span className="truncate text-[0.88rem] font-semibold text-[var(--t1,#222326)]">{id}</span>
        <KindBadge kind="task" />
        <div className="ml-auto flex gap-1 opacity-50 transition-opacity group-hover/scr:opacity-100" onClick={(e) => e.stopPropagation()}>
          {isRunning ? (
            <button
              type="button"
              className="grid size-[1.8rem] cursor-pointer place-items-center rounded-[var(--r-sm,8px)] border border-transparent text-[var(--t3,#8a8f98)] transition-colors duration-150 hover:border-[#FECACA] hover:bg-[var(--st-danger-tint,#fdecec)] hover:text-[var(--st-danger,#dc2626)] disabled:cursor-not-allowed disabled:opacity-50"
              title={t("pages.run.stopScriptTitle")}
              onClick={() => setConfirmStop(true)}
            >
              <Square className="size-3.5" />
            </button>
          ) : (
            <button
              type="button"
              className="grid size-[1.8rem] cursor-pointer place-items-center rounded-[var(--r-sm,8px)] border border-transparent text-[var(--t3,#8a8f98)] transition-colors duration-150 hover:border-[var(--line-strong,#d0d6e0)] hover:bg-[var(--surface-2,#f3f4f5)] hover:text-[var(--st-accent,#5e6ad2)] disabled:cursor-not-allowed disabled:opacity-50"
              title={anyRunning ? t("pages.run.scriptBusy") : t("pages.run.runScriptTitle")}
              disabled={anyRunning}
              onClick={() => void run()}
            >
              <Play className="size-3.5" />
            </button>
          )}
        </div>
      </div>
      <div className="mt-1.5 truncate text-[11px] text-[var(--t2,#62666d)]">{spec.desc ?? spec.cmds.join(" ; ")}</div>
      <div className="mt-auto min-h-[1.05rem] overflow-hidden">{foot}</div>

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
  const chipCls = isRunning
    ? "bg-[var(--ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]"
    : view?.last_error || (view?.last_exit?.code ?? 0) > 0
      ? "bg-[var(--st-danger-tint,#fdecec)] text-[var(--st-danger,#dc2626)]"
      : "bg-[var(--surface-2,#f3f4f5)] text-[var(--t2,#62666d)]";

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
      <div className="flex items-center gap-2 border-b border-[var(--line,#e6e6e6)] px-4 py-3">
        <div className="flex min-w-0 flex-1 items-center gap-2 overflow-x-clip">
          <StatusDot state={scriptDotState(view ?? { state: "idle", last_exit: null, last_error: null })} size={10} />
          <h1 className="min-w-0 truncate text-[1.08rem] font-bold tracking-tight text-[var(--t1,#222326)]">{id}</h1>
          <KindBadge kind="task" />
          <span className={cn("shrink-0 rounded-full px-2 py-0.5 text-[11px] font-medium", chipCls)}>
            {scriptStateLabel(view ?? { state: "idle", last_exit: null, last_error: null })}
          </span>
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
      <div className="mx-4 mt-3 rounded-[var(--r-md,12px)] border border-[#2B2E36] bg-[#191B20] py-2 pl-3.5 pr-1.5 font-mono shadow-[0_1px_2px_rgb(16_24_40_/_0.08)]">
        <div className="flex items-start gap-2.5">
          <div className="min-w-0 flex-1">
            {spec.cmds.map((c, i) => (
              <div key={i} className="flex min-w-0 items-baseline gap-2 py-0.5">
                <span className="shrink-0 font-bold text-[#7B84EA]" aria-hidden>
                  $
                </span>
                <span className="min-w-0 flex-1 break-all text-[0.76rem] leading-5 text-[#E7E9EC]" title={c}>
                  {c}
                </span>
              </div>
            ))}
          </div>
          <button
            className={cn(
              "flex shrink-0 items-center gap-1 rounded-[var(--r-sm,8px)] px-1.5 py-1 text-[0.68rem] transition-all duration-150",
              copied ? "text-[#4ADE80]" : "text-[#9AA0AB] hover:bg-white/10 hover:text-[#E7E9EC]",
            )}
            title={copied ? t("common.copied") : t("pages.run.copyCmd")}
            onClick={copyCmds}
          >
            {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
            {copied ? <span>{t("common.copied")}</span> : null}
          </button>
        </div>
      </div>

      {/* meta strip */}
      <div className="mx-4 mt-2 flex flex-wrap items-center gap-x-[1.1rem] gap-y-1.5 rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] px-3.5 py-2">
        <Meta k={t("pages.run.metaCmds")} v={t("pages.run.cmdCount", { n: spec.cmds.length })} />
        <Meta k={t("pages.run.metaCwd")} v={spec.cwd ?? t("pages.run.wsRoot")} muted />
        <Meta k={t("pages.run.metaTimeout")} v={`${spec.timeout_secs ?? 1800}s`} />
        <Meta k="PID" v={view?.pid != null ? `${view.pid} · Job Object` : "—"} />
        <Meta k={t("pages.run.metaLog")} v={`script:${id}`} mono />
        <Meta
          k={t("pages.run.metaLastExit")}
          v={view?.last_exit ? t("pages.run.exitCode", { code: view.last_exit.code }) : "—"}
          ok={view?.last_exit?.code === 0}
        />
      </div>

      {/* logs：与日志页共用 LogView，来源 script:{id} */}
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden px-4 pb-4 pt-3">
        <LogView source={source} className="min-h-0 flex-1" height="100%" />
      </div>
    </div>
  );
}

/* ---------------- RunPage ---------------- */

export function RunPage() {
  const rt = useRuntime();
  const ws = useWorkspace();
  const { t } = useTranslation();
  const { compact } = useOutletContext<ShellCtx>();
  const [selected, setSelected] = useState<{ kind: "service" | "script"; id: string } | null>(null);

  const serviceIds = Object.keys(rt.state.services);
  const scriptIds = ws.state.spec ? Object.keys(ws.state.spec.scripts) : [];
  const running = serviceIds.filter((i) => rt.state.services[i].state === "running").length;
  const totalItems = serviceIds.length + scriptIds.length;
  const [cardsCollapsed, setCardsCollapsed] = useState(() => readCardsCollapsedPref() ?? totalItems === 1);

  useEffect(() => {
    if (readCardsCollapsedPref() === null && totalItems === 1) setCardsCollapsed(true);
  }, [totalItems]);

  const toggleCards = () => {
    setCardsCollapsed((v) => {
      const next = !v;
      writeCardsCollapsedPref(next);
      return next;
    });
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
    <div className="flex min-h-0 flex-1 overflow-hidden">
      {/* card column：仅卡片列表自身滚动 */}
      <section
        className={cn(
          "flex shrink-0 flex-col border-r border-[var(--line,#e6e6e6)] transition-[width] duration-200 ease-[var(--st-ease,cubic-bezier(.22,1,.36,1))]",
          cardsCollapsed ? "w-12" : "w-[21.5rem]",
          compact && !cardsCollapsed && "p-2",
          !cardsCollapsed && "p-3",
        )}
        style={
          cardsCollapsed
            ? undefined
            : { background: "linear-gradient(180deg, color-mix(in_oklch, var(--st-accent,#5e6ad2) 2.5%, transparent) 0%, transparent 22%)" }
        }
      >
        {cardsCollapsed ? (
          <div className="flex h-full flex-col items-center gap-2 py-2">
            <button
              type="button"
              onClick={toggleCards}
              className="grid size-8 cursor-pointer place-items-center rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] text-[var(--t2,#62666d)] transition-colors hover:border-[var(--t3,#8a8f98)] hover:bg-[var(--surface-2,#f3f4f5)] hover:text-[var(--t1,#222326)]"
              title={t("pages.run.expandList")}
            >
              <PanelLeftOpen className="size-4" />
            </button>
            {serviceIds.length === 1 ? (
              <button
                type="button"
                onClick={() => setSelected({ kind: "service", id: serviceIds[0] })}
                className="flex flex-col items-center gap-1 px-1 py-2"
                title={serviceIds[0]}
              >
                <StatusDot state={rt.state.services[serviceIds[0]].state} size={8} />
                <span className="font-mono text-[0.58rem] text-[var(--t3,#8a8f98)] [writing-mode:vertical-rl]">
                  {serviceIds[0]}
                </span>
              </button>
            ) : null}
          </div>
        ) : (
          <>
            <div className="flex items-center gap-2 px-1 pb-2 pt-1">
              <span className="text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">{t("pages.run.servicesHeader")}</span>
              <span className="font-mono text-[11px] text-[var(--t3,#8a8f98)]">{t("pages.run.serviceCountRunning", { total: serviceIds.length, running })}</span>
              <button
                type="button"
                onClick={toggleCards}
                className="ml-auto grid size-7 cursor-pointer place-items-center rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] text-[var(--t2,#62666d)] transition-colors hover:border-[var(--t3,#8a8f98)] hover:bg-[var(--surface-2,#f3f4f5)] hover:text-[var(--t1,#222326)]"
                title={totalItems === 1 ? t("pages.run.collapseListSingle") : t("pages.run.collapseList")}
              >
                <PanelLeftClose className="size-3.5" />
              </button>
            </div>
            <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto">
              {serviceIds.length === 0 ? (
                <div className="rounded-lg border border-dashed border-[var(--line,#e6e6e6)] p-6 text-center text-sm text-[var(--t3,#8a8f98)]">
                  {t("pages.run.noServices")}
                </div>
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
                  <div className="flex items-center gap-2 px-1 pb-2 pt-4">
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
            </div>
          </>
        )}
      </section>

      {/* detail：外层不滚动，滚动交给日志框体 / 各 Tab 面板 */}
      <section
        className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden px-4 pb-4 pt-2"
        style={{ background: "radial-gradient(70% 45% at 88% 0%, rgb(94 106 210 / 0.035), transparent 60%)" }}
      >
        {!sel ? (
          <div className="flex h-full items-center justify-center text-sm text-[var(--t3,#8a8f98)]">{t("pages.run.selectDetail")}</div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[var(--r-lg,16px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] shadow-[var(--shadow-1,0_1px_2px_rgb(16_24_40_/_0.05))]">
            {sel.kind === "service" ? <ServiceDetail id={sel.id} compact={compact} /> : <ScriptDetail id={sel.id} />}
          </div>
        )}
      </section>

      {/* TEMP: 临时错误弹框，验证后整体移除 */}
      {rt.state.error ? (
        <div
          className="fixed inset-0 z-[200] grid place-items-center bg-black/40"
          onClick={() => rt.actions.clearError()}
        >
          <div
            className="w-[420px] rounded-xl border border-red-200 bg-white p-5 shadow-xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="mb-2 text-sm font-semibold text-red-600">{t("pages.run.startFailed")}</div>
            <p className="mb-4 whitespace-pre-wrap break-words text-sm text-[var(--t1,#222326)]">
              {rt.state.error}
            </p>
            <div className="flex justify-end">
              <Button onClick={() => rt.actions.clearError()}>{t("pages.run.ack")}</Button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
