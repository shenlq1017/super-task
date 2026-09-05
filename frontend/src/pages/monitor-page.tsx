import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Card } from "@/components/ui/card";
import { cn } from "@/lib/utils";
import { apiSystemMetrics } from "../ipc/api";
import type { HostMetrics } from "../ipc/protocol";
import { fmtBytes, fmtRate, loadColor } from "@/lib/metrics";
import { useTempMode } from "@/lib/temp-mode";

const POLL_MS = 1000;
/** ~2 min of samples, rolling while the page stays open. Not persisted. */
const HISTORY = 120;

/** Page-level sampling: 1 Hz with the shared temp-mode preference, so this
 *  page never fights the status bar over the backend sampler state. */
function useHostMetrics() {
  const tempMode = useTempMode();
  const [host, setHost] = useState<HostMetrics | null>(null);
  const [cpuHistory, setCpuHistory] = useState<number[]>([]);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    const tick = async () => {
      try {
        const m = await apiSystemMetrics(tempMode);
        if (!alive.current) return;
        setHost(m);
        const cpu = m.cpuPercent;
        if (cpu != null) {
          setCpuHistory((prev) => prev.concat([cpu]).slice(-HISTORY));
        }
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

  return { host, cpuHistory };
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
 * 系统监控：整机 CPU / 内存 / 存储 / 网络的实时面板。
 * 数据来自 `system.metrics`（1 Hz 轮询）；历史曲线仅本页滚动累积，不持久化。
 */
export function MonitorPage() {
  const { t } = useTranslation();
  const { host, cpuHistory } = useHostMetrics();

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

            {/* CPU 使用率历史 */}
            <PageCard title={t("pages.monitor.cpuHistory")} className="xl:col-span-3">
              <div className="flex min-h-[88px] flex-1 items-end">
                {cpuHistory.length < 2 ? (
                  <div className="flex h-[88px] w-full items-center justify-center text-[11px] text-[var(--t3)]">
                    {t("pages.monitor.collecting")}
                  </div>
                ) : (
                  <AreaChart values={cpuHistory} />
                )}
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
          </div>
        </div>
      </div>
    </div>
  );
}
