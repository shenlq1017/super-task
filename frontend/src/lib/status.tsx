import type { RtState } from "@/ipc/protocol";

export const STATE_META: Record<RtState, { label: string; color: string; ring: string }> = {
  stopped: { label: "已停止", color: "var(--t3)", ring: "var(--line)" },
  building: { label: "构建中", color: "#9a6700", ring: "#f0d58a" },
  starting: { label: "启动中", color: "#9a6700", ring: "#f0d58a" },
  running: { label: "运行中", color: "var(--st-ok)", ring: "#9be3ad" },
  unhealthy: { label: "不健康", color: "var(--st-danger)", ring: "#f3b4b4" },
  stopping: { label: "停止中", color: "#9a6700", ring: "#f0d58a" },
  exited: { label: "已退出", color: "var(--st-danger)", ring: "#f3b4b4" },
};

export function StatusDot({ state, size = 8 }: { state: RtState; size?: number }) {
  const m = STATE_META[state];
  return (
    <span
      aria-hidden
      style={{
        width: size,
        height: size,
        borderRadius: "50%",
        background: m.color,
        boxShadow: `0 0 0 3px ${m.ring}`,
        display: "inline-block",
        flexShrink: 0,
      }}
    />
  );
}

export function stateLabel(state: RtState): string {
  return STATE_META[state].label;
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
// 1.1 长操作错误码 → 中文（模板 / Git / 更新，ipc.md §7、§10）
// ---------------------------------------------------------------------------

export const OP_ERROR_LABEL: Record<string, string> = {
  TARGET_NOT_EMPTY: "目标目录已存在且非空，请换一个目录名或清空后重试",
  TEMPLATE_INVALID: "内置模板校验失败，请升级 SuperTask 后重试",
  TEMPLATE_WRITE: "模板文件写入失败，请检查目录权限",
  PATH_ESCAPE: "目录名非法：不能包含路径分隔符、盘符或 ..",
  CWD_MISSING: "目录不存在，请重新选择",
  NOT_FOUND: "目标不存在",
  GIT_NOT_FOUND: "PATH 中没有 git.exe，请先安装 Git 并重试",
  GIT_NOT_REPOSITORY: "该目录不是 Git 仓库",
  GIT_DIRTY: "有未提交修改：请先提交或暂存，或选择「仍然拉取」",
  GIT_WORKSPACE_BUSY: "有服务或脚本正在运行，请先停止后再拉取",
  GIT_CONFLICT: "拉取产生冲突，已保留现场：请用 IDE 处理冲突，SuperTask 不自动恢复现场",
  GIT_AUTH: "Git 认证失败：请在 Git Credential Manager 中更新凭据后重试",
  GIT_REMOTE: "远端不存在或不可访问：请检查 URL 与网络后重试",
  GIT_BRANCH: "分支不存在或无法跟踪，请确认分支名",
  GIT_FAILED: "Git 命令执行失败，请查看引擎日志",
  IDE_NOT_FOUND: "未在固定候选中找到目标 IDE，请确认已安装",
  AUTOSTART_FAILED: "开机启动注册失败，已回滚为关闭；可稍后重试",
  UPDATE_BLOCKED_RUNNING: "有运行中的任务或进行中的操作，停止后再试",
  UPDATE_SIGNATURE: "更新包签名校验失败，已拒绝安装",
  UPDATE_FAILED: "更新失败，当前版本仍可继续使用",
  ALREADY_IN_PROGRESS: "已有同名操作在进行中，请稍候",
  // ---- 1.2 工具链 / 网络 ----
  TOOLCHAIN_MANAGER_MISSING: "未找到 mise 或 winget：请先安装其中之一（winget 随 Windows 11 自带）",
  TOOLCHAIN_VERSION_INVALID: "版本号非法：只允许数字、点号、连字符与 lts 别名",
  TOOLCHAIN_INSTALL_FAILED: "安装失败，已保留原有工具与配置：可检查网络后重试，或换用其他 provider",
  TOOLCHAIN_PERMISSION: "provider 需要管理员权限：请改用系统安装器完成安装，SuperTask 不会提权",
  MISSING_TOOL: "安装命令成功但未解析到工具：请重新探测；若刚安装可重启应用刷新 PATH",
  PROXY_INVALID: "代理 URL 非法：只允许 http/https，且不能内嵌用户名密码",
};

/** 长操作失败的中文提示；未知 code 原样返回。 */
export function opErrorLabel(code: string | null | undefined): string {
  if (!code) return "操作失败";
  return OP_ERROR_LABEL[code] ?? code;
}
