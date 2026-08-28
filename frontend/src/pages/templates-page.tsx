import { useEffect, useRef, useState } from "react";
import { ChevronDown, ChevronRight, FolderSearch, LayoutTemplate, Loader2 } from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { cn } from "@/lib/utils";
import { useToast } from "@/components/ui/toast";
import { apiTemplatesCreate, apiTemplatesList } from "../ipc/api";
import { isTauri } from "../ipc/invoke";
import { IpcFailure, type OpState, type TemplateSummary } from "../ipc/protocol";
import { operationResultWorkspaceId, useOperations, type OperationState } from "../providers/operation-provider";
import { opErrorLabel } from "../lib/status";
import { useOpenWorkspace } from "../lib/use-open-workspace";

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

/** 单层目录名校验（与后端 validate_directory_name 语义对齐的前端预检）。 */
function validateDirectoryName(name: string): string | null {
  const n = name.trim();
  if (!n) return "请输入目录名";
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

/** 长操作进度卡片：state 中文 + message + 有 progress 才显示进度条。 */
function TemplateOperationCard({ op, targetDir }: { op: OperationState; targetDir: string }) {
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
        <span className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">创建工作区</span>
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
        <div className="mt-2 text-[0.78rem] leading-relaxed text-[#DC2626]">
          {opErrorLabel(op.error_code)}
          {targetDir ? <span className="block text-[var(--t2,#62666d)]">目标目录：{targetDir}</span> : null}
        </div>
      ) : null}
    </Card>
  );
}

function TemplateCard({
  template,
  selected,
  onSelect,
}: {
  template: TemplateSummary;
  selected: boolean;
  onSelect: () => void;
}) {
  const [filesOpen, setFilesOpen] = useState(false);
  return (
    <Card
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      onClick={onSelect}
      onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && onSelect()}
      className={cn(
        "cursor-pointer p-4 outline-none transition-all duration-150 focus-visible:outline-2 focus-visible:outline-[var(--st-accent,#5e6ad2)]",
        selected
          ? "border-[var(--st-accent,#5e6ad2)] bg-[var(--st-accent-tint,#eef0fb)]"
          : "hover:-translate-y-px hover:border-[var(--line-strong,#d0d6e0)] hover:shadow-[var(--shadow-2,0_6px_20px_rgb(16_24_40_/_0.09))]",
      )}
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="text-[0.9rem] font-semibold text-[var(--t1,#222326)]">{template.name}</div>
          <div className="mt-1 text-[0.78rem] leading-relaxed text-[var(--t2,#62666d)]">{template.description}</div>
        </div>
        <Badge variant="secondary" className="shrink-0" title="模板版本">
          v{template.version}
        </Badge>
      </div>
      <div className="mt-2.5 flex flex-wrap items-center gap-1.5">
        {template.stacks.map((s) => (
          <Badge key={s} variant="outline" className="shrink-0">
            {s}
          </Badge>
        ))}
      </div>
      <Separator className="my-3" />
      <button
        type="button"
        aria-expanded={filesOpen}
        onClick={(e) => {
          e.stopPropagation();
          setFilesOpen((v) => !v);
        }}
        className="flex items-center gap-1 rounded-[var(--r-sm,8px)] text-[0.75rem] font-medium text-[var(--t2,#62666d)] outline-none transition-colors hover:text-[var(--t1,#222326)] focus-visible:outline-2 focus-visible:outline-[var(--st-accent,#5e6ad2)]"
      >
        {filesOpen ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
        文件概览（{template.files.length}）
      </button>
      {filesOpen ? (
        <ul className="mt-2 max-h-40 space-y-0.5 overflow-auto font-mono text-[0.68rem] text-[var(--t3,#8a8f98)]">
          {template.files.map((f) => (
            <li key={f} className="truncate" title={f}>
              {f}
            </li>
          ))}
        </ul>
      ) : null}
    </Card>
  );
}

