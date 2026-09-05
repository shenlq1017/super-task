/** Host-metric formatting shared by the status bar and the monitor page. */

export function fmtBytes(n: number | null | undefined): string {
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

/** Network throughput: bytes below 1 KB/s, one decimal above (e.g. 12.3 KB/s). */
export function fmtRate(bps: number | null | undefined): string {
  if (bps == null) return "\u2014";
  const units = ["B/s", "KB/s", "MB/s", "GB/s"];
  let v = bps;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return (v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)) + " " + units[i];
}

export function pct(used: number | null, total: number | null): number | null {
  if (used == null || total == null || total <= 0) return null;
  return Math.min(100, Math.max(0, (used / total) * 100));
}

/** Amber above 75%, red above 90% - same status language as services. */
export function loadColor(p: number | null): string {
  if (p == null) return "var(--t3)";
  if (p >= 90) return "var(--st-danger)";
  if (p >= 75) return "var(--st-warn)";
  return "var(--st-ok)";
}

export function tempColor(c: number | null): string {
  if (c == null) return "var(--t3)";
  if (c >= 85) return "var(--st-danger)";
  if (c >= 70) return "var(--st-warn)";
  return "var(--st-ok)";
}
