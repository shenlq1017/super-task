import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { cn } from "@/lib/utils";
import { useSession } from "../providers/session-provider";
import { useRuntime } from "../providers/runtime-provider";
import { useOperations } from "../providers/operation-provider";
import { apiSavePrefs, apiUpdateCheck, apiUpdateInstall } from "../ipc/api";
import { IpcFailure } from "../ipc/protocol";
import type { UpdateCheckResult } from "../ipc/protocol";
import { opErrorLabel } from "../lib/status";
import { useToast } from "@/components/ui/toast";

/** 偏好开关行：与既有 checkbox 样式一致，说明文案放第二行。 */
function PrefRow({
  label,
  desc,
  checked,
  onChange,
}: {
  label: string;
  desc: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="flex items-center justify-between gap-4 py-1.5 text-[0.875rem] text-[var(--t1,#222326)]">
      <span>
        {label}
        <span className="block text-[0.75rem] leading-relaxed text-[var(--t3,#8a8f98)]">{desc}</span>
      </span>
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        aria-label={label}
        className="shrink-0"
      />
    </label>
  );
}

const INSTALL_BLOCKED_HINT = "无法安装：有服务运行中或后台任务进行中。请先停止全部服务，再重新安装。";

/** 更新区块：检查更新（长操作）→ 结果 → 确认安装。 */
function UpdateCard() {
  const { state } = useSession();
  const { get } = useOperations();
  const runtime = useRuntime();
  const { toast } = useToast();

  const [checkOpId, setCheckOpId] = useState<string | null>(null);
  const [installOpId, setInstallOpId] = useState<string | null>(null);
  const [confirmInstall, setConfirmInstall] = useState(false);
  const [blocked, setBlocked] = useState<string | null>(null);

  const checkOp = checkOpId ? get(checkOpId) : null;
  const installOp = installOpId ? get(installOpId) : null;

  const checkRunning = !!checkOp && (checkOp.state === "queued" || checkOp.state === "running");
  const installRunning = !!installOp && (installOp.state === "queued" || installOp.state === "running");

  const available: UpdateCheckResult | null = (() => {
    if (!checkOp || checkOp.state !== "succeeded") return null;
    const r = checkOp.result as UpdateCheckResult | null;
    if (!r || typeof r !== "object" || r.status !== "available") return null;
    return r;
  })();

  const startCheck = async () => {
    setBlocked(null);
    try {
      const { operation_id } = await apiUpdateCheck();
      setCheckOpId(operation_id);
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  const startInstall = async (version: string) => {
    setBlocked(null);
    try {
      const { operation_id } = await apiUpdateInstall(version);
      setInstallOpId(operation_id);
    } catch (e) {
      // 同步拒绝（UPDATE_BLOCKED_RUNNING / UPDATE_FAILED）：给出原因与下一步
      if (e instanceof IpcFailure && e.code === "UPDATE_BLOCKED_RUNNING") {
        setBlocked(INSTALL_BLOCKED_HINT);
      } else {
        toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
      }
    }
  };

  // 安装 operation 失败：UPDATE_BLOCKED_RUNNING → 显示原因 + 停止全部快捷键
  useEffect(() => {
    if (installOp?.state === "failed" && installOp.error_code === "UPDATE_BLOCKED_RUNNING") {
      setBlocked(INSTALL_BLOCKED_HINT);
    }
  }, [installOp]);

  return (
    <Card className="p-4">
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-[0.875rem] font-semibold text-[var(--t1,#222326)]">更新</h3>
        <span className="font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]">
          当前版本 {state.hello?.product_version ?? "—"}
        </span>
      </div>
      <p className="mt-1 text-[0.75rem] leading-relaxed text-[var(--t3,#8a8f98)]">
        安装前会自动阻止：有服务运行中或后台任务进行时无法安装。安装需用户确认，发现更新只提示。
      </p>

      <div className="mt-3 flex items-center gap-2">
        <Button size="sm" onClick={() => void startCheck()} disabled={checkRunning || installRunning}>
          检查更新
        </Button>
        {checkRunning ? (
          <span className="flex items-center gap-1.5 text-[0.76rem] text-[var(--t2,#62666d)]" role="status">
            <Loader2 className="size-3.5 animate-spin text-[var(--st-accent,#5e6ad2)]" />
            {checkOp.message ?? "正在检查更新…"}
          </span>
        ) : null}
      </div>

      {checkOp?.state === "failed" ? (
        <div className="mt-2.5 rounded-lg border border-red-200 bg-[var(--st-danger-tint,#fdecec)] px-3 py-2 text-[0.78rem] text-[#DC2626]" role="alert">
          {opErrorLabel(checkOp.error_code)}
        </div>
      ) : null}

      {checkOp?.state === "succeeded" && !available ? (
        <div className="mt-2.5 flex items-center gap-2 text-[0.8rem] text-[var(--st-ok-deep,#1e7e35)]" role="status">
          <span className="size-1.5 rounded-full bg-[var(--st-ok,#27a644)]" />
          已是最新
        </div>
      ) : null}

      {available ? (
        <div className="mt-3 rounded-[var(--r-md,12px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] p-3">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">发现新版本</span>
            <span className="font-mono text-[0.8rem] text-[var(--st-accent,#5e6ad2)]">{available.version}</span>
            {available.date ? (
              <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">发布于 {available.date}</span>
            ) : null}
            <Button
              size="sm"
              className="ml-auto"
              disabled={installRunning || checkRunning}
              onClick={() => setConfirmInstall(true)}
            >
              安装更新
            </Button>
          </div>
          {available.notes ? (
            <p className="mt-1.5 whitespace-pre-wrap break-words text-[0.76rem] leading-relaxed text-[var(--t2,#62666d)]">
              {available.notes}
            </p>
          ) : null}
        </div>
      ) : null}

      {installRunning ? (
        <div className="mt-3 rounded-[var(--r-md,12px)] border border-[rgb(94_106_210_/_0.35)] p-3" role="status">
          <div className="flex items-center gap-2 text-[0.8rem] text-[var(--t1,#222326)]">
            <Loader2 className="size-3.5 animate-spin text-[var(--st-accent,#5e6ad2)]" />
            {installOp.message ?? "正在处理更新…"}
          </div>
          {/* progress 可能为 null：只显示状态文案，不伪造百分比 */}
          {installOp.progress != null ? (
            <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-[var(--surface-2,#f3f4f5)]">
              <div
                className="h-full rounded-full bg-[var(--st-accent,#5e6ad2)] transition-all duration-300"
                style={{ width: `${Math.round(Math.min(1, Math.max(0, installOp.progress)) * 100)}%` }}
              />
            </div>
          ) : null}
        </div>
      ) : null}

      {blocked ? (
        <div
          className={cn(
            "mt-3 flex flex-wrap items-center gap-2 rounded-[var(--r-md,12px)] border px-3 py-2 text-[0.78rem]",
            "border-[#f0d58a] bg-[#fdf6e3] text-[#B7791F]",
          )}
          role="alert"
        >
          <span className="min-w-0 flex-1">{blocked}</span>
          <Button variant="outline" size="sm" onClick={() => void runtime.actions.stopAll()}>
            停止全部
          </Button>
        </div>
      ) : null}

      <ConfirmDialog
        open={confirmInstall}
        title={`安装更新 ${available?.version ?? ""}？`}
        description={
          "安装过程中 SuperTask 将退出，由安装器接管完成升级。\n有服务运行中或后台任务进行时会自动阻止安装。"
        }
        confirmText="安装更新"
        cancelText="取消"
        onConfirm={() => {
          setConfirmInstall(false);
          if (available?.version) void startInstall(available.version);
        }}
        onCancel={() => setConfirmInstall(false)}
      />
    </Card>
  );
}

export function SettingsPage() {
  const { state } = useSession();
  const { toast } = useToast();
  const [theme, setTheme] = useState(state.app?.prefs.theme ?? "light");
  const [restore, setRestore] = useState(state.app?.prefs.restoreLast ?? true);
  const [closeToTray, setCloseToTray] = useState(state.app?.prefs.closeToTray ?? true);
  const [startOnLogin, setStartOnLogin] = useState(state.app?.prefs.startOnLogin ?? false);
  const [updateCheck, setUpdateCheck] = useState(state.app?.prefs.updateCheck ?? true);

  useEffect(() => {
    setTheme(state.app?.prefs.theme ?? "light");
    setRestore(state.app?.prefs.restoreLast ?? true);
    setCloseToTray(state.app?.prefs.closeToTray ?? true);
    setStartOnLogin(state.app?.prefs.startOnLogin ?? false);
    setUpdateCheck(state.app?.prefs.updateCheck ?? true);
  }, [state.app]);

  const save = async () => {
    try {
      await apiSavePrefs({ theme, restoreLast: restore, closeToTray, startOnLogin, updateCheck });
      toast("偏好已保存", "ok");
    } catch (e) {
      if (e instanceof IpcFailure && e.code === "AUTOSTART_FAILED") {
        // 后端已回滚偏好为 false；UI 开关同步回滚为未开启
        setStartOnLogin(false);
      }
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto flex max-w-2xl flex-col gap-6">
          <Card className="p-4">
            <h3 className="mb-3 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">常规</h3>
            <div className="flex flex-col divide-y divide-[var(--line,#e6e6e6)]">
              <PrefRow
                label="恢复上次工作区"
                desc="启动时自动打开最近使用的工作区"
                checked={restore}
                onChange={setRestore}
              />
              <PrefRow
                label="关闭窗口时隐藏到托盘"
                desc="点关闭按钮时最小化到托盘而不是退出；可从托盘菜单退出 SuperTask"
                checked={closeToTray}
                onChange={setCloseToTray}
              />
              <PrefRow
                label="开机自动启动 SuperTask"
                desc="登录 Windows 后自动启动（仅启动 SuperTask，不自动启动项目服务）"
                checked={startOnLogin}
                onChange={setStartOnLogin}
              />
              <PrefRow
                label="自动检查更新"
                desc="启动后联网检查新版本；发现更新只提示，安装需确认"
                checked={updateCheck}
                onChange={setUpdateCheck}
              />
            </div>
            <div className="mt-3 flex justify-end">
              <Button size="sm" onClick={() => void save()}>
                保存
              </Button>
            </div>
          </Card>

          <Card className="p-4">
            <h3 className="mb-3 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">外观</h3>
            <div className="flex gap-2">
              {(["light", "dark"] as const).map((t) => (
                <button
                  key={t}
                  disabled={t === "dark"}
                  onClick={() => setTheme(t)}
                  className={cn(
                    "flex-1 rounded-[var(--r-sm,8px)] border px-3 py-2 text-[0.875rem] transition-colors duration-150",
                    theme === t
                      ? "border-[var(--st-accent,#5e6ad2)] bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent-hover,#4f5ac8)]"
                      : "border-[var(--line,#e6e6e6)] text-[var(--t2,#62666d)] hover:border-[var(--line-strong,#d0d6e0)]",
                    t === "dark" && "opacity-50",
                  )}
                >
                  {t === "light" ? "浅色" : "深色（即将）"}
                </button>
              ))}
            </div>
          </Card>

          <UpdateCard />

          <Card className="p-4">
            <h3 className="mb-3 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">关于</h3>
            <dl className="grid grid-cols-[120px_1fr] gap-y-1.5 text-[0.875rem]">
              <dt className="text-[var(--t3,#8a8f98)]">产品版本</dt>
              <dd className="font-mono text-[var(--t1,#222326)]">{state.hello?.product_version}</dd>
              <dt className="text-[var(--t3,#8a8f98)]">引擎</dt>
              <dd className="font-mono text-[var(--t1,#222326)]">
                {state.hello?.engine} {state.hello?.engine_version}
              </dd>
              <dt className="text-[var(--t3,#8a8f98)]">协议</dt>
              <dd className="font-mono text-[var(--t1,#222326)]">{state.hello?.protocol}</dd>
              <dt className="text-[var(--t3,#8a8f98)]">系统</dt>
              <dd className="font-mono text-[var(--t1,#222326)]">{state.hello?.os}</dd>
            </dl>
          </Card>
        </div>
      </div>
    </div>
  );
}
