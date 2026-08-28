import type { RtState, ScriptRuntimeView, ScriptState } from "@/ipc/protocol";

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
// 脚本任务状态（ScriptRuntimeView）：一次只跑一个脚本，state 只有三态
// ---------------------------------------------------------------------------

export const SCRIPT_STATE_META: Record<ScriptState, { label: string; color: string; ring: string }> = {
  idle: { label: "待运行", color: "var(--t3)", ring: "var(--line)" },
  running: { label: "运行中", color: "var(--st-ok)", ring: "#9be3ad" },
  exited: { label: "已结束", color: "var(--t3)", ring: "var(--line)" },
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
  if (view.state !== "exited") return SCRIPT_STATE_META[view.state].label;
  if (view.last_error) return "已失败";
  if (view.last_exit?.code === 0) return "已完成";
  if (view.last_exit) return `已退出 · 码 ${view.last_exit.code}`;
  return "已结束";
}

// ---------------------------------------------------------------------------
// 1.1 长操作错误码 → 中文（模板 / Git / 更新，ipc.md §7、§10）
// ---------------------------------------------------------------------------

export const OP_ERROR_LABEL: Record<string, string> = {
  TARGET_NOT_EMPTY: "目标目录已存在且非空，请换一个目录名或清空后重试",
  TEMPLATE_INVALID: "模板校验失败：清单缺失或文件不一致（本地模板请检查 template.yaml）",
  TEMPLATE_WRITE: "模板文件写入失败，请检查目录权限",
  TEMPLATE_ID_CONFLICT: "本地模板 id 与内置模板冲突，请重命名本地模板目录后重试",
  TEMPLATE_PARAM_MISSING: "缺少必填模板参数，请补全后重试",
  TEMPLATE_PARAM_UNKNOWN: "模板未声明该参数，请刷新模板列表后重试",
  TEMPLATE_BLOCK_DEP: "服务块组合无效：所选块或其依赖不存在",
  TEMPLATE_BLOCK_PORT: "端口分配无效：存在缺失、重复或越界的端口",
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
  // ---- 1.2 端口 / 日志 / 指标 / profile ----
  PORT_SCAN_FAILED: "无法读取本机端口表（netstat 失败），不能当作端口可用",
  PORT_NO_AVAILABLE: "附近没有可用端口，请手动指定",
  PORT_IN_USE: "端口仍被占用，请先停止占用进程或更换端口",
  LOG_QUERY_INVALID: "搜索词非法：最长 256 字符",
  LOG_EXPORT_FAILED: "日志导出失败：目标文件已存在（不覆盖）或目录不可写",
  LOG_RETENTION_FAILED: "日志清理失败：轮转文件可能被占用",
  METRICS_UNAVAILABLE: "指标暂时不可读取：服务可能刚启动或未由 SuperTask 托管",
  PROFILE_NOT_FOUND: "profile 不存在，请检查 supertask.yaml",
  PROFILE_INVALID: "profile 配置非法：只允许覆盖 env/enabled/port",
  PROFILE_SWITCH_BUSY: "有服务正在启动/停止，切换 profile 请稍后重试",
  PROFILE_DISABLED: "该 profile 已被禁用",
  YAML_CONFLICT: "配置已被其他人/窗口修改（base_hash 不一致），请刷新后重试",
  LAUNCH_UNSUPPORTED: "该服务的 launch 方式暂不支持，请检查 supertask.yaml",
  FEATURE_SOON: "该功能尚未开放（规划中版本提供）",
  SCRIPT_BUSY: "已有脚本正在运行：同一工作区同时只能运行一个脚本",
  // ---- 1.3 docker / compose ----
  DOCKER_NOT_FOUND: "PATH 中没有 docker 可执行文件：请安装 Docker Desktop 后重试（SuperTask 不代装）",
  DOCKER_ENGINE_UNREACHABLE: "Docker 引擎未运行：请启动 Docker Desktop 后重试",
  DOCKER_COMPOSE_MISSING: "docker compose 插件不可用：请升级 Docker Desktop 后重试",
  COMPOSE_FILE_MISSING: "找不到 compose 文件：请确认 docker.compose_file 配置",
  COMPOSE_SERVICE_MISSING: "该服务不在 compose 文件中：请检查 services.*.service 是否与 compose 服务名一致",
  COMPOSE_CONFIG_FAILED: "docker compose config 解析失败：请检查 compose 文件语法",
  COMPOSE_UP_FAILED: "容器启动失败：详情见该服务日志（up 输出摘要）",
  COMPOSE_STOP_FAILED: "容器停止失败：将按容器实际状态对齐显示",
  COMPOSE_PORT_MISMATCH: "YAML 端口与 compose 映射不一致：健康检查以 YAML port 为准",
  DOCKER_BUILD_UNKNOWN: "构建条目不存在：docker.build 的 name 必须是 supertask.yaml docker.builds 中已定义的条目",
  IMAGE_BUILD_FAILED: "镜像构建失败：daemon 错误摘要见 operation message 或服务日志",
};

/** 长操作失败的中文提示；未知 code 原样返回。 */
export function opErrorLabel(code: string | null | undefined): string {
  if (!code) return "操作失败";
  return OP_ERROR_LABEL[code] ?? code;
}
