/** SuperTask appearance: palette × light/dark, persisted in prefs.theme. */

export type ThemeMode = "light" | "dark";
export type ThemePalette = "indigo" | "slate" | "emerald" | "amber" | "rose" | "ocean";

export const THEME_PALETTES: readonly ThemePalette[] = [
  "indigo",
  "slate",
  "emerald",
  "amber",
  "rose",
  "ocean",
] as const;

export type ParsedTheme = { palette: ThemePalette; mode: ThemeMode };

/** Swatches for the Settings appearance picker (light accents). */
export const PALETTE_SWATCHES: Record<
  ThemePalette,
  { accent: string; accentSoft: string; bg: string }
> = {
  indigo: { accent: "#5b63d3", accentSoft: "#eef0fb", bg: "#f5f6f7" },
  slate: { accent: "#475569", accentSoft: "#e8eef5", bg: "#f8fafc" },
  emerald: { accent: "#059669", accentSoft: "#e6f6ef", bg: "#f4f9f6" },
  amber: { accent: "#d97706", accentSoft: "#fff3e0", bg: "#faf8f4" },
  rose: { accent: "#e11d48", accentSoft: "#fde8ee", bg: "#faf5f6" },
  ocean: { accent: "#0891b2", accentSoft: "#e0f5fa", bg: "#f3f8fa" },
};

function isPalette(v: string): v is ThemePalette {
  return (THEME_PALETTES as readonly string[]).includes(v);
}

/** Parse prefs.theme. `light`/`dark` stay indigo for backward compatibility. */
export function parseTheme(theme: string | null | undefined): ParsedTheme {
  const raw = (theme ?? "light").trim().toLowerCase();
  if (raw === "light") return { palette: "indigo", mode: "light" };
  if (raw === "dark") return { palette: "indigo", mode: "dark" };
  const m = /^([a-z]+)-(light|dark)$/.exec(raw);
  if (m && isPalette(m[1])) {
    return { palette: m[1], mode: m[2] as ThemeMode };
  }
  if (isPalette(raw)) return { palette: raw, mode: "light" };
  return { palette: "indigo", mode: "light" };
}

/** Encode for prefs.theme. Indigo keeps legacy `light` / `dark` values. */
export function formatTheme(palette: ThemePalette, mode: ThemeMode): string {
  if (palette === "indigo") return mode;
  return `${palette}-${mode}`;
}

/** Apply palette + mode on <html>. Unknown values fall back to indigo light. */
export function applyTheme(theme: string | null | undefined) {
  const { palette, mode } = parseTheme(theme);
  const dark = mode === "dark";
  const root = document.documentElement;
  root.classList.toggle("dark", dark);
  root.dataset.palette = palette;
  root.style.colorScheme = dark ? "dark" : "light";
}
