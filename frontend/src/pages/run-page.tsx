import { useEffect, useMemo, useRef, useState } from "react";
import { useOutletContext, NavLink } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { cn } from "@/lib/utils";
import { readCardsCollapsedPref, writeCardsCollapsedPref } from "@/lib/workspace-storage";
import { useRuntime } from "@/providers/runtime-provider";
import { useWorkspace } from "@/providers/workspace-provider";
import { useYaml } from "@/providers/yaml-provider";
import { useToast } from "@/components/ui/toast";
import { LogView } from "@/components/log-view";
import {
  STATE_META,
  StatusDot,
  fmtDuration,
  healthClass,
  opErrorLabel,
  stateLabel,
} from "@/lib/status";
import { apiOpenIde } from "../ipc/api";
import { IpcFailure } from "../ipc/protocol";
import type { IdeTarget, LogSource, ScriptSpec, ServiceRuntimeView, ServiceSpec, SuperTaskFile } from "@/ipc/protocol";
import {
  Play,
  Square,
  RotateCw,
  Settings2,
  Activity,
  FileText,
  ExternalLink,
  Copy,
  Lock,
  PanelLeftClose,
  PanelLeftOpen,
} from "lucide-react";
import type { ShellCtx } from "../app/AppShell";

/* ---------------- helpers ---------------- */

function serviceCmd(id: string, s: ServiceSpec): string {
  if (s.kind === "node") {
    const dir = s.dir ?? id;
    const script = s.script ?? "dev";
    return `npm --prefix ${dir} run ${script}`;
  }
  const module = s.module ?? id;
  return `mvn -pl ${module} spring-boot:run`;
}

function kindLabel(kind: string): string {
  if (kind === "node") return "NODE";
  if (kind === "task") return "TASK";
  return "SPRING";
}

function KindBadge({ kind }: { kind: string }) {
  const color = kind === "node" ? "#2E90FA" : kind === "task" ? "var(--t3,#8a8f98)" : "var(--st-accent,#5e6ad2)";
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
        className={cn(
          "place-items-center border text-[var(--t3,#8a8f98)] transition-colors duration-150",
          "hover:border-[var(--line-strong,#d0d6e0)] hover:bg-[var(--surface,#fff)] hover:text-[var(--st-accent,#5e6ad2)]",
          "focus-visible:outline-none focus-visible:border-[var(--st-accent,#5e6ad2)]",
          variant === "icon"
            ? "grid size-[1.8rem] rounded-full border-transparent"
            : "inline-flex h-[1.9rem] items-center gap-1.5 rounded-[var(--r-sm,8px)] border-[var(--line,#e6e6e6)] px-2.5 text-[0.78rem] font-medium",
        )}
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
  const isBusy = svc.state === "starting" || svc.state === "stopping";
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
              className="grid size-[1.8rem] place-items-center rounded-full border border-transparent text-[var(--t3,#8a8f98)] transition-colors duration-150 hover:border-[var(--line-strong,#d0d6e0)] hover:bg-[var(--surface,#fff)] hover:text-[var(--st-accent,#5e6ad2)]"
              title="重启"
              disabled={external}
              onClick={() => runtime.actions.restartOne(id)}
            >
              <RotateCw className="size-3.5" />
            </button>
          ) : null}
          <button
            className={cn(
              "grid size-[1.8rem] place-items-center rounded-full border transition-colors duration-150",
              isRunning
                ? "border-transparent text-[var(--t3,#8a8f98)] hover:border-red-200 hover:bg-[#FDECEC] hover:text-[#DC2626]"
                : "border-transparent text-[var(--t3,#8a8f98)] hover:border-[var(--line-strong,#d0d6e0)] hover:bg-[var(--surface,#fff)] hover:text-[var(--st-accent,#5e6ad2)]",
            )}
            title={isRunning ? "停止进程树" : "启动"}
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
        {svc.port ? <span className="font-mono">:{svc.port}</span> : null}
        {svc.pid ? <span className="font-mono">pid {svc.pid}</span> : null}
        {isRunning && svc.started_at_ms ? <span className="font-mono text-[var(--t3,#8a8f98)]">· {fmtDuration(svc.started_at_ms)}</span> : null}
      </div>

      <div className="mt-1.5 min-h-[1.05rem] overflow-hidden">{foot}</div>
    </div>
  );
}

