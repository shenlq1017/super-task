import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Activity,
  Cpu,
  HardDrive,
  MemoryStick,
  Network,
  RefreshCw,
  Stethoscope,
  Thermometer,
  Info,
} from "lucide-react";
import { Card } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { apiDockerProbe, apiSystemInfo, apiSystemMetrics, apiToolchainProbe } from "../ipc/api";
import { useRuntime } from "@/providers/runtime-provider";
import { StatusChip } from "@/lib/status";
import type { ServiceMetrics, ServiceRuntimeView } from "../ipc/protocol";
import type {
  DockerProbe,
  HostMetrics,
  SystemInfo,
  ToolchainProbeOut,
} from "../ipc/protocol";
import { TEMP_MODES } from "../ipc/protocol";
import { fmtBytes, fmtRate, loadColor, pct, tempColor } from "@/lib/metrics";
import { pickTempMode, useTempMode } from "@/lib/temp-mode";
import { recordHostMetrics, useMetricsHistory } from "@/lib/metrics-history";
import { downloadTextFile } from "@/lib/download-text";

const PREFS_KEY = "st:monitor:prefs";
const REFRESH_OPTIONS = [1000, 2000, 3000, 5000] as const;
type RefreshMs = (typeof REFRESH_OPTIONS)[number];

type MonitorPrefs = {
  refreshMs: RefreshMs;
};

type DetailKind = "cpu" | "memory" | "disk" | "temp" | "network" | "sysinfo" | "service" | "doctor" | null;

function loadPrefs(): MonitorPrefs {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (!raw) return { refreshMs: 1000 };
    const parsed = JSON.parse(raw) as Partial<MonitorPrefs>;
    const ms = REFRESH_OPTIONS.includes(parsed.refreshMs as RefreshMs)
      ? (parsed.refreshMs as RefreshMs)
      : 1000;
    return { refreshMs: ms };
  } catch {
    return { refreshMs: 1000 };
  }
}

function savePrefs(prefs: MonitorPrefs) {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch {
    /* ignore */
  }
}

function fmtTime(ms: number | null): string {
  if (ms == null) return "\u2014";
  return new Date(ms).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function loadTone(p: number | null): "default" | "warn" | "danger" | "accent" {
  if (p == null) return "default";
  if (p >= 90) return "danger";
  if (p >= 75) return "warn";
  return "accent";
}

function tempTone(c: number | null): "default" | "warn" | "danger" | "accent" {
  if (c == null) return "default";
  if (c >= 85) return "danger";
  if (c >= 70) return "warn";
  return "accent";
}

function SummaryChip({
  label,
  value,
  tone = "default",
  title,
  onClick,
}: {
  label: string;
  value: number | string;
  tone?: "default" | "accent" | "warn" | "danger";
  title?: string;
  onClick?: () => void;
}) {
  const Comp = onClick ? "button" : "span";
  return (
    <Comp
      type={onClick ? "button" : undefined}
      title={title}
      onClick={onClick}
      className={cn(
        "inline-flex h-6 items-center gap-1.5 rounded-full border px-2.5 text-[0.72rem] leading-none",
        onClick && "cursor-pointer transition-colors hover:opacity-90 active:scale-95",
        tone === "accent" &&
          "border-[color-mix(in_srgb,var(--st-accent)_35%,transparent)] bg-[var(--st-accent-tint)] text-[var(--st-accent)]",
        tone === "warn" &&
          "border-[var(--st-warn-line)] bg-[var(--st-warn-tint)] text-[var(--st-warn)]",
        tone === "danger" &&
          "border-[var(--st-danger-ring)] bg-[var(--st-danger-tint)] text-[var(--st-danger)]",
        tone === "default" &&
          "border-[var(--line-strong)] bg-[var(--surface)] text-[var(--t2)]",
      )}
    >
      <span className="opacity-80">{label}</span>
      <span className="font-mono font-semibold tabular-nums text-[var(--t1)]">{value}</span>
    </Comp>
  );
}

function PageCard(props: {
  title: string;
  children: React.ReactNode;
  className?: string;
  action?: React.ReactNode;
  onClick?: () => void;
}) {
  return (
    <Card
      className={cn(
        "flex flex-col gap-3 p-4",
        props.onClick &&
          "cursor-pointer transition-colors hover:border-[var(--line-strong)] hover:bg-[var(--surface-2)]/40",
        props.className,
      )}
      onClick={props.onClick}
      role={props.onClick ? "button" : undefined}
      tabIndex={props.onClick ? 0 : undefined}
      onKeyDown={
        props.onClick
          ? (e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                props.onClick?.();
              }
            }
          : undefined
      }
    >
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-[13px] font-semibold text-[var(--t1)]">{props.title}</h3>
        {props.action}
      </div>
      {props.children}
    </Card>
  );
}

function StatCell(props: { label: string; value: React.ReactNode; className?: string }) {
  return (
    <div className={cn("min-w-0", props.className)}>
      <div className="truncate text-[11px] text-[var(--t3)]">{props.label}</div>
      <div className="mt-0.5 truncate font-mono text-[13px] font-semibold tabular-nums text-[var(--t1)]">
        {props.value}
      </div>
    </div>
  );
}

function MeterBar(props: { ratio: number | null; className?: string; color?: string }) {
  return (
    <div className={cn("h-2 overflow-hidden rounded-full bg-[var(--surface-2)]", props.className)}>
      <div
        className="h-full rounded-full transition-[width] duration-500"
        style={{
          width: (props.ratio ?? 0) + "%",
          background: props.color ?? loadColor(props.ratio),
        }}
      />
    </div>
  );
}

/** Tiny bar sparkline with threshold coloring (status-bar style). */
function BarSparkline({ values, colorFn }: { values: number[]; colorFn?: (v: number) => string }) {
  if (values.length < 2) return null;
  const shown = values.slice(-48);
  const paint = colorFn ?? loadColor;
  return (
    <span className="flex h-8 w-full items-end gap-px" aria-hidden>
      {shown.map((v, i) => (
        <span
          key={i}
          className="min-w-[2px] flex-1 rounded-sm"
          style={{
            height: Math.max(3, Math.round((Math.min(100, Math.max(0, v)) / 100) * 32)) + "px",
            background: paint(v),
            opacity: 0.35 + (0.65 * (i + 1)) / shown.length,
          }}
        />
      ))}
    </span>
  );
}

