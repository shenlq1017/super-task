import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronUp, Cpu, HardDrive, MemoryStick, Thermometer } from "lucide-react";
import { cn } from "@/lib/utils";
import { apiSystemMetrics } from "../ipc/api";
import { TEMP_MODES, type HostMetrics, type TempMode } from "../ipc/protocol";

const POLL_MS = 3000;
/** Fast temperature mode polls harder: the backend sampler is already resident,
 *  so the only extra cost here is one cheap IPC round trip. */
const POLL_MS_FAST_TEMP = 1200;
const ROTATE_MS = 2400;
const HISTORY = 24;
const TEMP_MODE_KEY = "supertask.statusBar.tempMode";

function loadTempMode(): TempMode {
  try {
    const raw = window.localStorage.getItem(TEMP_MODE_KEY);
    if (raw && (TEMP_MODES as readonly string[]).includes(raw)) return raw as TempMode;
  } catch {
    // Private mode / disabled storage: fall back to the default.
  }
  return "auto";
}

function fmtBytes(n: number | null | undefined): string {
  if (n == null) return "\u2014";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return v >= 100 || i <= 1
    ? Math.round(v) + " " + units[i]
    : v.toFixed(1) + " " + units[i];
}

function pct(used: number | null, total: number | null): number | null {
  if (used == null || total == null || total <= 0) return null;
  return Math.min(100, Math.max(0, (used / total) * 100));
}

/** Amber above 75%, red above 90% - same status language as services. */
function loadColor(p: number | null): string {
  if (p == null) return "var(--t3)";
  if (p >= 90) return "var(--st-danger)";
  if (p >= 75) return "var(--st-warn)";
  return "var(--st-ok)";
}

function tempColor(c: number | null): string {
  if (c == null) return "var(--t3)";
  if (c >= 85) return "var(--st-danger)";
  if (c >= 70) return "var(--st-warn)";
  return "var(--st-ok)";
}

/** Tiny inline CPU history bars - trend without spending a whole row. */
function Sparkline({ values }: { values: number[] }) {
  if (values.length < 2) return null;
  const shown = values.slice(-HISTORY);
  return (
    <span className="flex h-3 items-end gap-[1px]" aria-hidden>
      {shown.map((v, i) => (
        <span
          key={i}
          className="w-[2px] rounded-sm"
          style={{
            height: Math.max(2, Math.round((v / 100) * 12)) + "px",
            background: loadColor(v),
            opacity: 0.35 + (0.65 * (i + 1)) / shown.length,
          }}
        />
      ))}
    </span>
  );
}

function Metric(props: {
  icon: React.ReactNode;
  value: string;
  color?: string;
  title?: string;
}) {
  return (
    <span className="inline-flex shrink-0 items-center gap-1 whitespace-nowrap" title={props.title}>
      <span className="flex size-3 items-center justify-center text-[var(--t3)]">{props.icon}</span>
      <span className="font-semibold" style={{ color: props.color ?? "var(--t2)" }}>
        {props.value}
      </span>
    </span>
  );
}

export type EnvItem = { name: string; ok: boolean; v: string | null };


function Row(props: { label: string; value: string; ratio: number | null; color: string }) {
  return (
    <div className="flex items-center gap-2">
      <dt className="w-16 shrink-0 text-[var(--t3)]">{props.label}</dt>
      <dd className="flex min-w-0 flex-1 items-center gap-2">
        <span className="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-[var(--surface-2)]">
          <span
            className="block h-full rounded-full transition-[width] duration-500"
            style={{ width: (props.ratio ?? 0) + "%", background: props.color }}
          />
        </span>
        <span className="shrink-0 font-mono text-[var(--t2)]">{props.value}</span>
      </dd>
    </div>
  );
}

/**
 * Status bar. Host metrics stay pinned (CPU / MEM / DISK / temp); environment
 * versions share one rotating slot, and anything missing pins itself there
 * instead of rotating away. Click the bar to expand full details.
 */
