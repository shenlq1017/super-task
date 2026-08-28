import { useCallback, useEffect, useRef, useState } from "react";
import { FolderSearch, GitBranch, Loader2, RefreshCw, TriangleAlert } from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { cn } from "@/lib/utils";
import { useToast } from "@/components/ui/toast";
import { apiGitClone, apiGitPull, apiGitStatus } from "../ipc/api";
import { isTauri } from "../ipc/invoke";
import { IpcFailure, type GitStatus, type OpState } from "../ipc/protocol";
import { operationResultWorkspaceId, useOperations, type OperationState } from "../providers/operation-provider";
import { fmtTime, opErrorLabel } from "../lib/status";
import { useOpenWorkspace } from "../lib/use-open-workspace";
import { useWorkspace } from "../providers/workspace-provider";

const OP_STATE_LABEL: Record<OpState, string> = {
  queued: "排队中",
  running: "进行中",
  succeeded: "已完成",
  failed: "失败",
};

const OP_STATE_COLOR: Record<OpState, string> = {
  queued: "var(--t3,#8a8f98)",
  running: "var(--st-accent,#5e6ad2)",
  succeeded: "var(--st-ok-deep,#1e7e35)",
  failed: "var(--st-danger,#dc2626)",
};

/** 单层目录名校验（clone 目标目录名，与模板页同一套规则）。 */
function validateDirectoryName(name: string): string | null {
  const n = name.trim();
  if (!n) return "请输入目标目录名";
  if (n === "." || n === "..") return "不允许使用 . 或 ..";
  if (/[/\\]/.test(n)) return "目录名必须是单层目录，不能包含 / 或 \\";
  if (n.includes(":")) return "目录名不能包含盘符分隔符 :";
  return null;
}

function joinPath(parent: string, name: string): string {
  const p = parent.trim().replace(/[\\/]+$/, "");
  const sep = p.includes("\\") ? "\\" : "/";
  return p ? `${p}${sep}${name}` : name;
}

/** 从仓库 URL 猜测目标目录名（去掉 .git 后缀）。 */
function dirNameFromUrl(url: string): string {
  const seg = url.trim().replace(/\/+$/, "").split(/[\\/]/).filter(Boolean).pop() ?? "";
  return seg.replace(/\.git$/i, "");
}

function StatusRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-baseline gap-3 py-1.5">
      <span className="w-24 shrink-0 text-[0.72rem] font-medium text-[var(--t3,#8a8f98)]">{label}</span>
      <span className="min-w-0 text-[0.8rem] text-[var(--t1,#222326)]">{children}</span>
    </div>
  );
}