/** Semicircular load gauge. */
function CpuGauge({ value }: { value: number | null }) {
  const cx = 100;
  const cy = 96;
  const r = 76;
  const arc = `M ${cx - r} ${cy} A ${r} ${r} 0 0 1 ${cx + r} ${cy}`;
  const angle = value == null ? Math.PI : Math.PI * (1 - Math.min(100, Math.max(0, value)) / 100);
  const tipX = cx + 58 * Math.cos(angle);
  const tipY = cy - 58 * Math.sin(angle);
  return (
    <svg viewBox="0 0 200 104" className="mx-auto w-full max-w-[230px]" role="img" aria-hidden>
      <defs>
        <linearGradient id="st-monitor-gauge" x1="0" y1="0" x2="1" y2="0">
          <stop offset="0%" stopColor="var(--st-ok)" />
          <stop offset="55%" stopColor="var(--st-warn)" />
          <stop offset="100%" stopColor="var(--st-danger)" />
        </linearGradient>
      </defs>
      <path d={arc} fill="none" stroke="var(--surface-2)" strokeWidth={13} strokeLinecap="round" />
      <path
        d={arc}
        fill="none"
        stroke="url(#st-monitor-gauge)"
        strokeWidth={13}
        strokeLinecap="round"
        opacity={value == null ? 0.25 : 1}
      />
      <line
        x1={cx}
        y1={cy}
        x2={tipX}
        y2={tipY}
        stroke="var(--t1)"
        strokeWidth={7}
        strokeLinecap="round"
        opacity={value == null ? 0.3 : 1}
      />
      <circle cx={cx} cy={cy} r={7} fill="var(--t1)" opacity={value == null ? 0.3 : 1} />
    </svg>
  );
}