export function StatusBar(props: {
  env: EnvItem[];
  left: React.ReactNode;
  right: React.ReactNode;
}) {
  const { t } = useTranslation();
  const [host, setHost] = useState<HostMetrics | null>(null);
  const [cpuHistory, setCpuHistory] = useState<number[]>([]);
  const [expanded, setExpanded] = useState(false);
  const [rotateIdx, setRotateIdx] = useState(0);
  const [paused, setPaused] = useState(false);
  const [tempMode, setTempMode] = useState<TempMode>(loadTempMode);
  const alive = useRef(true);

  const pickTempMode = (mode: TempMode) => {
    setTempMode(mode);
    try {
      window.localStorage.setItem(TEMP_MODE_KEY, mode);
    } catch {
      // Preference is cosmetic; losing persistence is not worth surfacing.
    }
  };

  useEffect(() => {
    alive.current = true;
    const tick = async () => {
      try {
        const m = await apiSystemMetrics(tempMode);
        if (!alive.current) return;
        setHost(m);
        const c = m.cpuPercent;
        if (c != null) setCpuHistory((prev) => prev.concat([c]).slice(-HISTORY));
      } catch {
        // Ambient UI: a failed sample keeps the previous reading.
      }
    };
    void tick();
    const id = window.setInterval(
      () => void tick(),
      tempMode === "fast" ? POLL_MS_FAST_TEMP : POLL_MS,
    );
    return () => {
      alive.current = false;
      window.clearInterval(id);
    };
  }, [tempMode]);

  const missing = useMemo(() => props.env.filter((x) => !x.ok), [props.env]);
  const healthy = useMemo(() => props.env.filter((x) => x.ok), [props.env]);
  const rotating = missing.length ? [] : healthy;

  // Escape closes the popover, matching the app's other overlays.
  useEffect(() => {
    if (!expanded) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setExpanded(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [expanded]);

  useEffect(() => {
    if (paused || expanded || rotating.length < 2) return;
    const id = window.setInterval(() => setRotateIdx((i) => (i + 1) % rotating.length), ROTATE_MS);
    return () => window.clearInterval(id);
  }, [paused, expanded, rotating.length]);

  const memPct = pct(host?.memoryUsedBytes ?? null, host?.memoryTotalBytes ?? null);
  const diskPct = pct(host?.diskUsedBytes ?? null, host?.diskTotalBytes ?? null);
  const cpu = host?.cpuPercent ?? null;
  const temp = host?.cpuTempC ?? null;
  const tempSupported = host?.cpuTempSupported ?? true;
  const current = rotating.length ? rotating[rotateIdx % rotating.length] : null;

  const memTitle = host?.memoryTotalBytes
    ? t("statusBar.memory") + " " + fmtBytes(host.memoryUsedBytes) + " / " + fmtBytes(host.memoryTotalBytes)
    : t("statusBar.memory");
  const diskTitle = host?.diskTotalBytes
    ? t("statusBar.disk") + " " + fmtBytes(host.diskUsedBytes) + " / " + fmtBytes(host.diskTotalBytes)
    : t("statusBar.disk");

  return (
    <div className="relative shrink-0">
      {expanded ? (
        <>
          {/* Click anywhere else to dismiss, without stealing focus or scroll. */}
          <div className="fixed inset-0 z-[90]" onClick={() => setExpanded(false)} aria-hidden />
          <div
            role="dialog"
            aria-label={t("statusBar.system")}
            className={cn(
              "absolute bottom-full right-2 z-[100] mb-1.5 w-[min(34rem,calc(100vw-1rem))]",
              "rounded-[var(--r-md)] border border-[var(--line)] bg-[var(--surface)] px-3.5 py-3",
              "shadow-[0_12px_32px_rgb(0_0_0/0.16)] duration-150 animate-in fade-in-0 slide-in-from-bottom-1",
            )}
          >
            <div className="grid gap-4 sm:grid-cols-2">
              <section>
                <h4 className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--t3)]">
                  {t("statusBar.system")}
                </h4>
                <dl className="flex flex-col gap-1.5 text-[11px]">
                  <Row
                    label={t("statusBar.cpu")}
                    value={cpu == null ? "\u2014" : cpu.toFixed(0) + "%"}
                    ratio={cpu}
                    color={loadColor(cpu)}
                  />
                  <Row
                    label={t("statusBar.memory")}
                    value={
                      host?.memoryTotalBytes
                        ? fmtBytes(host.memoryUsedBytes) + " / " + fmtBytes(host.memoryTotalBytes)
                        : "\u2014"
                    }
                    ratio={memPct}
                    color={loadColor(memPct)}
                  />
                  <Row
                    label={t("statusBar.disk")}
                    value={
                      host?.diskTotalBytes
                        ? fmtBytes(host.diskUsedBytes) + " / " + fmtBytes(host.diskTotalBytes)
                        : "\u2014"
                    }
                    ratio={diskPct}
                    color={loadColor(diskPct)}
                  />
                  <Row
                    label={t("statusBar.cpuTemp")}
                    value={
                      temp != null
                        ? temp.toFixed(0) + " \u00b0C"
                        : !tempSupported
                          ? t("statusBar.unsupported")
                          : tempMode === "off"
                            ? t("statusBar.tempOffValue")
                            : "\u2014"
                    }
                    ratio={temp == null ? null : Math.min(100, temp)}
                    color={tempColor(temp)}
                  />
                </dl>

                <div className="mt-2.5 flex items-center gap-2 text-[11px]">
                  <span className="w-16 shrink-0 text-[var(--t3)]">{t("statusBar.tempModeLabel")}</span>
                  <div className="flex overflow-hidden rounded-[var(--r-sm)] border border-[var(--line)]">
                    {TEMP_MODES.map((mode) => (
                      <button
                        key={mode}
                        type="button"
                        disabled={!tempSupported && mode !== "off"}
                        onClick={() => pickTempMode(mode)}
                        title={t(`statusBar.tempModes.${mode}.hint`)}
                        className={cn(
                          "px-2 py-0.5 transition-colors duration-150",
                          "border-l border-[var(--line)] first:border-l-0",
                          tempMode === mode
                            ? "bg-[var(--st-accent-tint)] font-semibold text-[var(--st-accent-hover)]"
                            : "text-[var(--t2)] hover:bg-[var(--surface-2)]",
                          !tempSupported && mode !== "off" && "cursor-not-allowed opacity-40 hover:bg-transparent",
                        )}
                      >
                        {t(`statusBar.tempModes.${mode}.label`)}
                      </button>
                    ))}
                  </div>
                </div>
                <p className="mt-1.5 text-[10px] leading-relaxed text-[var(--t3)]">
                  {tempSupported
                    ? t(`statusBar.tempModes.${tempMode}.hint`)
                    : t("statusBar.tempUnsupportedHint")}
                </p>
              </section>
              <section>
                <h4 className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--t3)]">
                  {t("statusBar.environment")}
                </h4>
                <div className="flex flex-wrap gap-x-4 gap-y-1.5 text-[11px]">
                  {props.env.length ? (
                    props.env.map((item) => (
                      <span key={item.name} className="inline-flex items-center gap-1.5 whitespace-nowrap">
                        <span
                          className="size-1.5 shrink-0 rounded-full"
                          style={{ background: item.ok ? "var(--st-ok)" : "var(--st-warn-dot)" }}
                        />
                        <span className="font-semibold text-[var(--t1)]">{item.name}</span>
                        <span className="font-mono text-[var(--t3)]">
                          {item.ok ? item.v ?? "\u2713" : t("statusBar.notFound")}
                        </span>
                      </span>
                    ))
                  ) : (
                    <span className="text-[var(--t3)]">{"\u2014"}</span>
                  )}
                </div>
              </section>
            </div>
          </div>
        </>
      ) : null}

      <footer
        className="flex h-[30px] items-center gap-2 overflow-hidden whitespace-nowrap border-t border-[var(--line)] bg-[var(--surface)] px-3 font-mono text-[11px] text-[var(--t3)]"
        onMouseEnter={() => setPaused(true)}
        onMouseLeave={() => setPaused(false)}
      >
        {props.left}
        <span className="text-[var(--line-strong)]">{"\u00b7"}</span>

        <span className="flex shrink-0 items-center gap-3">
          <span className="inline-flex items-center gap-1.5">
            <Metric
              icon={<Cpu className="size-3" />}
              value={cpu == null ? "\u2014" : cpu.toFixed(0) + "%"}
              color={loadColor(cpu)}
              title={t("statusBar.cpu")}
            />
            <Sparkline values={cpuHistory} />
          </span>
          <Metric
            icon={<MemoryStick className="size-3" />}
            value={memPct == null ? "\u2014" : memPct.toFixed(0) + "%"}
            color={loadColor(memPct)}
            title={memTitle}
          />
          <Metric
            icon={<HardDrive className="size-3" />}
            value={diskPct == null ? "\u2014" : diskPct.toFixed(0) + "%"}
            color={loadColor(diskPct)}
            title={diskTitle}
          />
          {temp != null ? (
            <Metric
              icon={<Thermometer className="size-3" />}
              value={temp.toFixed(0) + "\u00b0"}
              color={tempColor(temp)}
              title={t("statusBar.cpuTemp")}
            />
          ) : null}
        </span>

        <span className="text-[var(--line-strong)]">{"\u00b7"}</span>

        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          title={t("statusBar.toggleDetails")}
          className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden text-left transition-colors hover:text-[var(--t2)]"
        >
          {missing.length ? (
            <span className="flex min-w-0 items-center gap-2 overflow-hidden">
              {missing.slice(0, 3).map((item) => (
                <span key={item.name} className="inline-flex items-center gap-1 whitespace-nowrap">
                  <span className="size-1.5 rounded-full bg-[var(--st-warn-dot)]" />
                  <span className="font-semibold text-[var(--st-warn)]">{item.name}</span>
                  <span>{t("statusBar.notFound")}</span>
                </span>
              ))}
            </span>
          ) : current ? (
            <span key={current.name} className="inline-flex items-center gap-1 whitespace-nowrap">
              <span className="size-1.5 rounded-full bg-[var(--st-ok)]" />
              <span className="font-semibold text-[var(--t2)]">{current.name}</span>
              <span>{current.v ?? "\u2713"}</span>
            </span>
          ) : (
            <span>{t("statusBar.environment")}</span>
          )}
          {props.env.length ? (
            <span className="shrink-0 rounded-full bg-[var(--surface-2)] px-1.5 text-[10px] text-[var(--t3)]">
              {healthy.length}/{props.env.length}
            </span>
          ) : null}
          <ChevronUp
            className={cn("size-3 shrink-0 transition-transform duration-200", expanded && "rotate-180")}
          />
        </button>

        <span className="flex shrink-0 items-center gap-2 pl-3">{props.right}</span>
      </footer>
    </div>
  );
}
