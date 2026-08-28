/**
 * 错误码 → 本地化文案（1.4 规格 §6.2）：
 * - 命中 `errors.<CODE>` 映射 → 优先显示本地化文案；
 * - 未命中 → 显示后端 message 原文（后端 message 保持中文，作为详情保留）。
 * 供 React 组件与非 hook 模块（provider / toast）共用，直接走 i18n 单例。
 */
import i18n from "@/i18n";
import { IpcFailure } from "@/ipc/protocol";

/** code 是否有本地化文案。 */
export function hasLocalizedError(code: string | null | undefined): boolean {
  if (!code) return false;
  return i18n.exists(`errors.${code}`, { ns: "translation" });
}

/**
 * 错误主文案：命中映射 → 本地化；未命中 → message 原文 → code 兜底。
 * `fallback` 用于无 code 且无 message 时的默认提示。
 */
export function errorDisplayText(
  code: string | null | undefined,
  message?: string | null,
  fallback?: string,
): string {
  if (code && hasLocalizedError(code)) return i18n.t(`errors.${code}`);
  if (message) return message;
  if (code) return code;
  return fallback ?? i18n.t("errors.OP_FAILED");
}

/** IpcFailure → 用户可读文案（本地化优先）。 */
export function formatIpcFailure(e: unknown, fallback?: string): string {
  if (e instanceof IpcFailure) return errorDisplayText(e.code, e.message, fallback);
  if (e instanceof Error) return e.message || (fallback ?? String(e));
  return String(e) || (fallback ?? "");
}

/**
 * 错误详情：命中映射时后端 message 作为次行/可展开详情保留；
 * 未命中时返回 null（message 已在主文案展示）。
 */
export function errorDetailText(code: string | null | undefined, message?: string | null): string | null {
  if (code && hasLocalizedError(code) && message && message !== i18n.t(`errors.${code}`)) {
    return message;
  }
  return null;
}