/** Rolling usage area chart with amber/red threshold guides. */
function AreaChart({
  values,
  emptyLabel,
}: {
  values: number[];
  emptyLabel: string;
}) {
  const W = 300;
  const H = 88;
  if (values.length < 2) {
    return (
      <div className="flex h-[88px] w-full items-center justify-center text-[11px] text-[var(--t3)]">
        {emptyLabel}
      </div>
    );
  }
  const last = values[values.length - 1] ?? 0;
  const stroke = loadColor(last);
  const pts = values.map((v, i) => {
    const x = (i / (values.length - 1)) * W;
    const y = H - (Math.min(100, Math.max(0, v)) / 100) * (H - 8) - 4;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  });
  const y75 = H - (75 / 100) * (H - 8) - 4;
  const y90 = H - (90 / 100) * (H - 8) - 4;
  return (
    <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" className="h-[88px] w-full" aria-hidden>
      <line
        x1={0}
        y1={y75}
        x2={W}
        y2={y75}
        stroke="var(--st-warn)"
        strokeWidth={1}
        strokeDasharray="3 3"
        opacity={0.45}
        vectorEffect="non-scaling-stroke"
      />
      <line
        x1={0}
        y1={y90}
        x2={W}
        y2={y90}
        stroke="var(--st-danger)"
        strokeWidth={1}
        strokeDasharray="3 3"
        opacity={0.45}
        vectorEffect="non-scaling-stroke"
      />
      <path d={`M${pts.join(" L")} L${W},${H} L0,${H} Z`} fill={stroke} opacity={0.18} />
      <path
        d={`M${pts.join(" L")}`}
        fill="none"
        stroke={stroke}
        strokeWidth={1.75}
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}

function useHostMetrics(refreshMs: RefreshMs, paused: boolean) {
  const tempMode = useTempMode();
  const [host, setHost] = useState<HostMetrics | null>(null);
  const [lastUpdated, setLastUpdated] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const alive = useRef(true);

  const tick = useCallback(async () => {
    try {
      const m = await apiSystemMetrics(tempMode);
      if (!alive.current) return;
      setHost(m);
      recordHostMetrics(m);
      setLastUpdated(m.sampledAtMs > 0 ? m.sampledAtMs : Date.now());
    } catch {
      // Ambient page: a failed sample keeps the previous reading.
    } finally {
      if (alive.current) setLoading(false);
    }
  }, [tempMode]);

  useEffect(() => {
    alive.current = true;
    void tick();
    if (paused) {
      return () => {
        alive.current = false;
      };
    }
    const id = window.setInterval(() => void tick(), refreshMs);
    return () => {
      alive.current = false;
      window.clearInterval(id);
    };
  }, [tempMode, refreshMs, paused, tick]);

  return { host, lastUpdated, loading, refreshNow: tick };
}

/**
 * 系统监控：整机 CPU / 内存 / 存储 / 网络的实时面板 + 按服务归因的资源占用。
 * 整机数据来自 `system.metrics`；历史曲线进 metrics-history 环形缓冲。
 * 详情用浮动 Dialog，避免撑开布局；详情打开时暂停自动刷新。
 */
export function MonitorPage() {
  const { t } = useTranslation();
  const [prefs, setPrefs] = useState<MonitorPrefs>(() => loadPrefs());
  const [detail, setDetail] = useState<DetailKind>(null);
  const [serviceFocus, setServiceFocus] = useState<ServiceRuntimeView | null>(null);
  const tempMode = useTempMode();
  const dialogOpen = detail != null;
  const { host, lastUpdated, loading, refreshNow } = useHostMetrics(prefs.refreshMs, dialogOpen);
  const history = useMetricsHistory();

  const cpuSeries = useMemo(
    () => history.map((s) => s.cpu).filter((v): v is number => v != null),
    [history],
  );
  const memSeries = useMemo(
    () => history.map((s) => s.mem).filter((v): v is number => v != null),
    [history],
  );
  const windowMinutes =
    history.length >= 2
      ? Math.max(1, Math.round((history[history.length - 1].at - history[0].at) / 60000))
      : 0;

  const cpu = host?.cpuPercent ?? null;
  const memPct = pct(host?.memoryUsedBytes ?? null, host?.memoryTotalBytes ?? null);
  const diskPct = pct(host?.diskUsedBytes ?? null, host?.diskTotalBytes ?? null);
  const temp = host?.cpuTempC ?? null;
  const tempSupported = host?.cpuTempSupported ?? true;
  const swapTotal = host?.swapTotalBytes ?? null;

  const setRefreshMs = (ms: RefreshMs) => {
    const next = { refreshMs: ms };
    setPrefs(next);
    savePrefs(next);
  };

  const openDetail = (kind: DetailKind, svc?: ServiceRuntimeView) => {
    setServiceFocus(svc ?? null);
    setDetail(kind);
  };

  const closeDetail = () => {
    setDetail(null);
    setServiceFocus(null);
  };

  const healthTone =
    (cpu != null && cpu >= 90) ||
    (memPct != null && memPct >= 90) ||
    (diskPct != null && diskPct >= 90) ||
    (temp != null && temp >= 85)
      ? "danger"
      : (cpu != null && cpu >= 75) ||
          (memPct != null && memPct >= 75) ||
          (diskPct != null && diskPct >= 75) ||
          (temp != null && temp >= 70)
        ? "warn"
        : host == null
          ? "default"
          : "accent";

  const healthLabel =
    healthTone === "danger"
      ? t("pages.monitor.healthHot")
      : healthTone === "warn"
        ? t("pages.monitor.healthElevated")
        : host == null
          ? t("pages.monitor.healthUnknown")
          : t("pages.monitor.healthOk");

  const tempDisplay =
    temp != null
      ? temp.toFixed(0) + " \u00b0C"
      : !tempSupported
        ? t("pages.monitor.tempUnsupported")
        : tempMode === "off"
          ? t("pages.monitor.tempOff")
          : t("pages.monitor.unavailable");

  const split = [
    { label: t("pages.monitor.system"), v: host?.cpuSystemPercent ?? null },
    { label: t("pages.monitor.user"), v: host?.cpuUserPercent ?? null },
    { label: t("pages.monitor.nice"), v: host?.cpuNicePercent ?? null },
    { label: t("pages.monitor.idle"), v: host?.cpuIdlePercent ?? null },
  ];

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto">
        <div className="sticky top-0 z-10 border-b border-[var(--line)] bg-[var(--surface)]/95 px-6 py-3 backdrop-blur-sm">
          <div className="mx-auto flex max-w-6xl flex-col gap-2.5">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0">
                <h2 className="text-[1.05rem] font-bold tracking-tight text-[var(--t1)]">
                  {t("pages.monitor.title")}
                </h2>
                <p className="mt-0.5 text-[0.78rem] text-[var(--t3)]">
                  {t("pages.monitor.subtitle")}
                  {dialogOpen ? (
                    <span className="ml-1 text-[var(--st-warn)]">
                      · {t("pages.monitor.refreshPaused")}
                    </span>
                  ) : null}
                </p>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <span className="font-mono text-[0.66rem] text-[var(--t3)]">
                  {t("pages.monitor.updatedAt", { time: fmtTime(lastUpdated) })}
                </span>

                <label className="flex items-center gap-1.5 text-[11px] text-[var(--t3)]">
                  <span className="shrink-0">{t("pages.monitor.refreshInterval")}</span>
                  <select
                    value={prefs.refreshMs}
                    onChange={(e) => setRefreshMs(Number(e.target.value) as RefreshMs)}
                    className="h-7 rounded-[var(--r-sm)] border border-[var(--line)] bg-[var(--surface)] px-1.5 font-mono text-[11px] text-[var(--t1)]"
                    aria-label={t("pages.monitor.refreshInterval")}
                  >
                    {REFRESH_OPTIONS.map((ms) => (
                      <option key={ms} value={ms}>
                        {ms === 1000
                          ? t("pages.monitor.refresh1s")
                          : ms === 2000
                            ? t("pages.monitor.refresh2s")
                            : ms === 3000
                              ? t("pages.monitor.refresh3s")
                              : t("pages.monitor.refresh5s")}
                      </option>
                    ))}
                  </select>
                </label>

                <div
                  className="flex overflow-hidden rounded-[var(--r-sm)] border border-[var(--line)]"
                  role="group"
                  aria-label={t("statusBar.tempModeLabel")}
                >
                  {TEMP_MODES.map((mode) => (
                    <button
                      key={mode}
                      type="button"
                      disabled={!tempSupported && mode !== "off"}
                      onClick={() => pickTempMode(mode)}
                      title={t(`statusBar.tempModes.${mode}.hint`)}
                      className={cn(
                        "px-2 py-0.5 text-[11px] transition-colors duration-150",
                        "border-l border-[var(--line)] first:border-l-0",
                        tempMode === mode
                          ? "bg-[var(--st-accent-tint)] font-semibold text-[var(--st-accent-hover)]"
                          : "text-[var(--t2)] hover:bg-[var(--surface-2)]",
                        !tempSupported &&
                          mode !== "off" &&
                          "cursor-not-allowed opacity-40 hover:bg-transparent",
                      )}
                    >
                      {t(`statusBar.tempModes.${mode}.label`)}
                    </button>
                  ))}
                </div>

                <Button
                  variant="soft"
                  size="sm"
                  onClick={() => void refreshNow()}
                  disabled={loading && host == null}
                  className="gap-1"
                >
                  <RefreshCw className={cn(loading && host == null && "animate-spin")} />
                  {t("common.refresh")}
                </Button>
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-2">
              <SummaryChip
                label={t("pages.monitor.chipCpu")}
                value={cpu == null ? "\u2014" : cpu.toFixed(0) + "%"}
                tone={loadTone(cpu)}
                onClick={() => openDetail("cpu")}
                title={t("pages.monitor.chipCpuHint")}
              />
              <SummaryChip
                label={t("pages.monitor.chipMem")}
                value={memPct == null ? "\u2014" : memPct.toFixed(0) + "%"}
                tone={loadTone(memPct)}
                onClick={() => openDetail("memory")}
                title={t("pages.monitor.chipMemHint")}
              />
              <SummaryChip
                label={t("pages.monitor.chipDisk")}
                value={diskPct == null ? "\u2014" : diskPct.toFixed(0) + "%"}
                tone={loadTone(diskPct)}
                onClick={() => openDetail("disk")}
                title={t("pages.monitor.chipDiskHint")}
              />
              <SummaryChip
                label={t("pages.monitor.chipTemp")}
                value={
                  temp != null
                    ? temp.toFixed(0) + "\u00b0"
                    : !tempSupported
                      ? t("pages.monitor.tempUnsupportedShort")
                      : "\u2014"
                }
                tone={tempTone(temp)}
                onClick={() => openDetail("temp")}
                title={t("pages.monitor.chipTempHint")}
              />
              <SummaryChip
                label={t("pages.monitor.chipHealth")}
                value={healthLabel}
                tone={healthTone}
              />
            </div>
          </div>
        </div>

        <div className="mx-auto flex max-w-6xl flex-col gap-4 p-6">
          {loading && host == null ? (
            <div
              className="flex h-40 items-center justify-center rounded-[var(--r-md)] border border-dashed border-[var(--line-strong)] text-[0.8rem] text-[var(--t3)]"
              role="status"
            >
              {t("pages.monitor.loading")}
            </div>
          ) : null}

          <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
            <HeroMetricCard
              icon={<Cpu className="size-4" />}
              title={t("pages.monitor.cpuLoad")}
              value={cpu == null ? "\u2014" : cpu.toFixed(1) + "%"}
              color={loadColor(cpu)}
              meter={cpu}
              spark={cpuSeries}
              onClick={() => openDetail("cpu")}
              hint={t("pages.monitor.openDetail")}
            />
            <HeroMetricCard
              icon={<MemoryStick className="size-4" />}
              title={t("pages.monitor.memory")}
              value={memPct == null ? "\u2014" : memPct.toFixed(1) + "%"}
              sub={
                host?.memoryTotalBytes
                  ? fmtBytes(host.memoryUsedBytes) + " / " + fmtBytes(host.memoryTotalBytes)
                  : undefined
              }
              color={loadColor(memPct)}
              meter={memPct}
              spark={memSeries}
              onClick={() => openDetail("memory")}
              hint={t("pages.monitor.openDetail")}
            />
            <HeroMetricCard
              icon={<HardDrive className="size-4" />}
              title={t("pages.monitor.storage")}
              value={diskPct == null ? "\u2014" : diskPct.toFixed(1) + "%"}
              sub={
                host?.diskTotalBytes
                  ? fmtBytes(host.diskUsedBytes) + " / " + fmtBytes(host.diskTotalBytes)
                  : undefined
              }
              color={loadColor(diskPct)}
              meter={diskPct}
              onClick={() => openDetail("disk")}
              hint={t("pages.monitor.openDetail")}
            />
            <HeroMetricCard
              icon={<Thermometer className="size-4" />}
              title={t("pages.monitor.cpuTemp")}
              value={tempDisplay}
              color={tempColor(temp)}
              meter={temp == null ? null : Math.min(100, temp)}
              meterColor={tempColor(temp)}
              onClick={() => openDetail("temp")}
              hint={t("pages.monitor.openDetail")}
              muted={temp == null}
            />
          </div>

          <PageCard
            title={t("pages.monitor.historyTitle")}
            action={
              windowMinutes > 0 ? (
                <span className="text-[11px] text-[var(--t3)]">
                  {t("pages.monitor.historyWindow", { n: windowMinutes })}
                </span>
              ) : null
            }
          >
            <div className="grid gap-4 md:grid-cols-2">
              <div>
                <div className="mb-1 flex items-center justify-between gap-2">
                  <span className="text-[11px] font-medium text-[var(--t2)]">CPU</span>
                  <span
                    className="font-mono text-[11px] tabular-nums"
                    style={{ color: loadColor(cpu) }}
                  >
                    {cpu == null ? "\u2014" : cpu.toFixed(1) + "%"}
                  </span>
                </div>
                <AreaChart values={cpuSeries} emptyLabel={t("pages.monitor.collecting")} />
                <div className="mt-1 flex justify-between text-[10px] text-[var(--t3)]">
                  <span>{t("pages.monitor.thresholdWarn")}</span>
                  <span>{t("pages.monitor.thresholdDanger")}</span>
                </div>
              </div>
              <div>
                <div className="mb-1 flex items-center justify-between gap-2">
                  <span className="text-[11px] font-medium text-[var(--t2)]">
                    {t("pages.monitor.pressure")}
                  </span>
                  <span
                    className="font-mono text-[11px] tabular-nums"
                    style={{ color: loadColor(memPct) }}
                  >
                    {memPct == null ? "\u2014" : memPct.toFixed(1) + "%"}
                  </span>
                </div>
                <AreaChart values={memSeries} emptyLabel={t("pages.monitor.collecting")} />
                <div className="mt-1 flex justify-between text-[10px] text-[var(--t3)]">
                  <span>{t("pages.monitor.thresholdWarn")}</span>
                  <span>{t("pages.monitor.thresholdDanger")}</span>
                </div>
              </div>
            </div>
          </PageCard>

          <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-6">
            <PageCard
              title={t("pages.monitor.network")}
              className="xl:col-span-2"
              onClick={() => openDetail("network")}
              action={<Network className="size-3.5 text-[var(--t3)]" aria-hidden />}
            >
              <div className="grid grid-cols-3 gap-2">
                <StatCell label={t("pages.monitor.localIp")} value={host?.netLocalIp ?? "\u2014"} />
                <StatCell
                  label={t("pages.monitor.upload")}
                  value={fmtRate(host?.netUploadBps ?? null)}
                  className="text-right"
                />
                <StatCell
                  label={t("pages.monitor.download")}
                  value={fmtRate(host?.netDownloadBps ?? null)}
                  className="text-right"
                />
              </div>
            </PageCard>

            <SystemInfoCard onOpen={() => openDetail("sysinfo")} />

            <ServiceAttribCard
              onOpenService={(svc) => openDetail("service", svc)}
              onOpenList={() => openDetail("service")}
            />

            <DoctorCard onOpen={() => openDetail("doctor")} />
          </div>
        </div>
      </div>

      <Dialog open={detail === "cpu"} onOpenChange={(o) => !o && closeDetail()}>
        <DialogContent className="sm:max-w-md" showCloseButton>
          <DialogHeader>
            <DialogTitle>{t("pages.monitor.cpuLoad")}</DialogTitle>
            <DialogDescription>{t("pages.monitor.detailCpuDesc")}</DialogDescription>
          </DialogHeader>
          <div className="-mt-1">
            <CpuGauge value={cpu} />
            <div
              className="mt-1 text-center font-mono text-[26px] font-semibold tabular-nums"
              style={{ color: loadColor(cpu) }}
            >
              {cpu == null ? "\u2014" : cpu.toFixed(1) + "%"}
            </div>
          </div>
          <div className="grid grid-cols-4 gap-2">
            {split.map((s) => (
              <StatCell
                key={s.label}
                label={s.label}
                value={s.v == null ? "\u2014" : s.v.toFixed(1) + "%"}
              />
            ))}
          </div>
          {cpuSeries.length >= 2 ? (
            <div>
              <div className="mb-1 text-[11px] text-[var(--t3)]">{t("pages.monitor.historyTitle")}</div>
              <BarSparkline values={cpuSeries} />
            </div>
          ) : null}
        </DialogContent>
      </Dialog>

      <Dialog open={detail === "memory"} onOpenChange={(o) => !o && closeDetail()}>
        <DialogContent className="sm:max-w-md" showCloseButton>
          <DialogHeader>
            <DialogTitle>{t("pages.monitor.memory")}</DialogTitle>
            <DialogDescription>{t("pages.monitor.detailMemDesc")}</DialogDescription>
          </DialogHeader>
          <div className="flex items-center justify-between gap-2">
            <MeterBar ratio={memPct} className="min-w-0 flex-1" />
            <span
              className="shrink-0 font-mono text-[14px] font-semibold tabular-nums"
              style={{ color: loadColor(memPct) }}
            >
              {memPct == null ? "\u2014" : memPct.toFixed(1) + "%"}
            </span>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <StatCell label={t("pages.monitor.used")} value={fmtBytes(host?.memoryUsedBytes ?? null)} />
            <StatCell label={t("pages.monitor.total")} value={fmtBytes(host?.memoryTotalBytes ?? null)} />
            <StatCell label={t("pages.monitor.available")} value={fmtBytes(host?.memoryAvailableBytes ?? null)} />
            <StatCell
              label={t("pages.monitor.swap")}
              value={
                swapTotal
                  ? fmtBytes(host?.swapUsedBytes ?? null) + " / " + fmtBytes(swapTotal)
                  : "\u2014"
              }
            />
          </div>
          {memSeries.length >= 2 ? (
            <div>
              <div className="mb-1 text-[11px] text-[var(--t3)]">{t("pages.monitor.pressure")}</div>
              <BarSparkline values={memSeries} />
            </div>
          ) : null}
        </DialogContent>
      </Dialog>

      <Dialog open={detail === "disk"} onOpenChange={(o) => !o && closeDetail()}>
        <DialogContent className="sm:max-w-sm" showCloseButton>
          <DialogHeader>
            <DialogTitle>{t("pages.monitor.storage")}</DialogTitle>
            <DialogDescription>{t("pages.monitor.detailDiskDesc")}</DialogDescription>
          </DialogHeader>
          {host?.diskTotalBytes == null ? (
            <p className="text-[13px] text-[var(--t3)]">{t("pages.monitor.unavailableLong")}</p>
          ) : (
            <>
              <div className="flex items-center justify-between gap-2">
                <MeterBar ratio={diskPct} className="min-w-0 flex-1" />
                <span
                  className="shrink-0 font-mono text-[14px] font-semibold tabular-nums"
                  style={{ color: loadColor(diskPct) }}
                >
                  {diskPct == null ? "\u2014" : diskPct.toFixed(1) + "%"}
                </span>
              </div>
              <div className="grid grid-cols-2 gap-3">
                <StatCell label={t("pages.monitor.used")} value={fmtBytes(host.diskUsedBytes)} />
                <StatCell label={t("pages.monitor.total")} value={fmtBytes(host.diskTotalBytes)} />
              </div>
            </>
          )}
        </DialogContent>
      </Dialog>

      <Dialog open={detail === "temp"} onOpenChange={(o) => !o && closeDetail()}>
        <DialogContent className="sm:max-w-sm" showCloseButton>
          <DialogHeader>
            <DialogTitle>{t("pages.monitor.cpuTemp")}</DialogTitle>
            <DialogDescription>
              {tempSupported
                ? t(`statusBar.tempModes.${tempMode}.hint`)
                : t("statusBar.tempUnsupportedHint")}
            </DialogDescription>
          </DialogHeader>
          <div
            className="text-center font-mono text-[32px] font-semibold tabular-nums"
            style={{ color: tempColor(temp) }}
          >
            {tempDisplay}
          </div>
          {!tempSupported ? (
            <p className="text-center text-[12px] text-[var(--t3)]">
              {t("pages.monitor.tempUnsupportedHint")}
            </p>
          ) : tempMode === "off" && temp == null ? (
            <p className="text-center text-[12px] text-[var(--t3)]">{t("pages.monitor.tempOffHint")}</p>
          ) : temp == null ? (
            <p className="text-center text-[12px] text-[var(--t3)]">{t("pages.monitor.tempWaiting")}</p>
          ) : null}
          <div className="flex justify-center">
            <div className="flex overflow-hidden rounded-[var(--r-sm)] border border-[var(--line)]">
              {TEMP_MODES.map((mode) => (
                <button
                  key={mode}
                  type="button"
                  disabled={!tempSupported && mode !== "off"}
                  onClick={() => pickTempMode(mode)}
                  className={cn(
                    "px-2.5 py-1 text-[11px] transition-colors",
                    "border-l border-[var(--line)] first:border-l-0",
                    tempMode === mode
                      ? "bg-[var(--st-accent-tint)] font-semibold text-[var(--st-accent-hover)]"
                      : "text-[var(--t2)] hover:bg-[var(--surface-2)]",
                    !tempSupported && mode !== "off" && "cursor-not-allowed opacity-40",
                  )}
                >
                  {t(`statusBar.tempModes.${mode}.label`)}
                </button>
              ))}
            </div>
          </div>
        </DialogContent>
      </Dialog>

      <Dialog open={detail === "network"} onOpenChange={(o) => !o && closeDetail()}>
        <DialogContent className="sm:max-w-sm" showCloseButton>
          <DialogHeader>
            <DialogTitle>{t("pages.monitor.network")}</DialogTitle>
            <DialogDescription>{t("pages.monitor.detailNetDesc")}</DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-3">
            <StatCell label={t("pages.monitor.localIp")} value={host?.netLocalIp ?? "\u2014"} />
            <StatCell label={t("pages.monitor.upload")} value={fmtRate(host?.netUploadBps ?? null)} />
            <StatCell label={t("pages.monitor.download")} value={fmtRate(host?.netDownloadBps ?? null)} />
            {host?.netUploadBps == null && host?.netDownloadBps == null ? (
              <p className="text-[12px] text-[var(--t3)]">{t("pages.monitor.netWaiting")}</p>
            ) : null}
          </div>
        </DialogContent>
      </Dialog>

      <Dialog open={detail === "sysinfo"} onOpenChange={(o) => !o && closeDetail()}>
        <DialogContent className="sm:max-w-md" showCloseButton>
          <DialogHeader>
            <DialogTitle>{t("pages.monitor.sysInfo")}</DialogTitle>
            <DialogDescription>{t("pages.monitor.detailSysDesc")}</DialogDescription>
          </DialogHeader>
          <SystemInfoBody />
        </DialogContent>
      </Dialog>

      <Dialog open={detail === "service"} onOpenChange={(o) => !o && closeDetail()}>
        <DialogContent className="sm:max-w-lg" showCloseButton>
          <DialogHeader>
            <DialogTitle>
              {serviceFocus
                ? t("pages.monitor.serviceDetailTitle", { id: serviceFocus.id })
                : t("pages.monitor.services")}
            </DialogTitle>
            <DialogDescription>{t("pages.monitor.detailSvcDesc")}</DialogDescription>
          </DialogHeader>
          <ServiceAttribBody focusId={serviceFocus?.id ?? null} />
        </DialogContent>
      </Dialog>

      <Dialog open={detail === "doctor"} onOpenChange={(o) => !o && closeDetail()}>
        <DialogContent className="sm:max-w-2xl" showCloseButton>
          <DialogHeader>
            <DialogTitle>{t("pages.monitor.doctorTitle")}</DialogTitle>
            <DialogDescription>{t("pages.monitor.doctorHint")}</DialogDescription>
          </DialogHeader>
          <DoctorBody />
        </DialogContent>
      </Dialog>
    </div>
  );
}

