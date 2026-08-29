import type { RtState, ScriptRuntimeView, ScriptState } from "@/ipc/protocol";
import i18n from "@/i18n";
import { errorDisplayText } from "@/lib/error-messages";

/** 状态点配色（文案走 `states.*` 的 i18n key，见 stateLabel）。 */
export const STATE_META: Record<RtState, { color: string; ring: string }> = {
  stopped: { color: "var(--t3)", ring: "var(--line)" },
  building: { color: "#9a6700", ring: "#f0d58a" },
  starting: { color: "#9a6700", ring: "#f0d58a" },
  running: { color: "var(--st-ok)", ring: "#9be3ad" },
  unhealthy: { color: "var(--st-danger)", ring: "#f3b4b4" },
  stopping: { color: "#9a6700", ring: "#f0d58a" },
  exited: { color: "var(--st-danger)", ring: "#f3b4b4" },
};

export function StatusDot({ state, size = 8 }: { state: RtState; size?: number }) {
  const m = STATE_META[state];
  // ring 用 box-shadow 外扩；外层占位避免被父级 overflow-hidden 裁切
  const ring = 3;
  return (
    <span
      aria-hidden
      className="inline-flex shrink-0 items-center justify-center"
      style={{ width: size + ring * 2, height: size + ring * 2 }}
    >
      <span
        style={{
          width: size,
          height: size,
          borderRadius: "50%",
          background: m.color,
          boxShadow: `0 0 0 ${ring}px ${m.ring}`,
          display: "block",
        }}
      />
    </span>
  );
}

export function stateLabel(state: RtState): string {
  return i18n.t(`states.${state}`);
}

export function fmtDuration(ms: number | null | undefined): string {
  if (!ms) return "—";
  const s = Math.floor((Date.now() - ms) / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

export function fmtTime(ts: number): string {
  const d = new Date(ts);
  return d.toTimeString().slice(0, 8);
}

export function healthClass(ok: boolean | null | undefined): string {
  if (ok === null || ok === undefined) return "text-muted-foreground";
  return ok ? "text-emerald-600" : "text-red-600";
}

// ---------------------------------------------------------------------------
// 脚本任务状态（ScriptRuntimeView）：一次只跑一个脚本，state 只有三态
// ---------------------------------------------------------------------------

/** 脚本状态点配色（文案走 `states.script*` 的 i18n key）。 */
export const SCRIPT_STATE_META: Record<ScriptState, { color: string; ring: string }> = {
  idle: { color: "var(--t3)", ring: "var(--line)" },
  running: { color: "var(--st-ok)", ring: "#9be3ad" },
  exited: { color: "var(--t3)", ring: "var(--line)" },
};

/** 复用服务 StatusDot 的三态映射：idle → stopped；退出码 0 的完成态也是中性灰，仅失败用红。 */
export function scriptDotState(view: Pick<ScriptRuntimeView, "state" | "last_exit" | "last_error">): RtState {
  if (view.state === "idle") return "stopped";
  if (view.state === "exited") {
    return view.last_error || (view.last_exit?.code ?? 1) !== 0 ? "exited" : "stopped";
  }
  return "running";
}

export function scriptStateLabel(view: Pick<ScriptRuntimeView, "state" | "last_exit" | "last_error">): string {
  if (view.state !== "exited") return i18n.t(`states.script_${view.state}`);
  if (view.last_error) return i18n.t("states.script_failed");
  if (view.last_exit?.code === 0) return i18n.t("states.script_done");
  if (view.last_exit) return i18n.t("states.script_exited_with_code", { code: view.last_exit.code });
  return i18n.t("states.script_exited");
}

// ---------------------------------------------------------------------------
// 1.6 网关托管状态（GatewayRuntimeView）：chip 配色 + 状态点，文案走
// `pages.gateway.state_*`。网关页与运行页代理 Tab 共用。
// ---------------------------------------------------------------------------

/** 网关状态 chip 底色（tint + 深字，与服务状态 chip 同一视觉语言）。 */
export const GATEWAY_STATE_TINT: Record<RtState, string> = {
  running: "bg-[#e9f7ed] text-[#1e7e35]",
  starting: "bg-[#fff8e1] text-[#9a6700]",
  building: "bg-[#fff8e1] text-[#9a6700]",
  stopping: "bg-[#fff8e1] text-[#9a6700]",
  unhealthy: "bg-[#fdecec] text-[#dc2626]",
  exited: "bg-[#fdecec] text-[#dc2626]",
  stopped: "bg-[var(--surface-2,#f3f4f5)] text-[var(--t2,#62666d)]",
};

/** 网关状态点颜色（chip 内小圆点）。 */
export const GATEWAY_STATE_DOT: Record<RtState, string> = {
  running: "bg-[#27a644]",
  starting: "bg-[#d9a514]",
  building: "bg-[#d9a514]",
  stopping: "bg-[#d9a514]",
  unhealthy: "bg-[#dc2626]",
  exited: "bg-[#dc2626]",
  stopped: "bg-[#8a8f98]",
};

// ---------------------------------------------------------------------------
// 1.1 长操作错误码 → 本地化文案（模板 / Git / 更新，ipc.md §7、§10）
// 映射真源在 `lib/error-messages.ts` + 资源 `errors.<CODE>`；后端 message 保持中文作详情。
// ---------------------------------------------------------------------------

/** 长操作失败提示：命中错误码映射显示本地化文案，未知 code 原样返回。 */
export function opErrorLabel(code: string | null | undefined): string {
  return errorDisplayText(code, null, i18n.t("errors.OP_FAILED"));
}
