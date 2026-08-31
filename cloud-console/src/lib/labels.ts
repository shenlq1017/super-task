type Dict = Record<string, string>;

const zh: Dict = {
  "app.title": "云管理控制台",
  "app.accounts": "账号",
  "app.logout": "退出登录",
  "app.signedInAs": "当前管理员",

  "common.cancel": "取消",
  "common.confirm": "确认",
  "common.save": "保存",
  "common.close": "关闭",
  "common.search": "搜索",
  "common.retry": "重试",
  "common.refresh": "刷新",
  "common.loading": "加载中…",
  "common.created": "创建",
  "common.yes": "是",
  "common.no": "否",
  "common.unknownError": "操作失败",

  "login.title": "管理员登录",
  "login.subtitle": "使用具备管理员角色的账号登录云控制台。",
  "login.email": "邮箱",
  "login.password": "口令",
  "login.submit": "登录",
  "login.signingIn": "登录中…",
  "login.emailRequired": "请输入邮箱",
  "login.emailInvalid": "邮箱格式不正确",
  "login.passwordRequired": "请输入口令",
  "login.showPassword": "显示口令",
  "login.hidePassword": "隐藏口令",
  "login.endpointHint": "服务地址：{endpoint}",
  "login.notConfigured": "服务端尚未配置管理员账号，请设置 SUPERTASK_ADMIN_EMAIL 与 SUPERTASK_ADMIN_PASSWORD 后重启服务。",
  "login.notAdmin": "该账号没有管理员角色，无法登录控制台。",
  "login.authFailed": "邮箱或口令不正确。",
  "login.notConfiguredTitle": "管理面未启用",

  "accounts.title": "账号管理",
  "accounts.subtitle": "创建云账号、调整角色、停用或恢复登录、重置口令与删除账号。",
  "accounts.searchPlaceholder": "按邮箱搜索",
  "accounts.new": "新建账号",
  "accounts.empty": "没有匹配的账号。",
  "accounts.loadFailed": "账号列表加载失败",
  "accounts.count": "共 {n} 个账号",
  "accounts.usage": "{entities} 个实体 · {bytes}",
  "accounts.self": "（当前登录）",
  "accounts.selfHint": "不能对当前登录的管理员执行此操作",

  "col.email": "邮箱",
  "col.role": "角色",
  "col.status": "状态",
  "col.usage": "实体 / 用量",
  "col.created": "创建时间",
  "col.actions": "操作",

  "role.admin": "管理员",
  "role.user": "普通用户",

  "state.enabled": "启用",
  "state.disabled": "已停用",

  "action.enable": "恢复",
  "action.disable": "停用",
  "action.promote": "升为管理员",
  "action.demote": "降为普通",
  "action.setPassword": "重设口令",
  "action.delete": "删除",

  "create.title": "新建账号",
  "create.email": "邮箱",
  "create.password": "初始口令",
  "create.role": "角色",
  "create.hint": "口令至少 12 个字符，服务端以 Argon2id 加盐哈希后存储。",
  "create.submit": "创建账号",
  "create.creating": "创建中…",
  "create.created": "账号 {email} 已创建",
  "create.emailInvalid": "邮箱格式不正确",
  "create.passwordTooShort": "口令至少 12 个字符",

  "password.title": "重设口令",
  "password.for": "为 {email} 设置新口令",
  "password.new": "新口令",
  "password.hint": "至少 12 个字符。保存后该账号的既有会话仍可继续使用，直到令牌过期。",
  "password.submit": "保存口令",
  "password.saved": "口令已更新",

  "confirm.disable.title": "停用账号",
  "confirm.disable.body": "停用后 {email} 立即无法登录，云端数据保留。可以随时恢复。",
  "confirm.enable.title": "恢复账号",
  "confirm.enable.body": "恢复后 {email} 可以重新登录。",
  "confirm.demote.title": "撤销管理员角色",
  "confirm.demote.body": "{email} 将失去控制台登录权限，云端同步数据不受影响。",
  "confirm.promote.title": "授予管理员角色",
  "confirm.promote.body": "{email} 将能登录控制台并管理所有账号，请确认此人可信。",
  "confirm.delete.title": "删除账号",
  "confirm.delete.body": "将永久删除 {email} 及其 {entities} 个实体、全部登录令牌与遥测记录。此操作不可撤销。",

  "done.disabled": "{email} 已停用",
  "done.enabled": "{email} 已恢复",
  "done.roleSet": "{email} 的角色已更新",
  "done.deleted": "{email} 已删除",
  "done.selfDisabled": "服务端拒绝停用当前登录的管理员",
};

