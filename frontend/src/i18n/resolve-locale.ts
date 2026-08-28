/** locale 解析规则（1.4 规格 §6.1）：auto 走 navigator.language，显式未知值回落 zh-CN。 */

export const SUPPORTED_LOCALES = ["zh-CN", "zh-TW", "en-US", "ja-JP"] as const;

export type Locale = (typeof SUPPORTED_LOCALES)[number];

export type LocalePreference = Locale | "auto";

export function isSupportedLocale(v: string | null | undefined): v is Locale {
  return !!v && (SUPPORTED_LOCALES as readonly string[]).includes(v);
}

/**
 * 偏好值 → 实际 locale：
 * - 显式受支持的 locale 原样返回；
 * - 显式未知值回落 zh-CN（设置页负责提示）；
 * - "auto"/空：navigator.language 匹配（ja* → ja-JP；zh-TW/zh-HK/zh-Hant* → zh-TW；
 *   zh* → zh-CN；en* → en-US；其余回落 zh-CN）。
 */
export function resolveLocale(preference?: string | null): Locale {
  if (isSupportedLocale(preference)) return preference;
  if (preference && preference !== "auto") return "zh-CN";

  const nav = typeof navigator !== "undefined" ? navigator : undefined;
  const candidates: readonly string[] = nav?.languages?.length
    ? Array.from(nav.languages)
    : nav?.language
      ? [nav.language]
      : [];

  for (const raw of candidates) {
    const lang = (raw ?? "").toLowerCase();
    if (!lang) continue;
    if (lang.startsWith("ja")) return "ja-JP";
    if (lang.startsWith("zh-hant") || lang.startsWith("zh-tw") || lang.startsWith("zh-hk")) return "zh-TW";
    if (lang.startsWith("zh")) return "zh-CN";
    if (lang.startsWith("en")) return "en-US";
  }
  return "zh-CN";
}
