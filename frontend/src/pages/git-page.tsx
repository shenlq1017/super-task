import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
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

/** operation 状态文案走 `pages.git.op_*`；颜色本地保留。 */
const OP_STATE_COLOR: Record<OpState, string> = {
  queued: "var(--t3,#8a8f98)",
  running: "var(--st-accent,#5e6ad2)",
  succeeded: "var(--st-ok-deep,#1e7e35)",
  failed: "var(--st-danger,#dc2626)",
  cancelled: "var(--t3,#8a8f98)",
};

/** 单层目录名校验（clone 目标目录名，与模板页同一套规则）。返回 i18n key，null = 合法。 */
function validateDirectoryName(name: string): string | null {
  const n = name.trim();
  if (!n) return "pages.git.dirErrEmpty";
  if (n === "." || n === "..") return "pages.git.dirErrDot";
  if (/[/\\]/.test(n)) return "pages.git.dirErrSep";
  if (n.includes(":")) return "pages.git.dirErrDrive";
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

/** 长操作进度卡片（与模板页同构：state 本地化 + message + 条件进度条）。 */
function GitOperationCard({ op }: { op: OperationState }) {
  const { t } = useTranslation();
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
          {op.kind === "git.clone" ? t("pages.git.cloneTitle") : t("pages.git.pullTitle")}
        </span>
        <Badge variant={op.state === "failed" ? "destructive" : "soon"} className="shrink-0">
          {t(`pages.git.op_${op.state}`)}
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
  const { t } = useTranslation();
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
      toast(t("pages.git.clonedTo", { name: wsId.split(/[\\/]/).filter(Boolean).pop() ?? wsId }), "ok");
      void openWs(wsId);
    }
  }, [op, activeOpId, openWs, toast, t]);

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
    const p = window.prompt(t("pages.git.promptParent"), parentPath);
    if (p) setParentPath(p);
  };

  const effectiveDirName = dirNameTouched ? dirName : dirName || dirNameFromUrl(url);
  const opRunning = !!op && (op.state === "queued" || op.state === "running");
  const targetPath = effectiveDirName.trim() && parentPath.trim() ? joinPath(parentPath, effectiveDirName) : "";

  const submit = async () => {
    if (submitting || opRunning) return;
    if (!url.trim()) {
      toast(t("pages.git.urlRequired"), "warn");
      return;
    }
    const invalid = validateDirectoryName(effectiveDirName);
    setDirNameError(invalid);
    if (invalid || !parentPath.trim()) {
      if (!parentPath.trim()) toast(t("pages.git.parentRequired"), "warn");
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
        <div className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">{t("pages.git.cloneHeading")}</div>
        <p className="mt-1 text-[0.74rem] text-[var(--t3,#8a8f98)]">
          {t("pages.git.cloneDesc")}
        </p>
        <label className="mt-3 flex flex-col gap-1">
          <span className="text-[0.72rem] font-medium text-[var(--t2,#62666d)]">{t("pages.git.repoUrl")}</span>
          <Input value={url} onChange={(e) => setUrl(e.target.value)} placeholder={t("pages.git.urlPlaceholder")} />
        </label>
        <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-[1fr_auto]">
          <label className="flex flex-col gap-1">
            <span className="text-[0.72rem] font-medium text-[var(--t2,#62666d)]">{t("pages.git.parentDir")}</span>
            <Input
              value={parentPath}
              onChange={(e) => setParentPath(e.target.value)}
              placeholder={t("pages.git.parentDirPlaceholder")}
            />
          </label>
          <div className="flex items-end">
            <Button variant="outline" size="default" className="gap-1" onClick={() => void pickParentDirectory()}>
              <FolderSearch /> {t("pages.git.pickDir")}
            </Button>
          </div>
        </div>
        <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-2">
          <label className="flex flex-col gap-1">
            <span className="text-[0.72rem] font-medium text-[var(--t2,#62666d)]">{t("pages.git.dirName")}</span>
            <Input
              value={effectiveDirName}
              onChange={(e) => {
                setDirNameTouched(true);
                setDirName(e.target.value);
                if (dirNameError) setDirNameError(validateDirectoryName(e.target.value));
              }}
              aria-invalid={!!dirNameError}
              placeholder={t("pages.git.dirNamePlaceholder")}
            />
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-[0.72rem] font-medium text-[var(--t2,#62666d)]">{t("pages.git.branch")}</span>
            <Input value={branch} onChange={(e) => setBranch(e.target.value)} placeholder={t("pages.git.branchPlaceholder")} />
          </label>
        </div>
        {dirNameError ? (
          <div className="mt-1.5 text-[0.74rem] text-[#DC2626]" role="alert">
            {t(dirNameError)}
          </div>
        ) : null}
        <div className="mt-4 flex items-center justify-between gap-3">
          <span className="truncate font-mono text-[0.68rem] text-[var(--t3,#8a8f98)]" title={targetPath}>
            {targetPath ? t("pages.git.target", { path: targetPath }) : ""}
          </span>
          <Button onClick={() => void submit()} disabled={submitting || opRunning || !url.trim()}>
            {t("pages.git.startClone")}
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
  const { t } = useTranslation();
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
    toast(t("pages.git.pullDone"), "ok");
    void refresh();
  }, [op, activeOpId, refresh, toast, t]);

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
        status.staged > 0 ? t("pages.git.stagedCount", { n: status.staged }) : null,
        status.unstaged > 0 ? t("pages.git.unstagedCount", { n: status.unstaged }) : null,
        status.untracked > 0 ? t("pages.git.untrackedCount", { n: status.untracked }) : null,
      ]
        .filter(Boolean)
        .join(t("pages.git.summaryJoin"))
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
        <Loader2 className="size-4 animate-spin" /> {t("pages.git.checkingRepo")}
      </div>
    );
  }

  if (!status.is_repository) {
    return (
      <Card className="p-6 text-center" role="status">
        <GitBranch className="mx-auto size-8 text-[var(--line-strong,#d0d6e0)]" />
        <div className="mt-2 text-[0.9rem] font-semibold text-[var(--t1,#222326)]">{t("pages.git.notRepo")}</div>
        <div className="mt-1 text-[0.78rem] text-[var(--t3,#8a8f98)]">
          {t("pages.git.notRepoDesc")}
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
            <span className="text-[0.9rem] font-semibold text-[var(--t1,#222326)]">{t("pages.git.repoStatus")}</span>
            {status.detached || !status.branch ? (
              <Badge variant="outline" className="shrink-0">detached HEAD</Badge>
            ) : (
              <Badge variant="soon" className="shrink-0">{status.branch}</Badge>
            )}
            {dirty ? (
              <Badge variant="destructive" className="shrink-0">{t("pages.git.dirtyBadge")}</Badge>
            ) : (
              <Badge variant="secondary" className="shrink-0">{t("pages.git.cleanBadge")}</Badge>
            )}
          </div>
          <div className="flex items-center gap-2">
            {lastRefresh ? (
              <span className="font-mono text-[0.66rem] text-[var(--t3,#8a8f98)]">{t("pages.git.refreshedAt", { time: fmtTime(lastRefresh) })}</span>
            ) : null}
            <Button variant="soft" size="sm" className="gap-1" onClick={() => void refresh()} disabled={loading}>
              <RefreshCw className={cn(loading && "animate-spin")} /> {t("common.refresh")}
            </Button>
          </div>
        </div>

        <Separator className="my-3" />

        <div className="grid grid-cols-1 md:grid-cols-2">
          <div>
            <StatusRow label={t("pages.git.branchLabel")}>
              {status.detached || !status.branch ? (
                <span className="text-[var(--t2,#62666d)]">{t("pages.git.detached")}</span>
              ) : (
                <span className="font-mono text-[0.78rem]">{status.branch}</span>
              )}
            </StatusRow>
            <StatusRow label={t("pages.git.remoteLabel")}>
              {status.remote ? (
                <span className="font-mono text-[0.78rem]">{status.remote}</span>
              ) : (
                <span className="text-[var(--t3,#8a8f98)]">{t("pages.git.remoteNone")}</span>
              )}
            </StatusRow>
            <StatusRow label={t("pages.git.aheadBehind")}>
              <span className="font-mono text-[0.78rem]">
                ↑ {status.ahead} · ↓ {status.behind}
              </span>
              {status.ahead === 0 && status.behind === 0 ? (
                <span className="ml-2 text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.git.inSync")}</span>
              ) : null}
            </StatusRow>
          </div>
          <div>
            <StatusRow label={t("pages.git.changesLabel")}>
              <span className="font-mono text-[0.78rem]">
                {t("pages.git.stagedCount", { n: status.staged })} · {t("pages.git.unstagedCount", { n: status.unstaged })} · {t("pages.git.untrackedCount", { n: status.untracked })}
              </span>
            </StatusRow>
            <StatusRow label={t("pages.git.pullCheck")}>
              {dirty ? (
                <span className="text-[0.76rem] text-[#B7791F]">{t("pages.git.dirtyBlockPull")}</span>
              ) : (
                <span className="text-[0.76rem] text-[var(--st-ok-deep,#1e7e35)]">{t("pages.git.safeToPull")}</span>
              )}
            </StatusRow>
          </div>
        </div>

        <Separator className="my-3" />

        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="text-[0.74rem] text-[var(--t3,#8a8f98)]">
            {dirty ? t("pages.git.dirtyPullHint") : t("pages.git.cleanPullHint")}
          </span>
          <div className="flex items-center gap-2">
            {dirty ? (
              <>
                <Button variant="outline" size="sm" disabled={opRunning} title={t("pages.git.pullDisabledTitle")}>
                  {t("pages.git.pull")}
                </Button>
                <Button variant="secondary" size="sm" className="gap-1" disabled={opRunning} onClick={() => setConfirmDirtyPull(true)}>
                  {t("pages.git.forcePull")}
                </Button>
              </>
            ) : (
              <Button size="sm" disabled={opRunning} onClick={() => void startPull(false)}>
                {t("pages.git.pull")}
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
        title={t("pages.git.forcePullConfirmTitle")}
        description={t("pages.git.forcePullConfirmDesc", { summary: dirtySummary || t("pages.git.localChanges") })}
        confirmText={t("pages.git.forcePullShort")}
        cancelText={t("common.cancel")}
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
  const { t } = useTranslation();
  const workspaceId = ws.state.workspaceId;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto flex max-w-4xl flex-col gap-4">
          <div>
            <h2 className="text-[1.05rem] font-bold tracking-tight text-[var(--t1,#222326)]">Git</h2>
            <p className="mt-0.5 text-[0.78rem] text-[var(--t3,#8a8f98)]">
              {workspaceId
                ? t("pages.git.pageDescWithWs")
                : t("pages.git.pageDescNoWs")}
            </p>
          </div>

          {workspaceId ? <WorkspaceGitView workspaceId={workspaceId} /> : <CloneEntry />}
        </div>
      </div>
    </div>
  );
}