const en: Dict = {
  "app.title": "Cloud Admin Console",
  "app.accounts": "Accounts",
  "app.logout": "Sign out",
  "app.signedInAs": "Signed in as",

  "common.cancel": "Cancel",
  "common.confirm": "Confirm",
  "common.save": "Save",
  "common.close": "Close",
  "common.search": "Search",
  "common.retry": "Retry",
  "common.refresh": "Refresh",
  "common.loading": "Loading…",
  "common.created": "Created",
  "common.yes": "Yes",
  "common.no": "No",
  "common.unknownError": "Operation failed",

  "login.title": "Administrator sign in",
  "login.subtitle": "Sign in with an account that holds the admin role.",
  "login.email": "Email",
  "login.password": "Password",
  "login.submit": "Sign in",
  "login.signingIn": "Signing in…",
  "login.emailRequired": "Email is required",
  "login.emailInvalid": "Invalid email address",
  "login.passwordRequired": "Password is required",
  "login.showPassword": "Show password",
  "login.hidePassword": "Hide password",
  "login.endpointHint": "Endpoint: {endpoint}",
  "login.notConfigured": "No administrator is configured on the server. Set SUPERTASK_ADMIN_EMAIL and SUPERTASK_ADMIN_PASSWORD, then restart.",
  "login.notAdmin": "This account does not hold the admin role, so it cannot sign in to the console.",
  "login.authFailed": "Wrong email or password.",
  "login.notConfiguredTitle": "Admin surface disabled",

  "accounts.title": "Accounts",
  "accounts.subtitle": "Create cloud accounts, change roles, disable or restore sign-in, reset passwords and delete accounts.",
  "accounts.searchPlaceholder": "Search by email",
  "accounts.new": "New account",
  "accounts.empty": "No accounts match.",
  "accounts.loadFailed": "Failed to load accounts",
  "accounts.count": "{n} accounts",
  "accounts.usage": "{entities} entities · {bytes}",
  "accounts.self": "(signed in)",
  "accounts.selfHint": "This action is not allowed on the signed-in administrator",

  "col.email": "Email",
  "col.role": "Role",
  "col.status": "Status",
  "col.usage": "Entities / usage",
  "col.created": "Created",
  "col.actions": "Actions",

  "role.admin": "Admin",
  "role.user": "User",

  "state.enabled": "Enabled",
  "state.disabled": "Disabled",

  "action.enable": "Restore",
  "action.disable": "Disable",
  "action.promote": "Make admin",
  "action.demote": "Revoke admin",
  "action.setPassword": "Reset password",
  "action.delete": "Delete",

  "create.title": "New account",
  "create.email": "Email",
  "create.password": "Initial password",
  "create.role": "Role",
  "create.hint": "At least 12 characters. The server stores an Argon2id salted hash.",
  "create.submit": "Create account",
  "create.creating": "Creating…",
  "create.created": "Account {email} created",
  "create.emailInvalid": "Invalid email address",
  "create.passwordTooShort": "Password must be at least 12 characters",

  "password.title": "Reset password",
  "password.for": "Set a new password for {email}",
  "password.new": "New password",
  "password.hint": "At least 12 characters. Existing sessions stay valid until their tokens expire.",
  "password.saved": "Password updated",

  "confirm.disable.title": "Disable account",
  "confirm.disable.body": "{email} cannot sign in immediately; cloud data is kept and the account can be restored later.",
  "confirm.enable.title": "Restore account",
  "confirm.enable.body": "{email} will be able to sign in again.",
  "confirm.demote.title": "Revoke admin role",
  "confirm.demote.body": "{email} loses console access. Synced cloud data is unaffected.",
  "confirm.promote.title": "Grant admin role",
  "confirm.promote.body": "{email} will be able to sign in to the console and manage every account. Make sure this person is trusted.",
  "confirm.delete.title": "Delete account",
  "confirm.delete.body": "Permanently deletes {email} with its {entities} entities, all session tokens and telemetry rows. This cannot be undone.",

  "done.disabled": "{email} disabled",
  "done.enabled": "{email} restored",
  "done.roleSet": "Role updated for {email}",
  "done.deleted": "{email} deleted",
  "done.selfDisabled": "The server refuses to disable the signed-in administrator",
};

const dictionaries: Record<string, Dict> = { zh, en };

function pickLocale(): string {
  const preferred = typeof navigator === "undefined" ? "zh" : navigator.language.toLowerCase();
  return preferred.startsWith("zh") ? "zh" : "en";
}

let active = pickLocale();

export function setLocale(locale: string) {
  active = locale.startsWith("zh") ? "zh" : "en";
}

export function currentLocale(): string {
  return active;
}

/** Translate a key with `{var}` interpolation; falls back to English, then the raw key. */
export function t(key: string, vars?: Record<string, string | number>): string {
  const raw = dictionaries[active][key] ?? dictionaries.en[key] ?? key;
  if (!vars) return raw;
  return raw.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in vars ? String(vars[name]) : match,
  );
}

export const MIN_PASSWORD_CHARS = 12;
