const LAST_KEY = "st:lastWorkspace";
const RECENTS_KEY = "st:recents";
const CARDS_COLLAPSED_KEY = "st:runCardsCollapsed";
const LOG_WRAP_KEY = "st:logWrap";
const LOG_LIMIT_KEY = "st:logLineLimit";
const LOG_SHOW_TIME_KEY = "st:logShowTime";

export function readLastWorkspace(): string | null {
  try {
    return localStorage.getItem(LAST_KEY);
  } catch {
    return null;
  }
}

export function writeLastWorkspace(path: string) {
  try {
    localStorage.setItem(LAST_KEY, path);
  } catch {
    /* ignore */
  }
}

export function clearLastWorkspace() {
  try {
    localStorage.removeItem(LAST_KEY);
  } catch {
    /* ignore */
  }
}

export function readRecents(): string[] {
  try {
    const raw = localStorage.getItem(RECENTS_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    return Array.isArray(parsed) ? parsed.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export function writeRecents(paths: string[]) {
  try {
    localStorage.setItem(RECENTS_KEY, JSON.stringify(paths.slice(0, 8)));
  } catch {
    /* ignore */
  }
}

export function mergeRecents(server: string[], local: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const p of [...server, ...local]) {
    if (!p || seen.has(p)) continue;
    seen.add(p);
    out.push(p);
    if (out.length >= 8) break;
  }
  return out;
}

export function readCardsCollapsedPref(): boolean | null {
  try {
    const v = localStorage.getItem(CARDS_COLLAPSED_KEY);
    if (v === null) return null;
    return v === "1";
  } catch {
    return null;
  }
}

export function writeCardsCollapsedPref(collapsed: boolean) {
  try {
    localStorage.setItem(CARDS_COLLAPSED_KEY, collapsed ? "1" : "0");
  } catch {
    /* ignore */
  }
}

export function readLogWrapPref(): boolean {
  try {
    return localStorage.getItem(LOG_WRAP_KEY) === "1";
  } catch {
    return false;
  }
}

export function writeLogWrapPref(wrap: boolean) {
  try {
    localStorage.setItem(LOG_WRAP_KEY, wrap ? "1" : "0");
  } catch {
    /* ignore */
  }
}

export function readLogLineLimitPref(): number {
  try {
    const n = Number(localStorage.getItem(LOG_LIMIT_KEY));
    return Number.isInteger(n) && n >= 50 && n <= 5000 ? n : 500;
  } catch {
    return 500;
  }
}

export function writeLogLineLimitPref(limit: number) {
  try {
    localStorage.setItem(LOG_LIMIT_KEY, String(Math.max(50, Math.min(5000, limit))));
  } catch {
    /* ignore */
  }
}

export function readLogShowTimePref(): boolean {
  try {
    const v = localStorage.getItem(LOG_SHOW_TIME_KEY);
    if (v === null) return true;
    return v === "1";
  } catch {
    return true;
  }
}

export function writeLogShowTimePref(show: boolean) {
  try {
    localStorage.setItem(LOG_SHOW_TIME_KEY, show ? "1" : "0");
  } catch {
    /* ignore */
  }
}