function HeroMetricCard(props: {
  icon: React.ReactNode;
  title: string;
  value: string;
  sub?: string;
  color: string;
  meter: number | null;
  meterColor?: string;
  spark?: number[];
  onClick: () => void;
  hint: string;
  muted?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={props.onClick}
      title={props.hint}
      className={cn(
        "flex flex-col gap-2.5 rounded-[var(--r-md)] border border-[var(--line)] bg-[var(--surface)] p-4 text-left",
        "shadow-[var(--shadow-1)] transition-colors duration-150",
        "hover:border-[var(--line-strong)] hover:bg-[var(--surface-2)]/50",
        "focus-visible:outline-2 focus-visible:outline-[var(--st-accent)] focus-visible:outline-offset-2",
      )}
    >
      <div className="flex items-center gap-2 text-[var(--t3)]">
        <span className="flex size-7 items-center justify-center rounded-[var(--r-sm)] bg-[var(--surface-2)] text-[var(--t2)]">
          {props.icon}
        </span>
        <span className="text-[12px] font-semibold text-[var(--t2)]">{props.title}</span>
      </div>
      <div
        className={cn(
          "font-mono text-[22px] font-semibold tabular-nums leading-none",
          props.muted && "text-[var(--t3)]",
        )}
        style={props.muted ? undefined : { color: props.color }}
      >
        {props.value}
      </div>
      {props.sub ? (
        <div className="truncate font-mono text-[11px] text-[var(--t3)]">{props.sub}</div>
      ) : null}
      <MeterBar ratio={props.meter} color={props.meterColor} />
      {props.spark && props.spark.length >= 2 ? (
        <BarSparkline values={props.spark} />
      ) : (
        <div className="h-8" />
      )}
    </button>
  );
}