export function TemplatesPage() {
  const { toast } = useToast();
  const openWs = useOpenWorkspace();
  const { get } = useOperations();

  const [templates, setTemplates] = useState<TemplateSummary[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const [parentPath, setParentPath] = useState("");
  const [dirName, setDirName] = useState("");
  const [dirNameError, setDirNameError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const [activeOpId, setActiveOpId] = useState<string | null>(null);
  const op = activeOpId ? get(activeOpId) : null;

  // 终态只处理一次；失败态保留在卡片上供用户阅读
  const handledOpRef = useRef<string | null>(null);
  useEffect(() => {
    if (!op || !activeOpId || op.state !== "succeeded" || handledOpRef.current === activeOpId) return;
    handledOpRef.current = activeOpId;
    const wsId = operationResultWorkspaceId(op);
    if (wsId) {
      toast(`模板工作区已创建：${wsId.split(/[\\/]/).filter(Boolean).pop() ?? wsId}`, "ok");
      void openWs(wsId);
    }
  }, [op, activeOpId, openWs, toast]);

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const out = await apiTemplatesList();
        if (!alive) return;
        setTemplates(out.templates);
        setSelectedId((cur) => cur ?? out.templates[0]?.id ?? null);
      } catch (e) {
        if (alive) setLoadError(e instanceof IpcFailure ? e.message : String(e));
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const pickParentDirectory = async () => {
    if (isTauri()) {
      try {
        const selected = await openDialog({ directory: true, multiple: false });
        if (typeof selected === "string") {
          setParentPath(selected);
          return;
        }
        return; // 用户取消
      } catch {
        // 插件不可用时降级为手动输入
      }
    }
    const p = window.prompt("输入父目录路径", parentPath);
    if (p) setParentPath(p);
  };

  const selected = templates?.find((t) => t.id === selectedId) ?? null;
  const opRunning = !!op && (op.state === "queued" || op.state === "running");
  const targetDir = selected && dirName.trim() ? joinPath(parentPath, dirName.trim()) : "";

  const submit = async () => {
    if (!selected || submitting || opRunning) return;
    const invalid = validateDirectoryName(dirName);
    setDirNameError(invalid);
    if (invalid || !parentPath.trim()) {
      if (!parentPath.trim()) toast("请先选择或填写父目录", "warn");
      return;
    }
    setSubmitting(true);
    try {
      const { operation_id } = await apiTemplatesCreate(selected.id, parentPath.trim(), dirName.trim());
      handledOpRef.current = null;
      setActiveOpId(operation_id);
    } catch (e) {
      // 同步校验失败（PathEscape / TARGET_NOT_EMPTY 等）：IpcFailure.message 已是中文
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setSubmitting(false);
    }
  };

  const loading = templates === null && !loadError;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto flex max-w-4xl flex-col gap-4">
          {/* 标题行 */}
          <div className="flex items-center justify-between gap-3">
            <div>
              <h2 className="text-[1.05rem] font-bold tracking-tight text-[var(--t1,#222326)]">模板</h2>
              <p className="mt-0.5 text-[0.78rem] text-[var(--t3,#8a8f98)]">
                用内置模板一键创建带 supertask.yaml 的新工作区，创建完成后自动打开。
              </p>
            </div>
            <LayoutTemplate className="size-8 shrink-0 text-[var(--line-strong,#d0d6e0)]" />
          </div>

          {loading ? (
            <div className="flex items-center justify-center gap-2 py-12 text-[0.8rem] text-[var(--t3,#8a8f98)]" role="status">
              <Loader2 className="size-4 animate-spin" /> 正在读取内置模板…
            </div>
          ) : null}

          {loadError ? (
            <Card className="border-red-200 bg-[var(--st-danger-tint,#fdecec)] p-4 text-[0.8rem] text-[#DC2626]" role="alert">
              模板列表读取失败：{loadError}
            </Card>
          ) : null}

          {/* 模板卡片 */}
          {templates && templates.length > 0 ? (
            <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
              {templates.map((t) => (
                <TemplateCard
                  key={t.id}
                  template={t}
                  selected={t.id === selectedId}
                  onSelect={() => setSelectedId(t.id)}
                />
              ))}
            </div>
          ) : null}

          {/* 创建表单 */}
          {selected ? (
            <Card className="p-4">
              <div className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">创建新工作区</div>
              <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-[1fr_auto]">
                <label className="flex flex-col gap-1">
                  <span className="text-[0.72rem] font-medium text-[var(--t2,#62666d)]">父目录</span>
                  <Input
                    value={parentPath}
                    onChange={(e) => setParentPath(e.target.value)}
                    placeholder="例如 C:\project\my"
                  />
                </label>
                <div className="flex items-end">
                  <Button variant="outline" size="default" className="gap-1" onClick={() => void pickParentDirectory()}>
                    <FolderSearch /> 选择目录…
                  </Button>
                </div>
              </div>
              <label className="mt-3 flex flex-col gap-1">
                <span className="text-[0.72rem] font-medium text-[var(--t2,#62666d)]">目录名（单层，不含路径分隔符）</span>
                <Input
                  value={dirName}
                  onChange={(e) => {
                    setDirName(e.target.value);
                    if (dirNameError) setDirNameError(validateDirectoryName(e.target.value));
                  }}
                  aria-invalid={!!dirNameError}
                  placeholder="例如 my-demo-app"
                />
              </label>
              {dirNameError ? (
                <div className="mt-1.5 text-[0.74rem] text-[#DC2626]" role="alert">
                  {dirNameError}
                </div>
              ) : null}
              <div className="mt-4 flex items-center justify-between gap-3">
                <span className="truncate font-mono text-[0.68rem] text-[var(--t3,#8a8f98)]" title={targetDir}>
                  {targetDir ? `目标：${targetDir}` : ""}
                </span>
                <Button
                  onClick={() => void submit()}
                  disabled={submitting || opRunning || !dirName.trim() || !parentPath.trim()}
                >
                  创建工作区
                </Button>
              </div>
            </Card>
          ) : null}

          {/* 长操作进度 / 结果 */}
          {op ? <TemplateOperationCard op={op} targetDir={targetDir} /> : null}
        </div>
      </div>
    </div>
  );
}
