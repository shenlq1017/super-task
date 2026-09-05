import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Card } from "@/components/ui/card";
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
import { fmtBytes, fmtRate, loadColor } from "@/lib/metrics";
import { useTempMode } from "@/lib/temp-mode";
import { recordHostMetrics, useMetricsHistory } from "@/lib/metrics-history";
import { downloadTextFile } from "@/lib/download-text";

const POLL_MS = 1000;

/** Page-level sampling: 1 Hz with the shared temp-mode preference, so this
 *  page never fights the status bar over the backend sampler state. Each
 *  sample also feeds the shared cross-page history store (metrics-history). */
function useHostMetrics() {
  const tempMode = useTempMode();
  const [host, setHost] = useState<HostMetrics | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    const tick = async () => {
      try {
        const m = await apiSystemMetrics(tempMode);
        if (!alive.current) return;
        setHost(m);
        recordHostMetrics(m);
      } catch {
        // Ambient page: a failed sample keeps the previous reading.
      }
    };
    void tick();
    const id = window.setInterval(() => void tick(), POLL_MS);
    return () => {
      alive.current = false;
      window.clearInterval(id);
    };
  }, [tempMode]);

  return { host };
}

function PageCard(props: { title: string; children: React.ReactNode; className?: string }) {
  return (
    <Card className={cn("flex flex-col gap-3 p-4", props.className)}>
      <h3 className="text-[13px] font-semibold text-[var(--t1)]">{props.title}</h3>
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

function MeterBar(props: { ratio: number | null; className?: string }) {
  return (
    <div className={cn("h-2.5 overflow-hidden rounded-full bg-[var(--surface-2)]", props.className)}>
      <div
        className="h-full rounded-full transition-[width] duration-500"
        style={{ width: (props.ratio ?? 0) + "%", background: loadColor(props.ratio) }}
      />
    </div>
  );
}

/** Semicircular load gauge: full green→amber→red arc with a needle, like the
 *  classic monitor widget. All colors come from theme tokens. */
function CpuGauge({ value }: { value: number | null }) {
  const cx = 100;
  const cy = 96;
  const r = 76;
  const arc = `M ${cx - r} ${cy} A ${r} ${r} 0 0 1 ${cx + r} ${cy}`;
  // 0% → needle points left (π), 100% → right (0).
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

/** Rolling usage area chart, hand-drawn SVG like the status bar sparkline. */
function AreaChart({ values }: { values: number[] }) {
  const W = 300;
  const H = 80;
  if (values.length < 2) return null;
  const pts = values.map((v, i) => {
    const x = (i / (values.length - 1)) * W;
    const y = H - (Math.min(100, Math.max(0, v)) / 100) * (H - 4) - 2;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  });
  return (
    <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" className="h-[88px] w-full" aria-hidden>
      <path d={`M${pts.join(" L")} L${W},${H} L0,${H} Z`} fill="var(--st-ok)" opacity={0.22} />
      <path
        d={`M${pts.join(" L")}`}
        fill="none"
        stroke="var(--st-ok)"
        strokeWidth={1.75}
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}

/**
 * 系统监控：整机 CPU / 内存 / 存储 / 网络的实时面板 + 按服务归因的资源占用。
 * 整机数据来自 `system.metrics`（1 Hz 轮询）；历史曲线仅本页滚动累积，不持久化。
 * 服务归因复用既有 per-service Job 采样（`metrics.snapshot` / st-runtime 载荷，
 * RuntimeProvider 在工作区打开期间全局订阅），本页只做展示与排序，不引入新采样。
 * 静态系统信息来自 `system.info`（一次拉取），与动态采样分列。
 * CPU / 内存压力历史进 metrics-history 环形缓冲：状态栏与监控页双馈源，
 * 跨页面存活、有界约 1 小时，不持久化。
 * 体检报告复用 `toolchain.probe` / `docker.probe` / `system.info` 既有探测，
 * 可导出 Markdown 随求助贴附带。
 */
export function MonitorPage() {
  const { t } = useTranslation();
  const { host } = useHostMetrics();
  const history = useMetricsHistory();
  const cpuSeries = history.map((s) => s.cpu).filter((v): v is number => v != null);
  const memSeries = history.map((s) => s.mem).filter((v): v is number => v != null);
  const windowMinutes =
    history.length >= 2
      ? Math.max(1, Math.round((history[history.length - 1].at - history[0].at) / 60000))
      : 0;

  const cpu = host?.cpuPercent ?? null;
  const memPct =
    host?.memoryUsedBytes != null && host?.memoryTotalBytes
      ? Math.min(100, Math.max(0, (host.memoryUsedBytes / host.memoryTotalBytes) * 100))
      : null;
  const diskPct =
    host?.diskUsedBytes != null && host?.diskTotalBytes
      ? Math.min(100, Math.max(0, (host.diskUsedBytes / host.diskTotalBytes) * 100))
      : null;
  const swapTotal = host?.swapTotalBytes ?? null;
  const split = [
    { label: t("pages.monitor.system"), v: host?.cpuSystemPercent ?? null },
    { label: t("pages.monitor.user"), v: host?.cpuUserPercent ?? null },
    { label: t("pages.monitor.nice"), v: host?.cpuNicePercent ?? null },
    { label: t("pages.monitor.idle"), v: host?.cpuIdlePercent ?? null },
  ];

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto flex max-w-5xl flex-col gap-4">
          <header>
            <h2 className="text-lg font-semibold text-[var(--t1)]">{t("pages.monitor.title")}</h2>
            <p className="mt-0.5 text-[0.8rem] text-[var(--t3)]">{t("pages.monitor.subtitle")}</p>
          </header>

          <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-6">
            {/* CPU 负载 */}
            <PageCard title={t("pages.monitor.cpuLoad")} className="xl:col-span-3">
              <div className="-mt-1">
                <CpuGauge value={cpu} />
                <div className="mt-1 text-center font-mono text-[26px] font-semibold tabular-nums text-[var(--t1)]">
                  {cpu == null ? "\u2014" : cpu.toFixed(1) + "%"}
                </div>
              </div>
              <div className="mt-1 grid grid-cols-4 gap-2">
                {split.map((s) => (
                  <StatCell
                    key={s.label}
                    label={s.label}
                    value={s.v == null ? "\u2014" : s.v.toFixed(1) + "%"}
                  />
                ))}
              </div>
            </PageCard>

            {/* 历史趋势：CPU + 内存压力（跨页面存活，应用关闭即清空） */}
            <PageCard title={t("pages.monitor.historyTitle")} className="xl:col-span-3">
              <div className="flex flex-col gap-2">
                <div>
                  <div className="text-[11px] text-[var(--t3)]">CPU</div>
                  <div className="flex items-end">
                    {cpuSeries.length < 2 ? (
                      <div className="flex h-[64px] w-full items-center justify-center text-[11px] text-[var(--t3)]">
                        {t("pages.monitor.collecting")}
                      </div>
                    ) : (
                      <AreaChart values={cpuSeries} />
                    )}
                  </div>
                </div>
                <div>
                  <div className="text-[11px] text-[var(--t3)]">{t("pages.monitor.pressure")}</div>
                  <div className="flex items-end">
                    {memSeries.length < 2 ? (
                      <div className="flex h-[64px] w-full items-center justify-center text-[11px] text-[var(--t3)]">
                        {t("pages.monitor.collecting")}
                      </div>
                    ) : (
                      <AreaChart values={memSeries} />
                    )}
                  </div>
                </div>
                {windowMinutes > 0 ? (
                  <p className="text-right text-[11px] text-[var(--t3)]">
                    {t("pages.monitor.historyWindow", { n: windowMinutes })}
                  </p>
                ) : null}
              </div>
            </PageCard>

            {/* 内存 */}
            <PageCard title={t("pages.monitor.memory")} className="xl:col-span-2">
              <div className="flex items-center justify-between gap-2">
                <MeterBar ratio={memPct} className="min-w-0 flex-1" />
                <span className="shrink-0 font-mono text-[12px] tabular-nums text-[var(--t2)]">
                  {host?.memoryTotalBytes
                    ? fmtBytes(host.memoryUsedBytes) + " / " + fmtBytes(host.memoryTotalBytes)
                    : "\u2014"}
                </span>
              </div>
              <div className="grid grid-cols-3 gap-2">
                <StatCell
                  label={t("pages.monitor.pressure")}
                  value={memPct == null ? "\u2014" : memPct.toFixed(1) + "%"}
                />
                <StatCell label={t("pages.monitor.available")} value={fmtBytes(host?.memoryAvailableBytes ?? null)} />
                <StatCell
                  label={t("pages.monitor.swap")}
                  value={swapTotal ? fmtBytes(host?.swapUsedBytes ?? null) : "\u2014"}
                />
              </div>
            </PageCard>

            {/* 存储 */}
            <PageCard title={t("pages.monitor.storage")} className="xl:col-span-2">
              <div className="flex items-center justify-between gap-2">
                <MeterBar ratio={diskPct} className="min-w-0 flex-1" />
                <span className="shrink-0 font-mono text-[12px] tabular-nums text-[var(--t2)]">
                  {host?.diskTotalBytes
                    ? fmtBytes(host.diskUsedBytes) + " / " + fmtBytes(host.diskTotalBytes)
                    : "\u2014"}
                </span>
              </div>
            </PageCard>

            {/* 网络 */}
            <PageCard title={t("pages.monitor.network")} className="xl:col-span-2">
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

            {/* 静态系统信息 */}
            <SystemInfoCard />

            {/* 按服务归因的资源占用 */}
            <ServiceAttribCard />

            {/* 一键体检报告（doctor 可视化导出） */}
            <DoctorCard />
          </div>
        </div>
      </div>
    </div>
  );
}

/** 一行归因：服务 id + 状态 + 该服务进程树的 CPU / 内存 / 进程数。 */
type AttribRow = { svc: ServiceRuntimeView; metric: ServiceMetrics | null };

function ServiceAttribCard() {
  const { t } = useTranslation();
  const rt = useRuntime();
  const rows: AttribRow[] = Object.values(rt.state.services)
    .map((svc) => ({ svc, metric: rt.state.metrics[svc.id] ?? null }))
    // 内存降序（「哪个服务在吃内存」一眼可答）；无指标的（已停止 / compose / 外部纳管）沉底，
    // 组内保持快照顺序。null 显示「—」，不伪造 0。
    .sort((a, b) => {
      const am = a.metric?.memory_bytes;
      const bm = b.metric?.memory_bytes;
      if (am != null && bm != null) return bm - am;
      if (am != null) return -1;
      if (bm != null) return 1;
      return 0;
    });

  const rowTitle = (r: AttribRow): string | undefined => {
    if (r.metric != null) return undefined;
    if (r.svc.managed === false) return t("pages.run.metricsEmptyHint", { id: r.svc.id });
    if (r.svc.kind === "compose") return t("pages.run.metricsComposeHint");
    return undefined;
  };

  return (
    <PageCard title={t("pages.monitor.services")} className="xl:col-span-4">
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
          {rows.map((r) => (
            <div
              key={r.svc.id}
              title={rowTitle(r)}
              className="grid grid-cols-[minmax(0,1fr)_5rem_5rem_3.5rem] items-center gap-2"
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
      )}
    </PageCard>
  );
}

/** 系统信息一行：左侧标签、右侧等宽值（超长截断并以 title 兜底）。 */
function SysInfoRow(props: { label: string; value: string }) {
  return (
    <div className="flex min-w-0 items-baseline justify-between gap-2">
      <span className="shrink-0 text-[11px] text-[var(--t3)]">{props.label}</span>
      <span
        className="truncate font-mono text-[12px] tabular-nums text-[var(--t2)]"
        title={props.value}
      >
        {props.value}
      </span>
    </div>
  );
}

/** 静态系统信息：一次拉取、不轮询。与整机动态采样（system.metrics）分列；
 *  取不到的字段显示「—」，不伪造为 0。 */
function SystemInfoCard() {
  const { t } = useTranslation();
  const [info, setInfo] = useState<SystemInfo | null>(null);

  useEffect(() => {
    let alive = true;
    void apiSystemInfo()
      .then((v) => {
        if (alive) setInfo(v);
      })
      .catch(() => {
        // Ambient page: 环境不可用时留空展示「—」。
      });
    return () => {
      alive = false;
    };
  }, []);

  const cpuValue =
    info?.cpuLogicalCores == null
      ? (info?.arch ?? "\u2014")
      : [
          info.arch,
          info.cpuPhysicalCores == null
            ? t("pages.monitor.sysInfoCoresLogical", { n: info.cpuLogicalCores })
            : t("pages.monitor.sysInfoCores", {
                logical: info.cpuLogicalCores,
                physical: info.cpuPhysicalCores,
              }),
        ].join(" · ");
  const osValue =
    info == null ? "\u2014" : [info.osName, info.osVersion].filter(Boolean).join(" · ") || "\u2014";
  const platformValue = info?.platform
    ? info.platform.charAt(0).toUpperCase() + info.platform.slice(1)
    : "\u2014";

  return (
    <PageCard title={t("pages.monitor.sysInfo")} className="xl:col-span-2">
      <div className="flex flex-col gap-1.5">
        <SysInfoRow label={t("pages.monitor.sysInfoPlatform")} value={platformValue} />
        <SysInfoRow label={t("pages.monitor.sysInfoOs")} value={osValue} />
        <SysInfoRow label={t("pages.monitor.sysInfoCpu")} value={cpuValue} />
        <SysInfoRow
          label={t("pages.monitor.sysInfoMemory")}
          value={fmtBytes(info?.totalMemoryBytes ?? null)}
        />
        <SysInfoRow label={t("pages.monitor.sysInfoAppVersion")} value={info?.appVersion ?? "\u2014"} />
      </div>
    </PageCard>
  );
}

/** 体检结果：一次探测的系统信息 + 工具链（含网关引擎）+ Docker。 */
type DoctorResult = {
  info: SystemInfo;
  tools: ToolchainProbeOut;
  docker: DockerProbe;
  atMs: number;
};

type DoctorProbe = { found: boolean; version: string | null; path: string | null };

/** 体检一行：工具名 + 版本（悬浮显示路径）；未找到灰显，缺省字段显示「—」。 */
function DoctorRow(props: { name: string; probe?: DoctorProbe }) {
  const { t } = useTranslation();
  return (
    <div className="flex min-w-0 items-baseline justify-between gap-2 text-[12px]">
      <span className="shrink-0 text-[var(--t2)]">{props.name}</span>
      {props.probe == null ? (
        <span className="font-mono text-[var(--t3)]">{"\u2014"}</span>
      ) : props.probe.found ? (
        <span
          className="truncate font-mono text-[var(--t1)]"
          title={props.probe.path ?? undefined}
        >
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

/** 一键体检：工具链 + 网关引擎 + Docker 三节只读探测（复用既有 IPC，零新采样面），
 *  可导出 Markdown 报告随求助贴附带。口径与 CLI `supertask doctor` 一致：含工具路径，
 *  无环境变量值/密钥/IP。探测失败保留上次结果。 */
function DoctorCard() {
  const { t } = useTranslation();
  const [result, setResult] = useState<DoctorResult | null>(null);
  const [probing, setProbing] = useState(false);
  const [saved, setSaved] = useState<"saved" | "failed" | null>(null);

  const run = async () => {
    setProbing(true);
    setSaved(null);
    try {
      const [info, tools, docker] = await Promise.all([
        apiSystemInfo(),
        apiToolchainProbe(true), // 体检要新鲜探测：强制刷新会话缓存
        apiDockerProbe(true),
      ]);
      setResult({ info, tools, docker, atMs: Date.now() });
    } catch {
      // Ambient card: 失败保留上次结果。
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

  const btn =
    "h-7 cursor-pointer rounded-[var(--r-sm)] border border-[var(--line)] px-2.5 text-[11px] font-medium text-[var(--t1)] transition-colors duration-150 hover:bg-[var(--surface-2)] disabled:cursor-default disabled:opacity-50";

  return (
    <PageCard title={t("pages.monitor.doctorTitle")} className="xl:col-span-6">
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
    </PageCard>
  );
}