/** 长操作进度卡片（与模板页同构：state 中文 + message + 条件进度条）。 */
function GitOperationCard({ op }: { op: OperationState }) {
  return (
    <Card
      className={cn(
        "p-4",
        op.state === "failed" && "border-red-200 bg-[var(--st-danger-tint,#fdecec)]",
        op.state === "succeeded" && "border-[rgb(39_166_68_/_0.35)]",
      )}
      role="status"
    >
      <div className="flex items-center gap-2">
        {op.state === "running" ? (
          <Loader2 className="size-3.5 animate-spin text-[var(--st-accent,#5e6ad2)]" />
        ) : (
          <span className="size-2 shrink-0 rounded-full" style={{ background: OP_STATE_COLOR[op.state] }} />
        )}
        <span className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">
          {op.kind === "git.clone" ? "克隆仓库" : "拉取更新"}
        </span>
        <Badge variant={op.state === "failed" ? "destructive" : "soon"} className="shrink-0">
          {OP_STATE_LABEL[op.state]}
        </Badge>
        <span className="truncate font-mono text-[0.62rem] text-[var(--t3,#8a8f98)]" title={op.operation_id}>
          {op.operation_id}
        </span>
      </div>
      {op.message ? (
        <div className="mt-1.5 text-[0.78rem] text-[var(--t2,#62666d)]">{op.message}</div>
      ) : null}
      {op.progress != null ? (
        <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-[var(--surface-2,#f3f4f5)]">
          <div
            className="h-full rounded-full bg-[var(--st-accent,#5e6ad2)] transition-all duration-300"
            style={{ width: `${Math.round(Math.min(1, Math.max(0, op.progress)) * 100)}%` }}
          />
        </div>
      ) : null}
      {op.state === "failed" ? (
        <div className="mt-2 text-[0.78rem] leading-relaxed text-[#DC2626]" role="alert">
          {opErrorLabel(op.error_code)}
        </div>
      ) : null}
    </Card>
  );
}

/** 无工作区：clone 入口。 */
function CloneEntry() {
  const { toast } = useToast();
  const openWs = useOpenWorkspace();
  const { get } = useOperations();

  const [url, setUrl] = useState("");
  const [parentPath, setParentPath] = useState("");
  const [dirName, setDirName] = useState("");
  const [dirNameTouched, setDirNameTouched] = useState(false);
  const [branch, setBranch] = useState("");
  const [dirNameError, setDirNameError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const [activeOpId, setActiveOpId] = useState<string | null>(null);
  const op = activeOpId ? get(activeOpId) : null;

  const handledOpRef = useRef<string | null>(null);
  useEffect(() => {
    if (!op || !activeOpId || op.state !== "succeeded" || handledOpRef.current === activeOpId) return;
    handledOpRef.current = activeOpId;
    const wsId = operationResultWorkspaceId(op);
    if (wsId) {
      toast(`仓库已克隆：${wsId.split(/[\\/]/).filter(Boolean).pop() ?? wsId}`, "ok");
      void openWs(wsId);
    }
  }, [op, activeOpId, openWs, toast]);

  const pickParentDirectory = async () => {
    if (isTauri()) {
      try {
        const selected = await openDialog({ directory: true, multiple: false });
        if (typeof selected === "string") {
          setParentPath(selected);
          return;
        }
        return;
      } catch {
        // 插件不可用时降级为手动输入
      }
    }
    const p = window.prompt("输入目标父目录路径", parentPath);
    if (p) setParentPath(p);
  };

  const effectiveDirName = dirNameTouched ? dirName : dirName || dirNameFromUrl(url);
  const opRunning = !!op && (op.state === "queued" || op.state === "running");
  const targetPath = effectiveDirName.trim() && parentPath.trim() ? joinPath(parentPath, effectiveDirName) : "";

  const submit = async () => {
    if (submitting || opRunning) return;
    if (!url.trim()) {
      toast("请输入仓库 URL", "warn");
      return;
    }
    const invalid = validateDirectoryName(effectiveDirName);
    setDirNameError(invalid);
    if (invalid || !parentPath.trim()) {
      if (!parentPath.trim()) toast("请先选择或填写目标父目录", "warn");
      return;
    }
    setSubmitting(true);
    try {
      const { operation_id } = await apiGitClone(url.trim(), targetPath, branch.trim() || null);
      handledOpRef.current = null;
      setActiveOpId(operation_id);
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <>
      <Card className="p-4">
        <div className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">克隆远程仓库为新工作区</div>
        <p className="mt-1 text-[0.74rem] text-[var(--t3,#8a8f98)]">
          克隆完成后会自动扫描项目结构；认证交给 Git Credential Manager，URL 中不要内嵌账号密码。
        </p>
        <label className="mt-3 flex flex-col gap-1">
          <span className="text-[0.72rem] font-medium text-[var(--t2,#62666d)]">仓库 URL</span>
          <Input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="例如 https://github.com/user/repo.git" />
        </label>
        <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-[1fr_auto]">
          <label className="flex flex-col gap-1">
            <span className="text-[0.72rem] font-medium text-[var(--t2,#62666d)]">目标父目录</span>
            <Input
              value={parentPath}
              onChange={(e) => setParentPath(e.target.value)}
              placeholder="例如 C:\project\github"
            />
          </label>
          <div className="flex items-end">
            <Button variant="outline" size="default" className="gap-1" onClick={() => void pickParentDirectory()}>
              <FolderSearch /> 选择目录…
            </Button>
          </div>
        </div>
        <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-2">
          <label className="flex flex-col gap-1">
            <span className="text-[0.72rem] font-medium text-[var(--t2,#62666d)]">目标目录名（单层）</span>
            <Input
              value={effectiveDirName}
              onChange={(e) => {
                setDirNameTouched(true);
                setDirName(e.target.value);
                if (dirNameError) setDirNameError(validateDirectoryName(e.target.value));
              }}
              aria-invalid={!!dirNameError}
              placeholder="留空则按 URL 推断"
            />
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-[0.72rem] font-medium text-[var(--t2,#62666d)]">分支（可选）</span>
            <Input value={branch} onChange={(e) => setBranch(e.target.value)} placeholder="默认远端分支" />
          </label>
        </div>
        {dirNameError ? (
          <div className="mt-1.5 text-[0.74rem] text-[#DC2626]" role="alert">
            {dirNameError}
          </div>
        ) : null}
        <div className="mt-4 flex items-center justify-between gap-3">
          <span className="truncate font-mono text-[0.68rem] text-[var(--t3,#8a8f98)]" title={targetPath}>
            {targetPath ? `目标：${targetPath}` : ""}
          </span>
          <Button onClick={() => void submit()} disabled={submitting || opRunning || !url.trim()}>
            开始克隆
          </Button>
        </div>
      </Card>
      {op ? <GitOperationCard op={op} /> : null}
    </>
  );
}

/** 有工作区：git.status 展示 + pull。 */
function WorkspaceGitView({ workspaceId }: { workspaceId: string }) {
  const { toast } = useToast();
  const { get } = useOperations();

  const [status, setStatus] = useState<GitStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lastRefresh, setLastRefresh] = useState<number | null>(null);

  const [activeOpId, setActiveOpId] = useState<string | null>(null);
  const op = activeOpId ? get(activeOpId) : null;
  const opRunning = !!op && (op.state === "queued" || op.state === "running");

  const [confirmDirtyPull, setConfirmDirtyPull] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const st = await apiGitStatus(workspaceId);
      setStatus(st);
      setLastRefresh(Date.now());
    } catch (e) {
      setStatus(null);
      setError(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e));
    } finally {
      setLoading(false);
    }
  }, [workspaceId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // pull 成功后自动刷新状态（终态只处理一次）
  const handledOpRef = useRef<string | null>(null);
  useEffect(() => {
    if (!op || !activeOpId || op.state !== "succeeded" || handledOpRef.current === activeOpId) return;
    handledOpRef.current = activeOpId;
    toast("拉取完成，已刷新状态", "ok");
    void refresh();
  }, [op, activeOpId, refresh, toast]);

  const startPull = async (allowDirty: boolean) => {
    if (opRunning) return;
    setError(null);
    try {
      const { operation_id } = await apiGitPull(workspaceId, { allowDirty });
      handledOpRef.current = null;
      setActiveOpId(operation_id);
    } catch (e) {
      // 同步拒绝（GIT_DIRTY / GIT_WORKSPACE_BUSY / NO_WORKSPACE）：给下一步提示
      setError(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e));
    }
  };

  const dirty = status?.dirty ?? false;
  const dirtySummary = status
    ? [
        status.staged > 0 ? `暂存 ${status.staged} 项` : null,
        status.unstaged > 0 ? `未暂存 ${status.unstaged} 项` : null,
        status.untracked > 0 ? `未跟踪 ${status.untracked} 项` : null,
      ]
        .filter(Boolean)
        .join("、")
    : "";

  if (error && !status) {
    return (
      <Card className="border-red-200 bg-[var(--st-danger-tint,#fdecec)] p-4" role="alert">
        <div className="flex items-start gap-2 text-[0.8rem] text-[#DC2626]">
          <TriangleAlert className="mt-0.5 size-4 shrink-0" />
          <span>{error}</span>
        </div>
      </Card>
    );
  }

  if (!status) {
    return (
      <div className="flex items-center justify-center gap-2 py-12 text-[0.8rem] text-[var(--t3,#8a8f98)]" role="status">
        <Loader2 className="size-4 animate-spin" /> 正在检测仓库状态…
      </div>
    );
  }

  if (!status.is_repository) {
    return (
      <Card className="p-6 text-center" role="status">
        <GitBranch className="mx-auto size-8 text-[var(--line-strong,#d0d6e0)]" />
        <div className="mt-2 text-[0.9rem] font-semibold text-[var(--t1,#222326)]">当前工作区不是 Git 仓库</div>
        <div className="mt-1 text-[0.78rem] text-[var(--t3,#8a8f98)]">
          目录里没有检测到 .git。可以先在 IDE 或终端执行 git init，再回到本页刷新。
        </div>
      </Card>
    );
  }

  return (
    <>
      <Card className="p-4">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <GitBranch className="size-4 shrink-0 text-[var(--st-accent,#5e6ad2)]" />
            <span className="text-[0.9rem] font-semibold text-[var(--t1,#222326)]">仓库状态</span>
            {status.detached || !status.branch ? (
              <Badge variant="outline" className="shrink-0">detached HEAD</Badge>
            ) : (
              <Badge variant="soon" className="shrink-0">{status.branch}</Badge>
            )}
            {dirty ? (
              <Badge variant="destructive" className="shrink-0">有未提交修改</Badge>
            ) : (
              <Badge variant="secondary" className="shrink-0">工作区干净</Badge>
            )}
          </div>
          <div className="flex items-center gap-2">
            {lastRefresh ? (
              <span className="font-mono text-[0.66rem] text-[var(--t3,#8a8f98)]">刷新于 {fmtTime(lastRefresh)}</span>
            ) : null}
            <Button variant="outline" size="sm" className="gap-1" onClick={() => void refresh()} disabled={loading}>
              <RefreshCw className={cn(loading && "animate-spin")} /> 刷新
            </Button>
          </div>
        </div>

        <Separator className="my-3" />

        <div className="grid grid-cols-1 md:grid-cols-2">
          <div>
            <StatusRow label="分支">
              {status.detached || !status.branch ? (
                <span className="text-[var(--t2,#62666d)]">游离检出（不跟踪任何分支）</span>
              ) : (
                <span className="font-mono text-[0.78rem]">{status.branch}</span>
              )}
            </StatusRow>
            <StatusRow label="远端">
              {status.remote ? (
                <span className="font-mono text-[0.78rem]">{status.remote}</span>
              ) : (
                <span className="text-[var(--t3,#8a8f98)]">未配置</span>
              )}
            </StatusRow>
            <StatusRow label="领先 / 落后">
              <span className="font-mono text-[0.78rem]">
                ↑ {status.ahead} · ↓ {status.behind}
              </span>
              {status.ahead === 0 && status.behind === 0 ? (
                <span className="ml-2 text-[0.72rem] text-[var(--t3,#8a8f98)]">与远端一致</span>
              ) : null}
            </StatusRow>
          </div>
          <div>
            <StatusRow label="变更">
              <span className="font-mono text-[0.78rem]">
                暂存 {status.staged} · 未暂存 {status.unstaged} · 未跟踪 {status.untracked}
              </span>
            </StatusRow>
            <StatusRow label="拉取前检查">
              {dirty ? (
                <span className="text-[0.76rem] text-[#B7791F]">有未提交修改，默认禁止拉取</span>
              ) : (
                <span className="text-[0.76rem] text-[var(--st-ok-deep,#1e7e35)]">可以安全拉取</span>
              )}
            </StatusRow>
          </div>
        </div>

        <Separator className="my-3" />

        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="text-[0.74rem] text-[var(--t3,#8a8f98)]">
            {dirty ? "提交或暂存修改后即可正常拉取；也可以选择保留现场强行拉取。" : "拉取会快进到远端最新提交。"}
          </span>
          <div className="flex items-center gap-2">
            {dirty ? (
              <>
                <Button variant="outline" size="sm" disabled={opRunning} title="有未提交修改，普通拉取被禁止">
                  拉取
                </Button>
                <Button variant="secondary" size="sm" className="gap-1" disabled={opRunning} onClick={() => setConfirmDirtyPull(true)}>
                  仍然拉取…
                </Button>
              </>
            ) : (
              <Button size="sm" disabled={opRunning} onClick={() => void startPull(false)}>
                拉取
              </Button>
            )}
          </div>
        </div>

        {error ? (
          <div className="mt-3 rounded-lg border border-red-200 bg-[var(--st-danger-tint,#fdecec)] px-3 py-2 text-[0.76rem] text-[#DC2626]" role="alert">
            {error}
          </div>
        ) : null}
      </Card>

      {op ? <GitOperationCard op={op} /> : null}

      <ConfirmDialog
        open={confirmDirtyPull}
        title="仍然拉取？"
        description={`当前工作区有未提交修改（${dirtySummary || "有本地改动"}）。\n带 allow_dirty 拉取可能产生冲突；冲突后 SuperTask 会保留现场，需要你用 IDE 手动处理，不会自动恢复。`}
        confirmText="仍然拉取"
        cancelText="取消"
        destructive
        onConfirm={() => {
          setConfirmDirtyPull(false);
          void startPull(true);
        }}
        onCancel={() => setConfirmDirtyPull(false)}
      />
    </>
  );
}

export function GitPage() {
  const ws = useWorkspace();
  const workspaceId = ws.state.workspaceId;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto flex max-w-4xl flex-col gap-4">
          <div>
            <h2 className="text-[1.05rem] font-bold tracking-tight text-[var(--t1,#222326)]">Git</h2>
            <p className="mt-0.5 text-[0.78rem] text-[var(--t3,#8a8f98)]">
              {workspaceId
                ? "查看当前工作区的仓库状态并拉取远端更新；提交、推送、冲突处理请使用你惯用的 IDE 或 Git 工具。"
                : "当前没有打开的工作区。可以先克隆一个远程仓库，克隆完成后自动打开为新工作区。"}
            </p>
          </div>

          {workspaceId ? <WorkspaceGitView workspaceId={workspaceId} /> : <CloneEntry />}
        </div>
      </div>
    </div>
  );
}
