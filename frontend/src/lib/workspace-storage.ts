const LAST_KEY = "st:lastWorkspace";
const RECENTS_KEY = "st:recents";
const CARDS_COLLAPSED_KEY = "st:runCardsCollapsed";

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
