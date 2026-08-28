/**
 * i18next 初始化（1.4 规格 §6）：
 * - 资源按 locale 静态 import（不按需加载），单一 translation namespace、嵌套 key；
 * - 初始语言 = resolveLocale("auto")（navigator.language 规则），
 *   app.load 拿到 prefs.locale 后由 session-provider 调 applyLocalePreference 校正；
 * - zh-CN 为源语言 + fallbackLng。
 */
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { resolveLocale } from "./resolve-locale";
import zhCN from "./locales/zh-CN";
import zhTW from "./locales/zh-TW";
import enUS from "./locales/en-US";
import jaJP from "./locales/ja-JP";

export const resources = {
  "zh-CN": { translation: zhCN },
  "zh-TW": { translation: zhTW },
  "en-US": { translation: enUS },
  "ja-JP": { translation: jaJP },
} as const;

void i18next.use(initReactI18next).init({
  resources,
  lng: resolveLocale("auto"),
  fallbackLng: "zh-CN",
  defaultNS: "translation",
  ns: ["translation"],
  interpolation: { escapeValue: false },
  returnEmptyString: false,
  react: { useSuspense: false },
});

/** 应用语言偏好（"auto" | "zh-CN" | "zh-TW" | "en-US" | "ja-JP"），即时生效。 */
export function applyLocalePreference(preference?: string | null): void {
  void i18next.changeLanguage(resolveLocale(preference));
}

export default i18next;