type AttribRow = { svc: ServiceRuntimeView; metric: ServiceMetrics | null };

function useAttribRows(): AttribRow[] {
  const rt = useRuntime();
  return Object.values(rt.state.services)
    .map((svc) => ({ svc, metric: rt.state.metrics[svc.id] ?? null }))
    .sort((a, b) => {
      const am = a.metric?.memory_bytes;
      const bm = b.metric?.memory_bytes;
      if (am != null && bm != null) return bm - am;
      if (am != null) return -1;
      if (bm != null) return 1;
      return 0;
    });
}

function ServiceAttribCard(props: {
  onOpenService: (svc: ServiceRuntimeView) => void;
  onOpenList: () => void;
}) {
  const { t } = useTranslation();
  const rt = useRuntime();
  const rows = useAttribRows();
  const preview = rows.slice(0, 5);

  return (
    <PageCard
      title={t("pages.monitor.services")}
      className="xl:col-span-4"
      action={
        <button
          type="button"
          onClick={props.onOpenList}
          className="text-[11px] font-medium text-[var(--st-accent)] hover:underline"
        >
          {t("pages.monitor.viewAll")}
        </button>
      }
    >
      {rt.state.snapshot == null ? (
        <p className="text-[11px] text-[var(--t3)]">{t("pages.monitor.servicesNoWs")}</p>
      ) : rows.length === 0 ? (
        <p className="text-[11px] text-[var(--t3)]">{t("pages.monitor.servicesEmpty")}</p>
      ) : (
        <div className="flex flex-col gap-1">
          <div className="grid grid-cols-[minmax(0,1fr)_5rem_5rem_3.5rem] gap-2 pb-0.5 text-[11px] text-[var(--t3)]">
            <div>{t("pages.monitor.servicesColService")}</div>
            <div className="text-right">CPU</div>
            <div className="text-right">{t("pages.monitor.servicesColMemory")}</div>
            <div className="text-right">{t("pages.monitor.servicesColProc")}</div>
          </div>
          {preview.map((r) => (
            <button
              key={r.svc.id}
              type="button"
              onClick={() => props.onOpenService(r.svc)}
              className="grid grid-cols-[minmax(0,1fr)_5rem_5rem_3.5rem] items-center gap-2 rounded-[var(--r-sm)] px-1 py-0.5 text-left hover:bg-[var(--surface-2)]"
            >
              <div className="flex min-w-0 items-center gap-2">
                <span className="truncate font-mono text-[12px] text-[var(--t1)]">{r.svc.id}</span>
                <StatusChip state={r.svc.state} className="shrink-0" />
              </div>
              <span className="text-right font-mono text-[12px] tabular-nums text-[var(--t2)]">
                {r.metric?.cpu_percent == null ? "\u2014" : r.metric.cpu_percent.toFixed(1) + "%"}
              </span>
              <span className="text-right font-mono text-[12px] tabular-nums text-[var(--t2)]">
                {fmtBytes(r.metric?.memory_bytes ?? null)}
              </span>
              <span className="text-right font-mono text-[12px] tabular-nums text-[var(--t2)]">
                {r.metric?.process_count ?? "\u2014"}
              </span>
            </button>
          ))}
          {rows.length > preview.length ? (
            <button
              type="button"
              onClick={props.onOpenList}
              className="mt-1 text-left text-[11px] text-[var(--t3)] hover:text-[var(--st-accent)]"
            >
              {t("pages.monitor.moreServices", { n: rows.length - preview.length })}
            </button>
          ) : null}
        </div>
      )}
    </PageCard>
  );
}

