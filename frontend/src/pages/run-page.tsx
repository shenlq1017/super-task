import { useEffect, useRef, useState, useCallback, type ReactNode } from "react";
import { useOutletContext, NavLink } from "react-router-dom";
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

function KindBadge({ kind }: { kind: string }) {
  const color =
    kind === "node"
      ? "#2E90FA"
      : kind === "compose"
        ? "#12B76A"
        : kind === "task"
          ? "var(--t3,#8a8f98)"
          : "var(--st-accent,#5e6ad2)";
  return (
    <span className="inline-flex items-center gap-1 rounded-full bg-[var(--surface-2,#f3f4f5)] px-1.5 py-0.5 font-mono text-[10px] font-semibold uppercase text-[var(--t2,#62666d)]">
      <span className="size-1.5 rounded-full" style={{ background: color }} />
      {kindLabel(kind)}
    </span>
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

const IDE_TARGETS: { id: IdeTarget; label: string }[] = [
  { id: "explorer", label: "资源管理器" },
  { id: "cursor", label: "Cursor" },
  { id: "idea", label: "IntelliJ IDEA" },
  { id: "code", label: "VS Code" },
];

/**
 * 「打开」入口：小按钮 + 下拉菜单（固定四目标枚举）。
 * 不依赖服务运行状态，随时可打开；fixed 定位避免被卡片列滚动容器裁剪。
 */
function IdeOpenMenu({ variant }: { variant: "icon" | "button" }) {
  const ws = useWorkspace();
  const { toast } = useToast();
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
      toast(`已用 ${label} 打开：${out.path}`, "ok");
      setOpen(false);
      triggerRef.current?.focus();
    } catch (e) {
      if (e instanceof IpcFailure && e.code === "IDE_NOT_FOUND") {
        toast(`未检测到 ${label}，可手动安装或改用其他方式`, "warn");
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
        aria-label="打开工作区（资源管理器或 IDE）"
        aria-haspopup="menu"
        aria-expanded={open}
        title="打开工作区"
        className={cn(buttonVariants({ variant: "outline", size: "sm" }), variant === "icon" && "size-7 px-0")}
      >
        <ExternalLink className="size-3.5" />
        {variant === "button" ? "打开" : null}
      </button>

      {open ? (
        <>
          <div className="fixed inset-0 z-[205]" onClick={() => setOpen(false)} aria-hidden />
          <div
            role="menu"
            aria-label="选择打开方式"
            className="fixed z-[210] min-w-[10.5rem] overflow-hidden rounded-[var(--r-md,12px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] p-1 shadow-[var(--shadow-2,0_6px_20px_rgb(16_24_40_/_0.09))]"
            style={{ top: pos?.top ?? 0, right: pos?.right ?? 0 }}
          >
            {IDE_TARGETS.map((t, i) => (
              <button
                key={t.id}
                role="menuitem"
                // eslint-disable-next-line jsx-a11y/no-autofocus -- 菜单打开即聚焦首项，键盘可达
                autoFocus={i === 0}
                disabled={busy !== null}
                onClick={() => void openWith(t.id, t.label)}
                className="flex w-full items-center rounded-[var(--r-sm,8px)] px-2.5 py-1.5 text-left text-[0.8rem] text-[var(--t1,#222326)] transition-colors duration-150 hover:bg-[var(--st-accent-tint,#eef0fb)] focus-visible:bg-[var(--st-accent-tint,#eef0fb)] focus-visible:outline-none disabled:opacity-50"
              >
                {t.label}
                {busy === t.id ? <span className="ml-auto text-[0.68rem] text-[var(--t3,#8a8f98)]">打开中…</span> : null}
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
          {spec?.depends_on?.length ? `依赖 ${spec.depends_on.join(", ")}` : "无依赖"}
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
        <KindBadge kind={svc.kind} />
        {external ? (
          <span className="shrink-0 rounded-full bg-[var(--surface-2,#f3f4f5)] px-1.5 py-0.5 font-mono text-[10px] font-semibold uppercase text-[var(--t2,#62666d)]" title="外部进程：非 SuperTask 启动，仅监控，停止将 taskkill 整棵树">
            外部
          </span>
        ) : null}
        <div className="ml-auto flex gap-1 opacity-50 transition-opacity group-hover:opacity-100" onClick={(e) => e.stopPropagation()}>
          <IdeOpenMenu variant="icon" />
          {isRunning ? (
            <button
              type="button"
              className="grid size-[1.8rem] cursor-pointer place-items-center rounded-[var(--r-sm,8px)] border border-[var(--st-warn-line,#f0dcb0)] bg-[var(--st-warn-tint,#fff8e1)] text-[var(--st-warn,#9a6700)] transition-colors duration-150 hover:border-[#E0C080] hover:bg-[rgb(234_179_8_/_0.2)] disabled:cursor-not-allowed disabled:opacity-50"
              title={external ? "停止外部进程并重新由 SuperTask 启动" : "重启"}
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
            title={isRunning ? "停止" : "启动"}
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
        title={`停止「${id}」？`}
        description={
          external
            ? "该服务为外部进程（非 SuperTask 启动），将按端口定位 PID 并 taskkill 结束整棵进程树。"
            : "将结束该服务的整棵进程树（含其派生的子进程）。"
        }
        confirmText="停止服务"
        cancelText="取消"
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
            title="容器托管：由 docker compose 管理，无宿主 pid"
          >
            容器托管
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
      setPortCheck({ port: n, inUse: false, message: "请输入 1024–65535 的整数端口后再检查" });
      return;
    }
    setPortBusy(true);
    try {
      const out = await apiPortsInspect(ws.state.workspaceId, id, n);
      const item = out.items[0];
      if (!item) {
        setPortCheck({ port: n, inUse: false, message: "无法判断该端口的占用情况" });
      } else if (item.in_use) {
        setPortCheck({
          port: item.port,
          inUse: true,
          message: `${item.port} 已被 ${item.process_name ?? `PID ${item.pid ?? "未知"}`} 占用${item.managed ? "（SuperTask 托管）" : "（外部进程）"}`,
        });
      } else {
        setPortCheck({
          port: item.port,
          inUse: false,
          message: `${item.port} 可用${isRunning && svc.port === item.port ? "（当前服务）" : ""}`,
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
          : { port: portNumber().n, inUse: false, message: "没有可用候选端口" },
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
      toast("端口必须是 1024–65535 的整数", "warn");
      return;
    }
    setPortBusy(true);
    try {
      const out = await apiPortsAssign(ws.state.workspaceId, id, n, yaml.state.hash, restart);
      if (out.restart_required) {
        setPortCheck({ port: n, inUse: false, message: "服务正在运行；确认后点击「改端口并重启」" });
      } else {
        await Promise.all([yaml.actions.reload(), ws.actions.refreshSpec()]);
        setPortCheck({ port: n, inUse: false, message: out.notes.length ? out.notes.join("；") : "端口已保存" });
        toast("端口已保存", "ok");
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
      toast("已保存环境变量", "ok");
    } else {
      toast(yaml.state.error ?? "保存失败", "err");
    }
  };

  return (
    <div className={cn("flex flex-col gap-5 p-4", compact && "gap-4 p-3")}>
      <section className="flex flex-col gap-2">
        <div className="flex items-center gap-2 text-[0.72rem] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
          <Settings2 className="size-3.5" /> 端口
          {svc.port != null ? <span className="font-mono text-[10px] font-normal normal-case text-[var(--t2,#62666d)]">当前 {svc.port}</span> : null}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Input
            type="number"
            value={portDraft}
            onChange={(e) => editPortDraft(e.target.value)}
            className="h-8 max-w-[7.5rem] font-mono text-sm"
            placeholder="端口"
            aria-label="服务端口"
          />
          <Button variant="soft" size="sm" onClick={() => void inspectPorts()} disabled={!ws.state.workspaceId || portBusy || !portValid}>
            检查
          </Button>
          <Button variant="outline" size="sm" onClick={() => void suggestPorts()} disabled={!ws.state.workspaceId || portBusy}>
            建议
          </Button>
          <Button size="sm" variant="success" onClick={() => void assignPort(false)} disabled={!ws.state.workspaceId || portBusy || !portValid}>
            保存
          </Button>
          {isRunning ? (
            <Button size="sm" variant="warn" onClick={() => void assignPort(true)} disabled={portBusy || !portValid}>
              改端口并重启
            </Button>
          ) : null}
        </div>
        {portCandidates.length > 0 ? (
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">建议端口</span>
            {portCandidates.map((p) => (
              <button
                key={p}
                type="button"
                title={`填入 ${p}`}
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

/** 长 URL 拆成「路径优先 + host 次要」，完整串放 title / 复制。 */
function healthTarget(h: ServiceSpec["health"] | undefined, port: number | null | undefined) {
  if (!h || h.type === "none") {
    return { kind: "none" as const, path: "未配置", host: "", full: "", hint: "在配置页为服务添加 health" };
  }
  if (h.type === "tcp") {
    const p = port ?? null;
    return {
      kind: "tcp" as const,
      path: p != null ? `:${p}` : "未设端口",
      host: "TCP 探测",
      full: p != null ? `tcp://127.0.0.1:${p}` : "",
      hint: "端口可连接即为成功",
    };
  }
  const raw = (h.http ?? "").trim();
  if (!raw) {
    return { kind: "http" as const, path: "—", host: "", full: "", hint: "http 2xx 为成功" };
  }
  try {
    const u = new URL(raw);
    const path = `${u.pathname || "/"}${u.search}`;
    return { kind: "http" as const, path, host: u.host, full: raw, hint: "http 2xx 为成功" };
  } catch {
    return { kind: "http" as const, path: raw, host: port != null ? `port ${port}` : "", full: raw, hint: "http 2xx 为成功" };
  }
}

function HealthPanel({ svc, spec }: { svc: ServiceRuntimeView; spec: ServiceSpec | undefined }) {
  const h = spec?.health;
  const last = svc.health;
  const target = healthTarget(h, svc.port);
  const { toast } = useToast();
  const watching = svc.state === "running" || svc.state === "unhealthy";
  const statusLabel = !h || h.type === "none" ? "未配置" : last ? (last.ok ? "健康" : "异常") : watching ? "探测中" : "已暂停";
  const failReason = svc.last_error ? svc.last_error : watching ? null : "服务未运行，健康检查已暂停";

  return (
    <div className="flex flex-col gap-4 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <span
          className={cn(
            "rounded-[var(--r-sm,8px)] px-2.5 py-1 text-[0.82rem] font-semibold",
            statusLabel === "健康" && "bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]",
            statusLabel === "异常" && "bg-[var(--st-danger-tint,#fdecec)] text-[var(--st-danger,#dc2626)]",
            (statusLabel === "未配置" || statusLabel === "已暂停" || statusLabel === "探测中") &&
              "bg-[var(--surface-2,#f3f4f5)] text-[var(--t2,#62666d)]",
          )}
        >
          {statusLabel}
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
          探测目标
          <span className="ml-auto font-normal normal-case tracking-normal">{target.hint}</span>
        </div>
        <div className="flex min-w-0 items-start gap-2">
          <div className="min-w-0 flex-1">
            <div className="truncate font-mono text-[0.95rem] font-semibold text-[var(--t1,#222326)]" title={target.full || target.path}>
              {target.path}
            </div>
            {target.host ? (
              <div className="mt-0.5 truncate font-mono text-[0.72rem] text-[var(--t2,#62666d)]" title={target.full}>
                {target.host}
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
                toast("已复制探测地址", "ok");
              }}
            >
              <Copy className="size-3.5" /> 复制
            </Button>
          ) : null}
        </div>
      </div>

      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] p-3">
          <div className="mb-2 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
            最近结果
            <span className="ml-auto flex items-center gap-2 text-[10px] font-normal normal-case">
              <span><span className="mr-1 inline-block size-2 rounded-sm bg-[var(--st-ok,#27a644)]" />成功</span>
              <span><span className="mr-1 inline-block size-2 rounded-sm bg-[var(--st-warn,#9a6700)]" />慢</span>
              <span><span className="mr-1 inline-block size-2 rounded-sm bg-[var(--st-danger,#dc2626)]" />失败</span>
            </span>
          </div>
          <HealthSparkline ok={last?.ok} />
          <div className="mt-2 flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-1 text-[13px]">
            <span className={cn("font-medium", healthClass(last?.ok))}>
              {last ? (last.ok ? "健康" : "异常") : "尚无结果"}
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
          <div className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">失败原因</div>
          <p
            className={cn(
              "text-[0.8rem] leading-relaxed",
              failReason && svc.last_error ? "text-[var(--st-danger,#dc2626)]" : "text-[var(--t2,#62666d)]",
            )}
          >
            {failReason ?? "—（无）"}
          </p>
        </div>
      </div>
    </div>
  );
}

/* ---------------- config panel ---------------- */

function ConfigPanel({ id }: { id: string }) {
  const ws = useWorkspace();
  const spec = ws.state.spec?.services[id];
  if (!spec) return null;
  const text = serviceYamlFragment(id, spec);
  return (
    <div className="flex flex-col gap-3 p-4">
      <div className="flex items-center justify-between">
        <div className="text-sm font-medium">原文 YAML（片段）</div>
        <Button variant="outline" size="sm" asChild className="gap-1">
          <NavLink to="/config">在配置页编辑</NavLink>
        </Button>
      </div>
      <pre className="overflow-auto rounded-lg border border-[var(--line,#e6e6e6)] bg-[#FBFBFC] p-3 font-mono text-[12px] leading-relaxed text-[var(--t2,#62666d)]">
        {text}
      </pre>
      <p className="text-[11px] text-[var(--t3,#8a8f98)]">
        端口重复 → 警告不阻断；depends_on 成环 → 在配置页拒绝保存并指出环。
      </p>
    </div>
  );
}

/* ---------------- detail (service) ---------------- */

function ServiceDetail({ id, compact }: { id: string; compact: boolean }) {
  const rt = useRuntime();
  const ws = useWorkspace();
  const runtime = useRuntime();
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
      toast(`已开始构建 ${id}`, "info");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setBuilding(false);
    }
  };

  const stack = isCompose
    ? `Compose · ${spec?.service ?? id}`
    : svc.kind === "node"
      ? `Node · ${spec?.package_manager ?? "npm"}`
      : spec?.module && spec.module !== "."
        ? `Spring Boot · ${spec.module}`
        : "Spring Boot";
  const topo = spec?.depends_on?.length ? `依赖 ${spec.depends_on.join(", ")}` : "无依赖";

  type DetailTab = "logs" | "env" | "health" | "config" | "metrics" | "container";
  const tabs: { k: DetailTab; label: string; icon: typeof FileText }[] = [
    { k: "logs", label: "日志", icon: FileText },
    { k: "env", label: "环境", icon: Settings2 },
    { k: "health", label: "健康", icon: Activity },
    { k: "config", label: "配置", icon: FileText },
    { k: "metrics", label: "指标", icon: Activity },
    // 1.3：容器 Tab 仅 compose 服务显示（镜像/容器 ID/healthcheck/退出码，只读）
    ...(isCompose ? [{ k: "container" as DetailTab, label: "容器", icon: Container }] : []),
  ];
  const locked = [
    // 版本以界面设计文档（真源）为准：终端 = 1.5 PTY；
    // 容器 Tab 1.3 已对 compose 服务上线，不再列入锁定项；
    // 代理 = 1.6 网关。指标 1.2 已上线为正式 Tab。
    { label: "终端", v: "1.5" },
    { label: "代理", v: "1.6" },
  ];

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* head */}
      <div className={cn("flex items-center gap-2 border-b border-[var(--line,#e6e6e6)] px-4 py-3", compact && "px-3 py-2")}>
        <div className="flex min-w-0 flex-1 items-center gap-2 overflow-x-clip">
          <StatusDot state={svc.state} size={10} />
          <h1 className="min-w-0 truncate text-[1.08rem] font-bold tracking-tight text-[var(--t1,#222326)]">{id}</h1>
          <KindBadge kind={svc.kind} />
          {external ? (
            <span
              className="shrink-0 rounded-full bg-[var(--surface-2,#f3f4f5)] px-2 py-0.5 text-[11px] font-medium text-[var(--t2,#62666d)]"
              title="外部进程：非 SuperTask 启动，仅监控"
            >
              外部 · 仅监控
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
              {building ? "构建中…" : jarService ? "构建 jar" : "构建镜像"}
            </Button>
          ) : null}
          {isRunning ? (
            <Button
              size="sm"
              variant="warn"
              className="gap-1"
              disabled={isBusy}
              title={external ? "停止外部进程并重新由 SuperTask 启动" : undefined}
              onClick={() => runtime.actions.restartOne(id)}
            >
              <RotateCw className="size-3.5" /> 重启
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
              <Square className="size-3.5" /> 停止
            </Button>
          ) : (
            <Button size="sm" variant="default" className="gap-1" disabled={isBusy} onClick={() => runtime.actions.startOne(id)}>
              <Play className="size-3.5" /> {svc.state === "exited" || svc.last_error ? "重试启动" : "启动"}
            </Button>
          )}
        </div>
      </div>

      <ConfirmDialog
        open={confirmStop}
        title={`停止「${id}」？`}
        description={
          external
            ? "该服务为外部进程（非 SuperTask 启动），将按端口定位 PID 并 taskkill 结束整棵进程树。"
            : "将结束该服务的整棵进程树（含其派生的子进程）。"
        }
        confirmText="停止服务"
        cancelText="取消"
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
          title={copied ? "已复制" : "复制命令"}
          onClick={() => {
            void copyText(cmd).then((ok) => {
              if (!ok) {
                toast("复制失败，请手动选择复制", "err");
                return;
              }
              setCopied(true);
              window.setTimeout(() => setCopied(false), 2500);
            });
          }}
        >
          {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
          {copied ? <span>已复制</span> : null}
        </button>
      </div>

      {/* meta strip */}
      <div className="mx-4 mt-2 flex flex-wrap items-center gap-x-[1.1rem] gap-y-1.5 rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] px-3.5 py-2">
        <Meta k="端口" v={svc.port != null ? `${svc.port}` : "—"} accent />
        <Meta k="运行栈" v={stack} />
        <Meta k="拓扑" v={topo} muted />
        <Meta
          k="PID"
          v={
            svc.pid != null
              ? `${svc.pid}${isRunning ? " · Job Object" : ""}`
              : isCompose
                ? "容器托管"
                : "—"
          }
        />
        <Meta k="日志" v={`service:${id}`} mono />
        <Meta k="已运行" v={svc.started_at_ms ? fmtDuration(svc.started_at_ms) : "—"} ok={isRunning} />
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
  const fmtBytes = (n: number | null | undefined) => {
    if (n == null) return "—";
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
    if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MiB`;
    return `${(n / 1024 / 1024 / 1024).toFixed(2)} GiB`;
  };
  const emptyHint = compose
    ? "容器托管：容器由 Docker Compose 管理，1.3 不采集容器内指标（docker stats 不在范围），CPU / 内存以「—」展示"
    : `${id} 当前没有可用的 Job Object 指标（外部进程不采样）`;
  return (
    <div className="grid grid-cols-1 gap-3 p-4 sm:grid-cols-3">
      <MetricTile icon={<Cpu className="size-4" />} label="CPU" value={metric?.cpu_percent == null ? "—" : `${metric.cpu_percent.toFixed(1)}%`} />
      <MetricTile icon={<HardDrive className="size-4" />} label="内存" value={fmtBytes(metric?.memory_bytes)} />
      <MetricTile icon={<Boxes className="size-4" />} label="进程树" value={metric?.process_count == null ? "—" : `${metric.process_count} 个进程`} />
      <div className="sm:col-span-3 text-[0.72rem] text-[var(--t3,#8a8f98)]">
        {metric ? `最近采样：${new Date(metric.sampled_at_ms).toLocaleTimeString()}` : emptyHint}
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
        ? "未检测到 Docker：PATH 中没有 docker 可执行文件。"
        : "Docker 引擎未运行：请启动 Docker Desktop 后重试。"
      : null;

  if (loading && containers == null && !error) {
    return (
      <div className="flex items-center gap-2 p-4 text-[0.8rem] text-[var(--t2,#62666d)]">
        <Loader2 className="size-3.5 animate-spin" /> 正在查询容器状态…
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4 p-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2 text-sm font-medium">
          <Container className="size-4 text-[#12B76A]" /> 容器信息
          <span className="font-mono text-[11px] text-[var(--t3,#8a8f98)]">compose: {composeService}</span>
        </div>
        <Button variant="soft" size="sm" className="gap-1" disabled={loading} onClick={() => void load()}>
          {loading ? <Loader2 className="size-3.5 animate-spin" /> : <RotateCw className="size-3.5" />} 刷新
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
          当前 compose project 中没有服务 {composeService} 的容器（尚未启动，或已被清理）。
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] p-3">
            <div className="mb-1 text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">镜像</div>
            <div className="break-all font-mono text-[0.85rem] font-semibold text-[var(--t1,#222326)]">{container.image}</div>
          </div>
          <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] p-3">
            <div className="mb-1 text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">容器 ID</div>
            <div className="font-mono text-[0.85rem] font-semibold text-[var(--t1,#222326)]" title={container.container_id}>
              {container.container_id.startsWith("sha256:") ? container.container_id.slice(7, 19) : container.container_id.slice(0, 12)}
            </div>
          </div>
          <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] p-3">
            <div className="mb-1 text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
              healthcheck 状态
              <span className="ml-1 font-normal normal-case tracking-normal">（compose 定义，仅展示）</span>
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
              <span className="text-[0.8rem] text-[var(--t3,#8a8f98)]">未定义或无读数</span>
            )}
          </div>
          <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] p-3">
            <div className="mb-1 text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">最近退出码</div>
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
        compose 服务无宿主 pid 与 Job Object 指标（「容器托管」）；compose healthcheck 只展示，健康状态机仍按 YAML health（默认 tcp 打主机映射端口）。
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
  const view: ScriptRuntimeView | null = rt.state.script?.id === id ? rt.state.script : null;
  const isRunning = view?.state === "running";
  // 引擎限制：同时只能跑一个脚本（SCRIPT_BUSY）
  const anyRunning = rt.state.script?.state === "running";

  const run = async () => {
    try {
      await apiScriptRun(id);
      toast(`已开始运行脚本 ${id}`, "info");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };
  const cancel = async () => {
    try {
      await apiScriptCancel(id);
      toast("已发送停止信号", "info");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  return { view, isRunning, anyRunning, run, cancel };
}

function ScriptCard({ id, spec, selected, onOpen }: { id: string; spec: ScriptSpec; selected: boolean; onOpen: () => void }) {
  const { view, isRunning, anyRunning, run, cancel } = useScriptView(id);
  const [confirmStop, setConfirmStop] = useState(false);

  const foot = isRunning ? (
    <span className="flex items-center gap-1 text-[11px] text-[var(--st-ok-deep,#1e7e35)]">
      运行中{view?.pid ? <span className="font-mono"> · pid {view.pid}</span> : null}
    </span>
  ) : view?.last_error ? (
    <span className="block truncate text-[11px] text-[var(--st-danger,#dc2626)]" title={view.last_error}>⚠ {view.last_error}</span>
  ) : view?.last_exit ? (
    <span className={cn("text-[11px]", view.last_exit.code === 0 ? "text-[var(--st-ok-deep,#1e7e35)]" : "text-[var(--st-danger,#dc2626)]")}>
      {scriptStateLabel(view)}
    </span>
  ) : (
    <span className="text-[11px] text-[var(--t3,#8a8f98)]">脚本任务 · {spec.cmds.length} 条命令</span>
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
              title="停止脚本"
              onClick={() => setConfirmStop(true)}
            >
              <Square className="size-3.5" />
            </button>
          ) : (
            <button
              type="button"
              className="grid size-[1.8rem] cursor-pointer place-items-center rounded-[var(--r-sm,8px)] border border-transparent text-[var(--t3,#8a8f98)] transition-colors duration-150 hover:border-[var(--line-strong,#d0d6e0)] hover:bg-[var(--surface-2,#f3f4f5)] hover:text-[var(--st-accent,#5e6ad2)] disabled:cursor-not-allowed disabled:opacity-50"
              title={anyRunning ? "已有脚本在运行" : "运行脚本"}
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
        title={`停止脚本「${id}」？`}
        description="将结束该脚本当前命令的整棵进程树，后续命令不再执行。"
        confirmText="停止脚本"
        cancelText="取消"
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
        toast("复制失败，请手动选择复制", "err");
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
              <Square className="size-3.5" /> 停止
            </Button>
          ) : (
            <Button
              size="sm"
              variant="default"
              className="gap-1"
              disabled={anyRunning}
              title={anyRunning ? "已有脚本在运行：同一工作区同时只能运行一个脚本" : undefined}
              onClick={() => void run()}
            >
              <Play className="size-3.5" /> {view?.last_exit ? "重新运行" : "运行"}
            </Button>
          )}
        </div>
      </div>

      <ConfirmDialog
        open={confirmStop}
        title={`停止脚本「${id}」？`}
        description="将结束该脚本当前命令的整棵进程树，后续命令不再执行。"
        confirmText="停止脚本"
        cancelText="取消"
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
            title={copied ? "已复制" : "复制命令"}
            onClick={copyCmds}
          >
            {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
            {copied ? <span>已复制</span> : null}
          </button>
        </div>
      </div>

      {/* meta strip */}
      <div className="mx-4 mt-2 flex flex-wrap items-center gap-x-[1.1rem] gap-y-1.5 rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] px-3.5 py-2">
        <Meta k="命令" v={`${spec.cmds.length} 条`} />
        <Meta k="工作目录" v={spec.cwd ?? "工作区根目录"} muted />
        <Meta k="超时" v={`${spec.timeout_secs ?? 1800}s`} />
        <Meta k="PID" v={view?.pid != null ? `${view.pid} · Job Object` : "—"} />
        <Meta k="日志" v={`script:${id}`} mono />
        <Meta
          k="上次退出"
          v={view?.last_exit ? `码 ${view.last_exit.code}` : "—"}
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
              title="展开服务列表"
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
              <span className="text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">服务</span>
              <span className="font-mono text-[11px] text-[var(--t3,#8a8f98)]">{serviceIds.length} · 运行 {running}</span>
              <button
                type="button"
                onClick={toggleCards}
                className="ml-auto grid size-7 cursor-pointer place-items-center rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] text-[var(--t2,#62666d)] transition-colors hover:border-[var(--t3,#8a8f98)] hover:bg-[var(--surface-2,#f3f4f5)] hover:text-[var(--t1,#222326)]"
                title={totalItems === 1 ? "收起服务列表（单服务工作区）" : "收起服务列表"}
              >
                <PanelLeftClose className="size-3.5" />
              </button>
            </div>
            <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto">
              {serviceIds.length === 0 ? (
                <div className="rounded-lg border border-dashed border-[var(--line,#e6e6e6)] p-6 text-center text-sm text-[var(--t3,#8a8f98)]">
                  工作区没有可运行的服务。
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
                    <span className="text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">脚本</span>
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
          <div className="flex h-full items-center justify-center text-sm text-[var(--t3,#8a8f98)]">选择一个服务查看详情</div>
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
            <div className="mb-2 text-sm font-semibold text-red-600">启动失败</div>
            <p className="mb-4 whitespace-pre-wrap break-words text-sm text-[var(--t1,#222326)]">
              {rt.state.error}
            </p>
            <div className="flex justify-end">
              <Button onClick={() => rt.actions.clearError()}>知道了</Button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