/* ---------------- env panel ---------------- */

function EnvPanel({ id, compact }: { id: string; compact: boolean }) {
  const ws = useWorkspace();
  const yaml = useYaml();
  const { toast } = useToast();
  const spec = ws.state.spec;
  const svc = spec?.services[id];
  const [portDraft, setPortDraft] = useState<string>(String(svc?.port ?? ""));
  const [envDraft, setEnvDraft] = useState<Record<string, string>>(svc?.env ?? {});

  useEffect(() => {
    setPortDraft(String(svc?.port ?? ""));
    setEnvDraft(svc?.env ?? {});
  }, [id, svc?.port, JSON.stringify(svc?.env)]);

  if (!svc) return null;

  const healthUrl = useMemo(() => {
    const base = svc.health?.http;
    if (!base) return "tcp :" + (portDraft || "?");
    if (!portDraft) return base;
    try {
      const u = new URL(base);
      return `${u.protocol}//${u.hostname}:${portDraft}${u.pathname}`;
    } catch {
      return base;
    }
  }, [svc.health?.http, portDraft]);

  const save = async () => {
    if (!spec) return;
    const next: SuperTaskFile = {
      ...spec,
      services: {
        ...spec.services,
        [id]: {
          ...svc,
          port: portDraft ? Number(portDraft) : null,
          ports: portDraft ? [Number(portDraft)] : [],
          env: envDraft,
        },
      },
    };
    const ok = await yaml.actions.saveForm(next);
    if (ok) {
      await ws.actions.refreshSpec();
      toast("已保存服务配置", "ok");
    } else {
      toast(yaml.state.error ?? "保存失败", "err");
    }
  };

  return (
    <div className={cn("flex flex-col gap-4 p-4", compact && "gap-3 p-3")}>
      <div className="grid grid-cols-[120px_1fr] items-center gap-3">
        <label className="text-sm text-[var(--t2,#62666d)]">端口</label>
        <Input
          type="number"
          value={portDraft}
          onChange={(e) => setPortDraft(e.target.value)}
          className="max-w-[160px]"
        />
      </div>

      <div className="flex items-center gap-2 rounded-lg border border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] px-3 py-2 font-mono text-[12px]">
        <span className="text-[var(--t3,#8a8f98)]">健康检查</span>
        <span className="truncate text-[var(--t1,#222326)]">{healthUrl}</span>
        {svc.health?.http ? (
          <a href={svc.health.http} target="_blank" rel="noreferrer" className="ml-auto text-[var(--st-accent,#5e6ad2)] hover:underline">
            <ExternalLink className="size-3.5" />
          </a>
        ) : null}
      </div>

      <Separator />

      <div className="text-sm font-medium text-[var(--t1,#222326)]">环境变量</div>
      <div className="flex flex-col gap-1.5">
        {Object.keys(envDraft).length === 0 ? (
          <div className="text-sm text-[var(--t3,#8a8f98)]">无</div>
        ) : (
          Object.entries(envDraft).map(([k, v]) => (
            <div key={k} className="grid grid-cols-[1fr_1fr_auto] items-center gap-2">
              <code className="truncate rounded bg-[var(--surface-2,#f3f4f5)] px-2 py-1 font-mono text-[12px]">{k}</code>
              <Input value={v} onChange={(e) => setEnvDraft((m) => ({ ...m, [k]: e.target.value }))} />
            </div>
          ))
        )}
      </div>

      <div className="flex items-center gap-2 pt-1">
        <Button size="sm" onClick={save}>保存</Button>
        <span className="text-[11px] text-[var(--t3,#8a8f98)]">保存后健康检查地址与运行时端口自动跟随。</span>
      </div>
    </div>
  );
}

/* ---------------- health panel ---------------- */