function ServiceAttribBody({ focusId }: { focusId: string | null }) {
  const { t } = useTranslation();
  const rt = useRuntime();
  const rows = useAttribRows();
  const focused = focusId ? rows.find((r) => r.svc.id === focusId) : null;

  if (rt.state.snapshot == null) {
    return <p className="text-[13px] text-[var(--t3)]">{t("pages.monitor.servicesNoWs")}</p>;
  }
  if (rows.length === 0) {
    return <p className="text-[13px] text-[var(--t3)]">{t("pages.monitor.servicesEmpty")}</p>;
  }

  return (
    <div className="flex max-h-[60vh] flex-col gap-3 overflow-auto">
      {focused ? (
        <div className="rounded-[var(--r-sm)] border border-[var(--line)] bg-[var(--surface-2)]/50 p-3">
          <div className="flex items-center gap-2">
            <span className="font-mono text-[13px] font-semibold text-[var(--t1)]">{focused.svc.id}</span>
            <StatusChip state={focused.svc.state} />
          </div>
          <div className="mt-2 grid grid-cols-3 gap-2">
            <StatCell
              label="CPU"
              value={
                focused.metric?.cpu_percent == null
                  ? "\u2014"
                  : focused.metric.cpu_percent.toFixed(1) + "%"
              }
            />
            <StatCell
              label={t("pages.monitor.servicesColMemory")}
              value={fmtBytes(focused.metric?.memory_bytes ?? null)}
            />
            <StatCell
              label={t("pages.monitor.servicesColProc")}
              value={focused.metric?.process_count ?? "\u2014"}
            />
          </div>
          {focused.metric == null ? (
            <p className="mt-2 text-[11px] text-[var(--t3)]">
              {focused.svc.managed === false
                ? t("pages.run.metricsEmptyHint", { id: focused.svc.id })
                : focused.svc.kind === "compose"
                  ? t("pages.run.metricsComposeHint")
                  : t("pages.monitor.metricsUnavailable")}
            </p>
          ) : null}
        </div>
      ) : null}
      <div className="flex flex-col gap-1">
        <div className="grid grid-cols-[minmax(0,1fr)_5rem_5rem_3.5rem] gap-2 pb-0.5 text-[11px] text-[var(--t3)]">
          <div>{t("pages.monitor.servicesColService")}</div>
          <div className="text-right">CPU</div>
          <div className="text-right">{t("pages.monitor.servicesColMemory")}</div>
          <div className="text-right">{t("pages.monitor.servicesColProc")}</div>
        </div>
        {rows.map((r) => (
          <div
            key={r.svc.id}
            className={cn(
              "grid grid-cols-[minmax(0,1fr)_5rem_5rem_3.5rem] items-center gap-2 rounded-[var(--r-sm)] px-1 py-0.5",
              focusId === r.svc.id && "bg-[var(--st-accent-tint)]",
            )}
          >
            <div className="flex min-w-0 items-center gap-2">
              <span className="truncate font-mono text-[12px] text-[var(--t1)]">{r.svc.id}</span>
              <StatusChip state={r.svc.state} className="shrink-0" />
            </div>
            <span className="text-right font-mono text-[12px] tabular-nums text-[var(--t2)]">
              {r.metric?.cpu_percent == null ? "\u2014" : r.metric.cpu_percent.toFixed(1) + "%"}
            </span>
            <span className="text-right font-mono text-[12px] tabular-nums text-[var(--t2)]">
              {fmtBytes(r.metric?.memory_bytes ?? null)}
            </span>
            <span className="text-right font-mono text-[12px] tabular-nums text-[var(--t2)]">
              {r.metric?.process_count ?? "\u2014"}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function SysInfoRow(props: { label: string; value: string }) {
  return (
    <div className="flex min-w-0 items-baseline justify-between gap-2">
      <span className="shrink-0 text-[11px] text-[var(--t3)]">{props.label}</span>
      <span className="truncate font-mono text-[12px] tabular-nums text-[var(--t2)]" title={props.value}>
        {props.value}
      </span>
    </div>
  );
}

function useSystemInfo() {
  const [info, setInfo] = useState<SystemInfo | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let alive = true;
    void apiSystemInfo()
      .then((v) => {
        if (alive) setInfo(v);
      })
      .catch(() => {
        if (alive) setFailed(true);
      });
    return () => {
      alive = false;
    };
  }, []);
  return { info, failed };
}

function SystemInfoBody() {
  const { t } = useTranslation();
  const { info, failed } = useSystemInfo();

  if (failed && info == null) {
    return <p className="text-[13px] text-[var(--t3)]">{t("pages.monitor.unavailableLong")}</p>;
  }
  if (info == null) {
    return <p className="text-[13px] text-[var(--t3)]">{t("common.loading")}</p>;
  }

  const cpuValue =
    info.cpuLogicalCores == null
      ? (info.arch ?? "\u2014")
      : [
          info.arch,
          info.cpuPhysicalCores == null
            ? t("pages.monitor.sysInfoCoresLogical", { n: info.cpuLogicalCores })
            : t("pages.monitor.sysInfoCores", {
                logical: info.cpuLogicalCores,
                physical: info.cpuPhysicalCores,
              }),
        ].join(" · ");
  const osValue = [info.osName, info.osVersion].filter(Boolean).join(" · ") || "\u2014";
  const platformValue = info.platform
    ? info.platform.charAt(0).toUpperCase() + info.platform.slice(1)
    : "\u2014";

  return (
    <div className="flex flex-col gap-1.5">
      <SysInfoRow label={t("pages.monitor.sysInfoPlatform")} value={platformValue} />
      <SysInfoRow label={t("pages.monitor.sysInfoOs")} value={osValue} />
      <SysInfoRow label={t("pages.monitor.sysInfoCpu")} value={cpuValue} />
      <SysInfoRow label={t("pages.monitor.sysInfoMemory")} value={fmtBytes(info.totalMemoryBytes ?? null)} />
      <SysInfoRow label={t("pages.monitor.sysInfoAppVersion")} value={info.appVersion ?? "\u2014"} />
    </div>
  );
}

function SystemInfoCard({ onOpen }: { onOpen: () => void }) {
  const { t } = useTranslation();
  const { info } = useSystemInfo();
  const platformValue = info?.platform
    ? info.platform.charAt(0).toUpperCase() + info.platform.slice(1)
    : "\u2014";

  return (
    <PageCard
      title={t("pages.monitor.sysInfo")}
      className="xl:col-span-2"
      onClick={onOpen}
      action={<Info className="size-3.5 text-[var(--t3)]" aria-hidden />}
    >
      {info == null ? (
        <p className="text-[11px] text-[var(--t3)]">{t("common.loading")}</p>
      ) : (
        <div className="flex flex-col gap-1.5">
          <SysInfoRow label={t("pages.monitor.sysInfoPlatform")} value={platformValue} />
          <SysInfoRow
            label={t("pages.monitor.sysInfoOs")}
            value={[info.osName, info.osVersion].filter(Boolean).join(" · ") || "\u2014"}
          />
          <SysInfoRow label={t("pages.monitor.sysInfoAppVersion")} value={info.appVersion ?? "\u2014"} />
        </div>
      )}
    </PageCard>
  );
}

type DoctorResult = {
  info: SystemInfo;
  tools: ToolchainProbeOut;
  docker: DockerProbe;
  atMs: number;
};

type DoctorProbe = { found: boolean; version: string | null; path: string | null };

function DoctorRow(props: { name: string; probe?: DoctorProbe }) {
  const { t } = useTranslation();
  return (
    <div className="flex min-w-0 items-baseline justify-between gap-2 text-[12px]">
      <span className="shrink-0 text-[var(--t2)]">{props.name}</span>
      {props.probe == null ? (
        <span className="font-mono text-[var(--t3)]">{"\u2014"}</span>
      ) : props.probe.found ? (
        <span className="truncate font-mono text-[var(--t1)]" title={props.probe.path ?? undefined}>
          {props.probe.version ?? "?"}
        </span>
      ) : (
        <span className="font-mono text-[var(--t3)]">{t("pages.monitor.doctorNotFound")}</span>
      )}
    </div>
  );
}
/** Markdown 报告组装：与 CLI `supertask doctor` 同口径（工具名/版本/路径），附系统信息。 */
function buildDoctorReport(t: (key: string) => string, r: DoctorResult): string {
  const nf = t("pages.monitor.doctorNotFound");
  const line = (name: string, probe?: DoctorProbe) => {
    if (probe == null) return `- ${name}: —`;
    if (!probe.found) return `- ${name}: ${nf}`;
    return `- ${name}: ${probe.version ?? "?"}（${probe.path ?? ""}）`;
  };
  const d = r.docker;
  return [
    `# ${t("pages.monitor.doctorReportTitle")}`,
    "",
    `- ${t("pages.monitor.doctorReportGenerated")}: ${new Date(r.atMs).toLocaleString()}`,
    `- ${t("pages.monitor.sysInfoAppVersion")}: ${r.info.appVersion}`,
    `- ${t("pages.monitor.sysInfoPlatform")}: ${r.info.platform} · ${r.info.osName ?? "—"} ${r.info.osVersion ?? ""} · ${r.info.arch}`,
    "",
    `## ${t("pages.monitor.doctorSectionTools")}`,
    line("java", r.tools.java),
    line("maven", r.tools.maven),
    line("gradle", r.tools.gradle),
    line("node", r.tools.node),
    line("npm", r.tools.npm),
    line("pnpm", r.tools.pnpm),
    line("yarn", r.tools.yarn),
    line("bun", r.tools.bun),
    line("python", r.tools.python),
    line("go", r.tools.go),
    "",
    `## ${t("pages.monitor.doctorSectionGateway")}`,
    line("nginx", r.tools.gateway?.nginx),
    line("caddy", r.tools.gateway?.caddy),
    line("apache", r.tools.gateway?.apache),
    "",
    "## Docker",
    d.found ? `- docker: ${d.version ?? "?"}` : `- docker: ${nf}`,
    d.found
      ? `- compose: ${d.compose_version ?? t("pages.monitor.doctorComposeMissing")}`
      : "- compose: —",
    `- ${t("pages.monitor.doctorDaemon")}: ${d.found ? (d.running ? t("pages.monitor.doctorDaemonRunning") : t("pages.monitor.doctorDaemonStopped")) : "—"}`,
    "",
  ].join("\n");
}

function doctorStamp(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}`;
}
function useDoctor() {
  const [result, setResult] = useState<DoctorResult | null>(null);
  const [probing, setProbing] = useState(false);
  const [saved, setSaved] = useState<"saved" | "failed" | null>(null);
  const { t } = useTranslation();

  const run = async () => {
    setProbing(true);
    setSaved(null);
    try {
      const [info, tools, docker] = await Promise.all([
        apiSystemInfo(),
        apiToolchainProbe(true),
        apiDockerProbe(true),
      ]);
      setResult({ info, tools, docker, atMs: Date.now() });
    } catch {
      // Ambient: keep last result.
    } finally {
      setProbing(false);
    }
  };

  const exportReport = async () => {
    if (!result) return;
    const out = await downloadTextFile(
      `supertask-doctor-${doctorStamp(result.atMs)}.md`,
      buildDoctorReport(t, result),
    );
    setSaved(out === "cancelled" ? null : out);
  };

  return { result, probing, saved, run, exportReport };
}

function DoctorCard({ onOpen }: { onOpen: () => void }) {
  const { t } = useTranslation();
  return (
    <PageCard
      title={t("pages.monitor.doctorTitle")}
      className="xl:col-span-6"
      action={<Stethoscope className="size-3.5 text-[var(--t3)]" aria-hidden />}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="min-w-0 flex-1 text-[11px] text-[var(--t3)]">{t("pages.monitor.doctorHint")}</p>
        <Button variant="soft" size="sm" onClick={onOpen} className="gap-1">
          <Activity className="size-3.5" />
          {t("pages.monitor.doctorOpen")}
        </Button>
      </div>
    </PageCard>
  );
}

function DoctorBody() {
  const { t } = useTranslation();
  const { result, probing, saved, run, exportReport } = useDoctor();
  const btn =
    "h-7 cursor-pointer rounded-[var(--r-sm)] border border-[var(--line)] px-2.5 text-[11px] font-medium text-[var(--t1)] transition-colors duration-150 hover:bg-[var(--surface-2)] disabled:cursor-default disabled:opacity-50";

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="min-w-0 flex-1 truncate text-[11px] text-[var(--t3)]">
          {result
            ? `${t("pages.monitor.doctorAt")} ${new Date(result.atMs).toLocaleTimeString()} · v${result.info.appVersion} · ${result.info.platform}` +
              (saved == null
                ? ""
                : saved === "saved"
                  ? ` · ${t("pages.monitor.doctorExported")}`
                  : ` · ${t("pages.monitor.doctorExportFailed")}`)
            : t("pages.monitor.doctorHint")}
        </p>
        <div className="flex shrink-0 gap-2">
          <button onClick={() => void run()} disabled={probing} className={btn}>
            {probing ? t("pages.monitor.doctorProbing") : t("pages.monitor.doctorRun")}
          </button>
          <button
            onClick={() => void exportReport()}
            disabled={probing || result == null}
            className={btn}
          >
            {t("pages.monitor.doctorExport")}
          </button>
        </div>
      </div>
      {result ? (
        <div className="grid gap-x-8 gap-y-3 md:grid-cols-3">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="text-[11px] font-semibold text-[var(--t2)]">
              {t("pages.monitor.doctorSectionTools")}
            </div>
            <DoctorRow name="java" probe={result.tools.java} />
            <DoctorRow name="maven" probe={result.tools.maven} />
            <DoctorRow name="gradle" probe={result.tools.gradle} />
            <DoctorRow name="node" probe={result.tools.node} />
            <DoctorRow name="npm" probe={result.tools.npm} />
            <DoctorRow name="pnpm" probe={result.tools.pnpm} />
            <DoctorRow name="yarn" probe={result.tools.yarn} />
            <DoctorRow name="bun" probe={result.tools.bun} />
            <DoctorRow name="python" probe={result.tools.python} />
            <DoctorRow name="go" probe={result.tools.go} />
          </div>
          <div className="flex min-w-0 flex-col gap-1">
            <div className="text-[11px] font-semibold text-[var(--t2)]">
              {t("pages.monitor.doctorSectionGateway")}
            </div>
            <DoctorRow name="nginx" probe={result.tools.gateway?.nginx} />
            <DoctorRow name="caddy" probe={result.tools.gateway?.caddy} />
            <DoctorRow name="apache" probe={result.tools.gateway?.apache} />
          </div>
          <div className="flex min-w-0 flex-col gap-1">
            <div className="text-[11px] font-semibold text-[var(--t2)]">Docker</div>
            <DoctorRow
              name="docker"
              probe={
                result.docker.found
                  ? { found: true, version: result.docker.version, path: null }
                  : { found: false, version: null, path: null }
              }
            />
            <div className="flex min-w-0 items-baseline justify-between gap-2 text-[12px]">
              <span className="shrink-0 text-[var(--t2)]">compose</span>
              <span className="truncate font-mono text-[var(--t1)]">
                {result.docker.found
                  ? (result.docker.compose_version ?? t("pages.monitor.doctorComposeMissing"))
                  : "\u2014"}
              </span>
            </div>
            <div className="flex min-w-0 items-baseline justify-between gap-2 text-[12px]">
              <span className="shrink-0 text-[var(--t2)]">{t("pages.monitor.doctorDaemon")}</span>
              <span className="font-mono text-[var(--t1)]">
                {result.docker.found
                  ? result.docker.running
                    ? t("pages.monitor.doctorDaemonRunning")
                    : t("pages.monitor.doctorDaemonStopped")
                  : "\u2014"}
              </span>
            </div>
          </div>
        </div>
      ) : null}

      {!result ? (
        <div className="flex h-24 items-center justify-center rounded-[var(--r-sm)] border border-dashed border-[var(--line)] text-[12px] text-[var(--t3)]">
          {t("pages.monitor.doctorEmpty")}
        </div>
      ) : null}
    </div>
  );
}