function HealthPanel({ svc, spec }: { svc: ServiceRuntimeView; spec: ServiceSpec | undefined }) {
  const h = spec?.health;
  const httpType = h?.type === "http";
  const last = svc.health;
  const failReason =
    svc.last_error
      ? svc.last_error
      : svc.state === "running"
        ? "—（无）"
        : "未运行，检查暂停";

  return (
    <div className="grid grid-cols-1 gap-0 md:grid-cols-[18rem_1fr]">
      <aside className="border-[var(--line,#e6e6e6)] p-4 md:border-r">
        <div className="mb-3 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
          健康参数
          <span className="ml-auto inline-flex items-center gap-1 rounded-full bg-[var(--ok-tint,#e9f7ed)] px-1.5 py-0.5 text-[var(--st-ok-deep,#1e7e35)]">
            <span className="size-1.5 animate-pulse rounded-full bg-[var(--st-ok,#27a644)]" />
            {svc.state === "running" ? "watching" : "paused"}
          </span>
        </div>
        <div className="flex flex-col gap-2 text-[13px]">
          <div className="grid grid-cols-[5rem_1fr] gap-2 border-b border-dashed border-[var(--line,#e6e6e6)] pb-2">
            <span className="text-[var(--t3,#8a8f98)]">类型</span>
            <span className="font-medium">{h ? (httpType ? "http（2xx 为成功）" : h.type === "tcp" ? "tcp（端口开放为成功）" : "none") : "未配置"}</span>
          </div>
          <div className="grid grid-cols-[5rem_1fr] gap-2 border-b border-dashed border-[var(--line,#e6e6e6)] pb-2">
            <span className="text-[var(--t3,#8a8f98)]">URL</span>
            <span className="font-mono text-[12px]">{h?.http ?? (svc.port ? `tcp :${svc.port}` : "—")}</span>
          </div>
          <div className="grid grid-cols-[5rem_1fr] gap-2 border-b border-dashed border-[var(--line,#e6e6e6)] pb-2">
            <span className="text-[var(--t3,#8a8f98)]">间隔/超时</span>
            <span className="font-mono text-[12px]">{h ? `${h.interval_secs}s / ${h.timeout_secs}s` : "—"}</span>
          </div>
          <div className="grid grid-cols-[5rem_1fr] gap-2">
            <span className="text-[var(--t3,#8a8f98)]">失败原因</span>
            <span className={cn("font-medium", svc.last_error ? "text-[var(--st-danger,#dc2626)]" : "text-[var(--t2,#62666d)]")}>{failReason}</span>
          </div>
        </div>
      </aside>

      <section className="flex flex-col gap-3 p-4">
        <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
          最近结果
          <span className="ml-auto flex items-center gap-3 text-[10px] font-normal normal-case">
            <span><span className="mr-1 inline-block size-2 rounded-sm bg-[var(--st-ok,#27a644)]" />成功</span>
            <span><span className="mr-1 inline-block size-2 rounded-sm bg-[var(--st-warn,#9a6700)]" />慢</span>
            <span><span className="mr-1 inline-block size-2 rounded-sm bg-[var(--st-danger,#dc2626)]" />失败</span>
          </span>
        </div>
        <HealthSparkline ok={last?.ok} />
        <div className="flex items-center gap-2 text-[13px]">
          <span className="text-[var(--t3,#8a8f98)]">最近</span>
          <span className={cn("font-medium", healthClass(last?.ok))}>
            {last ? (last.ok ? "健康" : "异常") : "未配置"}
          </span>
          {last ? <span className="font-mono text-[12px] text-[var(--t2,#62666d)]">{last.detail}</span> : null}
        </div>
      </section>
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
  const [tab, setTab] = useState<"logs" | "env" | "health" | "config">("logs");
  const [confirmStop, setConfirmStop] = useState(false);
  const svc = rt.state.services[id];
  const spec = ws.state.spec?.services[id];

  if (!svc) return null;
  const isRunning = svc.state === "running";
  const isBusy = svc.state === "starting" || svc.state === "stopping";
  const external = isRunning && svc.managed === false;
  const source: LogSource = { kind: "service", id };
  const cmd = spec ? serviceCmd(id, spec) : "";

  const stack = svc.kind === "node" ? `Node · ${spec?.package_manager ?? "npm"}` : `Spring Boot · ${spec?.module ?? id}`;
  const topo = spec?.depends_on?.length ? `依赖 ${spec.depends_on.join(", ")}` : "无依赖";

  const tabs = [
    { k: "logs", label: "日志", icon: FileText },
    { k: "env", label: "环境", icon: Settings2 },
    { k: "health", label: "健康", icon: Activity },
    { k: "config", label: "配置", icon: FileText },
  ] as const;
  const locked = [
    { label: "终端", v: "1.2" },
    { label: "指标", v: "1.2" },
    { label: "容器", v: "1.3" },
    { label: "代理", v: "1.6" },
  ];

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* head */}
      <div className={cn("flex items-center gap-3 border-b border-[var(--line,#e6e6e6)] px-4 py-3", compact && "px-3 py-2")}>
        <StatusDot state={svc.state} size={10} />
        <h1 className="text-[1.08rem] font-bold tracking-tight text-[var(--t1,#222326)]">{id}</h1>
        <KindBadge kind={svc.kind} />
        {external ? (
          <span
            className="rounded-full bg-[var(--surface-2,#f3f4f5)] px-2 py-0.5 text-[11px] font-medium text-[var(--t2,#62666d)]"
            title="外部进程：非 SuperTask 启动，仅监控"
          >
            外部 · 仅监控
          </span>
        ) : null}
        <span
          className={cn(
            "rounded-full px-2 py-0.5 text-[11px] font-medium",
            isRunning ? "bg-[var(--ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]" : "bg-[var(--surface-2,#f3f4f5)] text-[var(--t2,#62666d)]",
          )}
        >
          {stateLabel(svc.state)}
          {isRunning && svc.started_at_ms ? ` · ${fmtDuration(svc.started_at_ms)}` : ""}
        </span>
        <div className="ml-auto flex gap-2">
          <IdeOpenMenu variant="button" />
          {isRunning ? (
            <Button
              size="sm"
              variant="outline"
              className="gap-1 hover:border-[var(--t3,#8a8f98)] hover:bg-[#FCFCFD]"
              disabled={isBusy}
              onClick={() => runtime.actions.restartOne(id)}
            >
              <RotateCw /> 重启
            </Button>
          ) : null}
          {isRunning || svc.state === "starting" ? (
            <Button
              size="sm"
              variant="outline"
              className="gap-1 hover:border-[#DC2626] hover:bg-[#FDECEC] hover:text-[#DC2626]"
              disabled={isBusy}
              onClick={() => (isRunning ? setConfirmStop(true) : runtime.actions.stopOne(id))}
            >
              <Square /> 停止进程树
            </Button>
          ) : (
            <Button size="sm" variant="default" className="gap-1" disabled={isBusy} onClick={() => runtime.actions.startOne(id)}>
              <Play /> {svc.state === "exited" || svc.last_error ? "重试启动" : "启动"}
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

      {/* command line */}
      <div className="mx-4 mt-3 flex items-center gap-2 rounded-[var(--r-md,12px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] px-3 py-2 font-mono text-[0.74rem] text-[var(--t1,#222326)] transition-colors duration-150 hover:border-[var(--line-strong,#d0d6e0)]">
        <span className="font-bold text-[var(--t3,#8a8f98)]">$</span>
        <span className="min-w-0 flex-1 truncate">{cmd}</span>
        <button
          className="rounded-[var(--r-sm,8px)] p-1 text-[var(--t3,#8a8f98)] transition-all duration-150 hover:bg-[var(--surface,#fff)] hover:text-[var(--st-accent,#5e6ad2)]"
          title="复制命令"
          onClick={() => navigator.clipboard?.writeText(cmd)}
        >
          <Copy className="size-3.5" />
        </button>
      </div>

      {/* meta strip */}
      <div className="mx-4 mt-3 flex flex-wrap items-center gap-x-[0.9rem] gap-y-1.5 rounded-[var(--r-md,12px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] px-3.5 py-2">
        <Meta k="端口" v={svc.port != null ? `:${svc.port}` : "—"} accent />
        <Meta k="运行栈" v={stack} />
        <Meta k="拓扑" v={topo} muted />
        <Meta k="PID" v={svc.pid != null ? `${svc.pid}${isRunning ? " · Job Object" : ""}` : "—"} />
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
      </div>
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
    <span className="relative inline-flex items-center gap-[0.42rem] pl-[0.9rem] first:pl-0 [&:not(:first-child)]:before:absolute [&:not(:first-child)]:before:left-0 [&:not(:first-child)]:before:top-1/2 [&:not(:first-child)]:before:h-[0.85em] [&:not(:first-child)]:before:w-px [&:not(:first-child)]:before:-translate-y-1/2 [&:not(:first-child)]:before:bg-[var(--line-strong,#d0d6e0)] [&:not(:first-child)]:before:content-['']">
      <span className="text-[0.56rem] font-semibold uppercase tracking-[0.08em] text-[var(--t3,#8a8f98)]">{k}</span>
      <span
        className={cn(
          "text-[0.74rem]",
          mono || accent || ok ? "font-mono" : "",
          accent && "font-bold text-[var(--st-accent,#5e6ad2)]",
          ok && "font-bold text-[var(--st-ok-deep,#1e7e35)]",
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

function ScriptCard({ id, spec, selected, onOpen }: { id: string; spec: ScriptSpec; selected: boolean; onOpen: () => void }) {
  return (
    <div
      onClick={onOpen}
      className={cn(
        "relative flex h-[5.7rem] cursor-pointer flex-col overflow-hidden rounded-xl border bg-[var(--surface,#fff)] p-2.5 transition-all",
        selected ? "border-[var(--st-accent,#5e6ad2)] bg-[color-mix(in_oklch,var(--st-accent,#5e6ad2)_6%,white)] ring-1 ring-[var(--st-accent,#5e6ad2)]/30" : "border-[var(--line,#e6e6e6)] hover:border-[var(--line-strong,#d0d6e0)]",
      )}
    >
      <div className="flex items-center gap-2">
        <span className="size-2 rounded-full bg-[var(--t3,#8a8f98)]" />
        <span className="truncate text-[0.88rem] font-semibold text-[var(--t1,#222326)]">{id}</span>
        <KindBadge kind="task" />
      </div>
      <div className="mt-1.5 truncate text-[11px] text-[var(--t2,#62666d)]">{spec.desc ?? spec.cmds.join(" ; ")}</div>
      <div className="mt-auto text-[11px] text-[var(--t3,#8a8f98)]">脚本任务 · 运行将在 1.1 提供</div>
    </div>
  );
}

function ScriptDetail({ id }: { id: string }) {
  const ws = useWorkspace();
  const spec = ws.state.spec?.scripts[id];
  if (!spec) return null;
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 p-4">
      <div className="flex items-center gap-2">
        <h1 className="text-[1.08rem] font-bold text-[var(--t1,#222326)]">{id}</h1>
        <KindBadge kind="task" />
      </div>
      <p className="text-sm text-[var(--t2,#62666d)]">{spec.desc ?? "脚本任务"}</p>
      <pre className="overflow-auto rounded-lg border border-[var(--line,#e6e6e6)] bg-[#FBFBFC] p-3 font-mono text-[12px] text-[var(--t2,#62666d)]">
        {spec.cmds.join("\n")}
      </pre>
      <div className="rounded-lg border border-dashed border-[var(--line-strong,#d0d6e0)] p-4 text-sm text-[var(--t3,#8a8f98)]">
        脚本任务运行将在 <span className="font-medium text-[var(--t2,#62666d)]">1.1</span> 提供（复用于服务相同的进程树）。当前仅探测，不运行、不展示假数据。
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
              className="grid size-8 place-items-center rounded-[var(--r-sm,8px)] text-[var(--t3,#8a8f98)] transition-colors hover:bg-[rgb(0_0_0_/_0.045)] hover:text-[var(--t1,#222326)]"
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
                className="ml-auto grid size-7 place-items-center rounded-[var(--r-sm,8px)] text-[var(--t3,#8a8f98)] transition-colors hover:bg-[rgb(0_0_0_/_0.045)] hover:text-[var(--t1,#222326)]"
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
