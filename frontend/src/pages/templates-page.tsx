import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  ChevronRight,
  Download,
  Eye,
  FolderSearch,
  LayoutTemplate,
  Loader2,
  RefreshCw,
  Search,
  Upload,
  X,
} from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { cn } from "@/lib/utils";
import { useToast } from "@/components/ui/toast";
import { apiTemplatesCreate, apiTemplatesExport, apiTemplatesImport, apiTemplatesList, apiTemplatesPreview, type TemplatesCreateArgs } from "../ipc/api";
import { isTauri } from "../ipc/invoke";
import {
  IpcFailure,
  type OpState,
  type TemplateBlockSummary,
  type TemplateSource,
  type TemplateSummary,
  type TemplatesPreviewOut,
} from "../ipc/protocol";
import { operationResultWorkspaceId, useOperations, type OperationState } from "../providers/operation-provider";
import { opErrorLabel } from "../lib/status";
import { useOpenWorkspace } from "../lib/use-open-workspace";

const PREFS_KEY = "st:templates:prefs";

type SourceFilter = "all" | TemplateSource;

type TemplatesPrefs = {
  source: SourceFilter;
  stacks: string[];
};

function loadPrefs(): TemplatesPrefs {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (!raw) return { source: "builtin", stacks: [] };
    const parsed = JSON.parse(raw) as Partial<TemplatesPrefs>;
    const source = parsed.source === "all" || parsed.source === "builtin" || parsed.source === "local" ? parsed.source : "builtin";
    const stacks = Array.isArray(parsed.stacks) ? parsed.stacks.filter((s): s is string => typeof s === "string") : [];
    return { source, stacks };
  } catch {
    return { source: "builtin", stacks: [] };
  }
}

function savePrefs(prefs: TemplatesPrefs) {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // ignore quota / private mode
  }
}

const OP_STATE_COLOR: Record<OpState, string> = {
  queued: "var(--t3,#8a8f98)",
  running: "var(--st-accent,#5e6ad2)",
  succeeded: "var(--st-ok-deep,#1e7e35)",
  failed: "var(--st-danger,#dc2626)",
  cancelled: "var(--t3,#8a8f98)",
};

/** 单层目录名校验（与后端 validate_directory_name 语义对齐的前端预检）。返回 i18n key。 */
function validateDirectoryName(name: string): string | null {
  const n = name.trim();
  if (!n) return "pages.templates.dirErrEmpty";
  if (n === "." || n === "..") return "pages.templates.dirErrDot";
  if (/[/\\]/.test(n)) return "pages.templates.dirErrSep";
  if (n.includes(":")) return "pages.templates.dirErrDrive";
  return null;
}

/** 块依赖闭合：从已选集合出发把 requires 递归并入（与后端 plan_blocks 同语义）。 */
function closeBlockDeps(blocks: TemplateBlockSummary[], ids: string[]): string[] {
  const chosen = [...ids];
  for (let i = 0; i < chosen.length; i++) {
    const b = blocks.find((x) => x.id === chosen[i]);
    for (const r of b?.requires ?? []) {
      if (!chosen.includes(r)) chosen.push(r);
    }
  }
  return chosen;
}

/** 组合向导步骤条：当前步高亮，已完成的步打勾。 */
function Stepper({ steps, current }: { steps: string[]; current: number }) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      {steps.map((label, i) => {
        const n = i + 1;
        const done = n < current;
        const cur = n === current;
        return (
          <div key={label} className="flex items-center gap-2">
            <span
              className={cn(
                "grid size-5 place-items-center rounded-full text-[10px] font-bold",
                cur
                  ? "bg-[var(--st-accent,#5e6ad2)] text-white"
                  : done
                    ? "bg-[var(--st-ok,#27a644)] text-white"
                    : "border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] text-[var(--t3,#8a8f98)]",
              )}
            >
              {done ? "✓" : n}
            </span>
            <span className={cn("text-[0.75rem]", cur ? "font-semibold text-[var(--t1,#222326)]" : "text-[var(--t3,#8a8f98)]")}>
              {label}
            </span>
            {i < steps.length - 1 ? <span className="h-px w-6 bg-[var(--line-strong,#d0d6e0)]" /> : null}
          </div>
        );
      })}
    </div>
  );
}

function joinPath(parent: string, name: string): string {
  const p = parent.trim().replace(/[\\/]+$/, "");
  const sep = p.includes("\\") ? "\\" : "/";
  return p ? `${p}${sep}${name}` : name;
}

/** 长操作进度卡片：state 本地化 + message + 有 progress 才显示进度条；成功态给「打开工作区」兜底。 */
function TemplateOperationCard({
  op,
  targetDir,
  workspaceId,
  onOpenWorkspace,
}: {
  op: OperationState;
  targetDir: string;
  workspaceId: string | null;
  onOpenWorkspace: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Card
      className={cn(
        "p-4",
        op.state === "failed" && "border-red-200 bg-[var(--st-danger-tint,#fdecec)]",
        op.state === "succeeded" && "border-[rgb(39_166_68_/_0.35)] bg-[var(--ok-tint,#e9f7ed)]",
      )}
      role="status"
    >
      <div className="flex items-center gap-2">
        {op.state === "running" ? (
          <Loader2 className="size-3.5 animate-spin text-[var(--st-accent,#5e6ad2)]" />
        ) : (
          <span className="size-2 shrink-0 rounded-full" style={{ background: OP_STATE_COLOR[op.state] }} />
        )}
        <span className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">{t("pages.templates.opTitle")}</span>
        <Badge variant={op.state === "failed" ? "destructive" : "soon"} className="shrink-0">
          {t(`pages.git.op_${op.state}`)}
        </Badge>
        {op.operation_id ? (
          <span className="truncate font-mono text-[0.62rem] text-[var(--t3,#8a8f98)]" title={op.operation_id}>
            {op.operation_id}
          </span>
        ) : null}
      </div>
      {op.message ? <div className="mt-1.5 text-[0.78rem] text-[var(--t2,#62666d)]">{op.message}</div> : null}
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
          {targetDir ? <span className="block text-[var(--t2,#62666d)]">{t("pages.git.target", { path: targetDir })}</span> : null}
        </div>
      ) : null}
      {op.state === "succeeded" && workspaceId ? (
        <div className="mt-2 flex items-center justify-between gap-2">
          <span className="truncate font-mono text-[0.7rem] text-[var(--t2,#62666d)]" title={workspaceId}>
            {workspaceId}
          </span>
          <Button variant="success" size="sm" className="shrink-0" onClick={onOpenWorkspace}>
            {t("pages.templates.openWs")}
          </Button>
        </div>
      ) : null}
    </Card>
  );
}

const SOURCE_TABS: { key: SourceFilter; labelKey: string }[] = [
  { key: "all", labelKey: "pages.templates.srcAll" },
  { key: "builtin", labelKey: "pages.templates.srcBuiltin" },
  { key: "local", labelKey: "pages.templates.srcLocal" },
];

function SourceSegmented({
  value,
  hasLocal,
  onChange,
}: {
  value: SourceFilter;
  hasLocal: boolean;
  onChange: (s: SourceFilter) => void;
}) {
  const { t } = useTranslation();
  const tabs = SOURCE_TABS.filter((tab) => tab.key !== "local" || hasLocal);
  return (
    <div
      className="inline-flex items-center gap-0.5 rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] p-0.5"
      role="tablist"
      aria-label={t("pages.templates.sourceAria")}
    >
      {tabs.map((tab) => (
        <button
          key={tab.key}
          type="button"
          role="tab"
          aria-selected={value === tab.key}
          onClick={() => onChange(tab.key)}
          className={cn(
            "cursor-pointer rounded-[6px] px-3 py-1 text-[0.75rem] font-semibold transition-colors duration-150",
            value === tab.key
              ? "bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent,#5e6ad2)] shadow-[inset_0_0_0_1px_var(--st-accent,#5e6ad2)]"
              : "text-[var(--t2,#62666d)] hover:bg-[var(--surface,#fff)] hover:text-[var(--t1,#222326)]",
          )}
        >
          {t(tab.labelKey)}
        </button>
      ))}
    </div>
  );
}

function TemplateCard({
  template,
  selected,
  onSelect,
  onPreview,
  onCreate,
  onCompose,
  onExport,
  exporting,
}: {
  template: TemplateSummary;
  selected: boolean;
  onSelect: () => void;
  onPreview: () => void;
  onCreate: () => void;
  onCompose: () => void;
  onExport?: () => void;
  exporting?: boolean;
}) {
  const { t } = useTranslation();
  const [filesOpen, setFilesOpen] = useState(false);

  if (template.invalid) {
    return (
      <Card className="p-4 opacity-60" aria-disabled>
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div className="min-w-0">
            <div className="text-[0.9rem] font-semibold text-[var(--t1,#222326)]">{template.name}</div>
            <div className="mt-1.5 flex items-center gap-1.5 text-[0.78rem] text-[#DC2626]">
              <span className="shrink-0 font-semibold">{t("pages.templates.manifestBroken")}</span>
              <span className="truncate text-[var(--t2,#62666d)]" title={template.invalid_reason ?? undefined}>
                {template.invalid_reason}
              </span>
            </div>
          </div>
          <Badge variant="secondary" className="shrink-0">
            {t("pages.templates.srcLocal")}
          </Badge>
        </div>
      </Card>
    );
  }

  const isCombo = !!template.blocks?.length;

  return (
    <Card
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      onClick={onSelect}
      onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && onSelect()}
      className={cn(
        "flex h-full cursor-pointer flex-col p-4 outline-none transition-all duration-150 focus-visible:outline-2 focus-visible:outline-[var(--st-accent,#5e6ad2)]",
        selected
          ? "border-[var(--st-accent,#5e6ad2)] bg-[var(--st-accent-tint,#eef0fb)]"
          : "hover:-translate-y-px hover:border-[var(--line-strong,#d0d6e0)] hover:shadow-[var(--shadow-2,0_6px_20px_rgb(16_24_40_/_0.09))]",
      )}
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="text-[0.9rem] font-semibold text-[var(--t1,#222326)]">{template.name}</div>
          <div className="mt-1 line-clamp-2 text-[0.78rem] leading-relaxed text-[var(--t2,#62666d)]">{template.description}</div>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          <Badge
            variant="secondary"
            className="shrink-0"
            title={template.source === "builtin" ? t("pages.templates.builtinTitle") : t("pages.templates.localTitle")}
          >
            {template.source === "builtin" ? t("pages.templates.builtinShort") : t("pages.templates.srcLocal")}
          </Badge>
          <Badge variant="secondary" className="shrink-0" title={t("pages.templates.versionTitle")}>
            v{template.version}
          </Badge>
          {isCombo ? (
            <Badge variant="outline" className="shrink-0">
              {t("pages.templates.comboBadge")}
            </Badge>
          ) : null}
        </div>
      </div>
      <div className="mt-2.5 flex flex-wrap items-center gap-1.5">
        {template.stacks.map((s) => (
          <Badge key={s} variant="outline" className="shrink-0">
            {s}
          </Badge>
        ))}
      </div>
      <Separator className="my-3" />
      <div className="mt-auto flex flex-wrap items-center justify-between gap-2">
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
          {t("pages.templates.filesOverview", { n: template.files.length })}
        </button>
        <div className="flex shrink-0 items-center gap-1.5">
          {onExport ? (
            <Button
              variant="ghost"
              size="sm"
              className="gap-1"
              disabled={exporting}
              onClick={(e) => {
                e.stopPropagation();
                onExport();
              }}
            >
              {exporting ? <Loader2 className="size-3.5 animate-spin" /> : <Download className="size-3.5" />}
              {t("pages.templates.exportCta")}
            </Button>
          ) : null}
          <Button
            variant="ghost"
            size="sm"
            className="gap-1"
            onClick={(e) => {
              e.stopPropagation();
              onPreview();
            }}
          >
            <Eye className="size-3.5" /> {t("pages.templates.previewCta")}
          </Button>
          {isCombo ? (
            <Button
              variant="soft"
              size="sm"
              className="gap-1"
              onClick={(e) => {
                e.stopPropagation();
                onCompose();
              }}
            >
              {t("pages.templates.comboCta")} <ChevronRight className="size-3.5" />
            </Button>
          ) : (
            <Button
              size="sm"
              onClick={(e) => {
                e.stopPropagation();
                onCreate();
              }}
            >
              {t("pages.templates.useCta")}
            </Button>
          )}
        </div>
      </div>
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
  const { t } = useTranslation();
  const openWs = useOpenWorkspace();
  const { get } = useOperations();

  const initialPrefs = useMemo(() => loadPrefs(), []);
  const [templates, setTemplates] = useState<TemplateSummary[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [sourceFilter, setSourceFilter] = useState<SourceFilter>(initialPrefs.source);
  const [stackFilter, setStackFilter] = useState<Set<string>>(() => new Set(initialPrefs.stacks));
  const [searchQuery, setSearchQuery] = useState("");

  const [parentPath, setParentPath] = useState("");
  const [dirName, setDirName] = useState("");
  const [dirNameError, setDirNameError] = useState<string | null>(null);
  const [paramValues, setParamValues] = useState<Record<string, string>>({});
  const [selectedBlocks, setSelectedBlocks] = useState<string[]>([]);
  const [portValues, setPortValues] = useState<Record<string, number>>({});
  const [preview, setPreview] = useState<TemplatesPreviewOut | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  /** gallery 上的只读预览浮层；创建/组合向导是另一套 Dialog */
  const [detailOpen, setDetailOpen] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [wizardMode, setWizardMode] = useState(false);
  const [step, setStep] = useState<1 | 2 | 3>(1);
  const [confirmCreate, setConfirmCreate] = useState(false);
  const WIZARD_STEPS = [t("pages.templates.stepBlocks"), t("pages.templates.stepBasic"), t("pages.templates.stepPreview")];

  const openDetail = (id: string) => {
    setSelectedId(id);
    setDetailOpen(true);
  };
  const openCreate = (id: string) => {
    setSelectedId(id);
    setWizardMode(false);
    setStep(1);
    setPreview(null);
    setCreateOpen(true);
    setDetailOpen(false);
  };
  const openWizard = (id: string) => {
    setSelectedId(id);
    setWizardMode(true);
    setStep(1);
    setPreview(null);
    setCreateOpen(true);
    setDetailOpen(false);
  };
  const closeCreate = () => {
    setCreateOpen(false);
    setWizardMode(false);
    setStep(1);
    setPreview(null);
    setConfirmCreate(false);
  };

  const [activeOpId, setActiveOpId] = useState<string | null>(null);
  const op = activeOpId ? get(activeOpId) : null;
  const opRef = useRef(op);
  opRef.current = op;
  const [pendingCreate, setPendingCreate] = useState(false);
  const [submittedTarget, setSubmittedTarget] = useState<string | null>(null);
  const [showFallback, setShowFallback] = useState(false);

  useEffect(() => {
    if (op) {
      setPendingCreate(false);
      setShowFallback(false);
    }
  }, [op]);

  useEffect(() => {
    savePrefs({ source: sourceFilter, stacks: [...stackFilter] });
  }, [sourceFilter, stackFilter]);

  const displayOp: OperationState | null =
    op ??
    (pendingCreate
      ? {
          operation_id: "",
          kind: "templates.create",
          state: "queued",
          progress: null,
          message: t("pages.templates.pendingQueued"),
          error_code: null,
          result: null,
        }
      : null);
  const wsIdForCard = op ? operationResultWorkspaceId(op) : null;

  const handledOpRef = useRef<string | null>(null);
  useEffect(() => {
    if (!op || !activeOpId || op.state !== "succeeded" || handledOpRef.current === activeOpId) return;
    handledOpRef.current = activeOpId;
    const wsId = operationResultWorkspaceId(op);
    if (wsId) {
      toast(t("pages.templates.createdTo", { name: wsId.split(/[\\/]/).filter(Boolean).pop() ?? wsId }), "ok");
      void openWs(wsId);
      closeCreate();
    }
  }, [op, activeOpId, openWs, toast, t]);

  const loadTemplates = async (opts?: { soft?: boolean }) => {
    if (!opts?.soft) {
      setLoadError(null);
    } else {
      setRefreshing(true);
    }
    try {
      const out = await apiTemplatesList();
      setTemplates(out.templates);
      setSelectedId((cur) => {
        if (cur && out.templates.some((tpl) => tpl.id === cur)) return cur;
        return out.templates.find((tpl) => !tpl.invalid)?.id ?? out.templates[0]?.id ?? null;
      });
      setLoadError(null);
    } catch (e) {
      setLoadError(e instanceof IpcFailure ? e.message : String(e));
    } finally {
      setRefreshing(false);
    }
  };

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const out = await apiTemplatesList();
        if (!alive) return;
        setTemplates(out.templates);
        setSelectedId((cur) => cur ?? out.templates.find((tpl) => !tpl.invalid)?.id ?? out.templates[0]?.id ?? null);
      } catch (e) {
        if (alive) setLoadError(e instanceof IpcFailure ? e.message : String(e));
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    setParamValues({});
    setPreview(null);
    const blocks = templates?.find((tpl) => tpl.id === selectedId)?.blocks;
    setSelectedBlocks(blocks?.map((b) => b.id) ?? []);
    setPortValues(
      Object.fromEntries(
        (blocks ?? []).flatMap((b) => b.services.map((s) => [s, b.default_port])).filter(([, v]) => v != null) as [string, number][],
      ),
    );
  }, [selectedId, templates]);

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

  const [importing, setImporting] = useState(false);
  const [exportingId, setExportingId] = useState<string | null>(null);

  /** 导入模板包（zip）到本地库：文件对话框选包 → templates.import → 刷新列表。 */
  const importPackage = async () => {
    if (importing) return;
    let picked: string | null = null;
    if (isTauri()) {
      try {
        const selected = await openDialog({
          multiple: false,
          filters: [{ name: "Template package", extensions: ["zip"] }],
        });
        if (typeof selected === "string") picked = selected;
      } catch {
        // 插件不可用时降级为手动输入
      }
    }
    if (!picked) {
      const p = window.prompt(t("pages.templates.importPrompt"), "");
      if (p) picked = p;
    }
    if (!picked) return;
    setImporting(true);
    try {
      const out = await apiTemplatesImport({ zipPath: picked });
      toast(t("pages.templates.importedOk", { id: out.id }), "ok");
      await loadTemplates({ soft: true });
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setImporting(false);
    }
  };

  /** 导出模板为可分享 zip：目录对话框选目标 → templates.export。 */
  const exportPackage = async (tpl: TemplateSummary) => {
    if (exportingId) return;
    let dir: string | null = null;
    if (isTauri()) {
      try {
        const selected = await openDialog({ directory: true, multiple: false });
        if (typeof selected === "string") dir = selected;
      } catch {
        // 插件不可用时降级为手动输入
      }
    }
    if (!dir) {
      const p = window.prompt(t("pages.templates.exportPrompt"), "");
      if (p) dir = p;
    }
    if (!dir) return;
    setExportingId(tpl.id);
    try {
      const out = await apiTemplatesExport({ templateId: tpl.id, source: tpl.source, targetDir: dir });
      toast(t("pages.templates.exportedTo", { path: out.path }), "ok");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setExportingId(null);
    }
  };

  const allStacks = useMemo(() => {
    const set = new Set<string>();
    for (const tpl of templates ?? []) {
      for (const s of tpl.stacks) set.add(s);
    }
    return [...set].sort((a, b) => a.localeCompare(b));
  }, [templates]);

  const visibleTemplates = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    return (templates ?? []).filter((tpl) => {
      if (sourceFilter !== "all" && tpl.source !== sourceFilter) return false;
      if (stackFilter.size > 0 && !tpl.stacks.some((s) => stackFilter.has(s))) return false;
      if (q) {
        const hay = [tpl.name, tpl.description, tpl.id, ...tpl.stacks, ...tpl.files].join("\n").toLowerCase();
        if (!hay.includes(q)) return false;
      }
      return true;
    });
  }, [templates, sourceFilter, stackFilter, searchQuery]);

  const selected = templates?.find((tpl) => tpl.id === selectedId) ?? null;
  const wizardActive = createOpen && wizardMode && !!selected?.blocks?.length;
  const activeBlocks = selected?.blocks ?? [];
  const opRunning = !!op && (op.state === "queued" || op.state === "running");
  const targetDir = selected && dirName.trim() ? joinPath(parentPath, dirName.trim()) : "";
  const hasLocal = !!templates?.some((tpl) => tpl.source === "local");
  const hasActiveFilters = sourceFilter !== "all" || stackFilter.size > 0 || searchQuery.trim() !== "";

  const submit = async () => {
    if (!selected || submitting || opRunning) return;
    const invalid = validateDirectoryName(dirName);
    setDirNameError(invalid);
    if (invalid || !parentPath.trim()) {
      if (!parentPath.trim()) toast(t("pages.templates.parentRequired"), "warn");
      return;
    }
    if (selected.blocks?.length && !preview) {
      toast(t("pages.templates.previewRequired"), "warn");
      return;
    }
    setSubmitting(true);
    setPendingCreate(true);
    setShowFallback(false);
    setConfirmCreate(false);
    try {
      const args: TemplatesCreateArgs = {
        templateId: selected.id,
        parentPath: parentPath.trim(),
        directoryName: dirName.trim(),
        source: selected.source,
        params: paramValues,
      };
      if (selected.blocks?.length) {
        args.blocks = selectedBlocks;
        args.ports = portValues;
      }
      const { operation_id } = await apiTemplatesCreate(args);
      handledOpRef.current = null;
      setActiveOpId(operation_id);
      setSubmittedTarget(joinPath(parentPath.trim(), dirName.trim()));
      window.setTimeout(() => {
        if (!opRef.current) setShowFallback(true);
      }, 4000);
    } catch (e) {
      setPendingCreate(false);
      setSubmittedTarget(null);
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setSubmitting(false);
    }
  };

  const requestCreate = () => {
    if (!selected || submitting || opRunning) return;
    const invalid = validateDirectoryName(dirName);
    setDirNameError(invalid);
    if (invalid || !parentPath.trim()) {
      if (!parentPath.trim()) toast(t("pages.templates.parentRequired"), "warn");
      return;
    }
    if (selected.blocks?.length && !preview) {
      toast(t("pages.templates.previewRequired"), "warn");
      return;
    }
    if (selected.params?.some((p) => p.required && !paramValues[p.key]?.trim())) {
      toast(t("pages.templates.paramRequired"), "warn");
      return;
    }
    setConfirmCreate(true);
  };

  const toggleBlock = (id: string) => {
    const blocks = selected?.blocks ?? [];
    setPreview(null);
    if (selectedBlocks.includes(id)) {
      const dependents = selectedBlocks.filter(
        (other) => other !== id && blocks.find((b) => b.id === other)?.requires.includes(id),
      );
      if (dependents.length > 0) {
        const names = dependents.map((d) => blocks.find((b) => b.id === d)?.label ?? d).join("、");
        toast(t("pages.templates.blockLocked", { block: blocks.find((b) => b.id === id)?.label ?? id, names }), "warn");
        return;
      }
      setSelectedBlocks((cur) => cur.filter((x) => x !== id));
    } else {
      setSelectedBlocks((cur) => closeBlockDeps(blocks, [...cur, id]));
    }
  };

  const changePort = (svcId: string, raw: string) => {
    setPreview(null);
    setPortValues((cur) => ({ ...cur, [svcId]: raw === "" ? NaN : Number(raw) }));
  };

  const wizardServices =
    selected?.blocks
      ?.filter((b) => selectedBlocks.includes(b.id))
      .flatMap((b) => b.services.map((svcId) => ({ svcId, block: b, port: portValues[svcId] ?? b.default_port ?? NaN }))) ?? [];
  const portConflict = (() => {
    const seen = new Map<number, string>();
    for (const { svcId, port } of wizardServices) {
      if (seen.has(port)) return { port, a: seen.get(port)!, b: svcId };
      seen.set(port, svcId);
    }
    return null;
  })();
  const portInvalid = wizardServices.some(({ port }) => !Number.isInteger(port) || port < 1024 || port > 65535);

  const runPreview = async () => {
    if (!selected || previewing) return;
    if (portConflict || portInvalid) {
      toast(t("pages.templates.portProblem"), "warn");
      return;
    }
    setPreviewing(true);
    try {
      const out = await apiTemplatesPreview({
        templateId: selected.id,
        source: selected.source,
        blocks: selectedBlocks,
        ports: Object.fromEntries(wizardServices.map(({ svcId, port }) => [svcId, port])),
        params: paramValues,
      });
      setPreview(out);
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setPreviewing(false);
    }
  };

  const goNext = () => {
    if (!selected) return;
    if (step === 1) {
      if (selectedBlocks.length === 0) {
        toast(t("pages.templates.needBlock"), "warn");
        return;
      }
      setStep(2);
      return;
    }
    if (step === 2) {
      const invalid = validateDirectoryName(dirName);
      setDirNameError(invalid);
      if (invalid || !parentPath.trim()) {
        toast(t("pages.templates.parentRequired"), "warn");
        return;
      }
      if (portConflict || portInvalid) {
        toast(t("pages.templates.portProblem"), "warn");
        return;
      }
      if (selected.params?.some((p) => p.required && !paramValues[p.key]?.trim())) {
        toast(t("pages.templates.paramRequired"), "warn");
        return;
      }
      setStep(3);
    }
  };

  const toggleStack = (stack: string) => {
    setStackFilter((prev) => {
      const next = new Set(prev);
      if (next.has(stack)) next.delete(stack);
      else next.add(stack);
      return next;
    });
  };

  const clearFilters = () => {
    setSourceFilter("all");
    setStackFilter(new Set());
    setSearchQuery("");
  };

  const loading = templates === null && !loadError;

  const createFormFields = selected ? (
    <>
      {selected.params?.length ? (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
          {selected.params.map((p) => (
            <label key={p.key} className="flex flex-col gap-1">
              <span className="text-[0.72rem] font-medium text-[var(--t2,#62666d)]">
                {p.label || p.key}
                {p.required ? <span className="ml-0.5 text-[#DC2626]">*</span> : null}
              </span>
              <Input
                value={paramValues[p.key] ?? ""}
                onChange={(e) => {
                  setParamValues((cur) => ({ ...cur, [p.key]: e.target.value }));
                  setPreview(null);
                }}
                placeholder={p.key === "project_name" ? t("pages.templates.projectNamePlaceholder") : p.key}
              />
            </label>
          ))}
        </div>
      ) : null}
      <div className={cn("grid grid-cols-1 gap-3 md:grid-cols-[1fr_auto]", selected.params?.length ? "mt-3" : "")}>
        <label className="flex flex-col gap-1">
          <span className="text-[0.72rem] font-medium text-[var(--t2,#62666d)]">{t("pages.templates.parentDir")}</span>
          <Input
            value={parentPath}
            onChange={(e) => setParentPath(e.target.value)}
            placeholder={t("pages.templates.parentDirPlaceholder")}
          />
        </label>
        <div className="flex items-end">
          <Button variant="outline" size="default" className="gap-1" onClick={() => void pickParentDirectory()}>
            <FolderSearch /> {t("pages.git.pickDir")}
          </Button>
        </div>
      </div>
      <label className="mt-3 flex flex-col gap-1">
        <span className="text-[0.72rem] font-medium text-[var(--t2,#62666d)]">{t("pages.templates.dirName")}</span>
        <Input
          value={dirName}
          onChange={(e) => {
            setDirName(e.target.value);
            if (dirNameError) setDirNameError(validateDirectoryName(e.target.value));
          }}
          aria-invalid={!!dirNameError}
          placeholder={t("pages.templates.dirNameExample")}
        />
      </label>
      {dirNameError ? (
        <div className="mt-1.5 text-[0.74rem] text-[#DC2626]" role="alert">
          {t(dirNameError)}
        </div>
      ) : null}
    </>
  ) : null;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto">
        <div className="mx-auto flex max-w-5xl flex-col gap-4 p-6 pb-28">
          <div className="flex items-center justify-between gap-3">
            <div>
              <h2 className="text-[1.05rem] font-bold tracking-tight text-[var(--t1,#222326)]">{t("nav.templates")}</h2>
              <p className="mt-0.5 text-[0.78rem] text-[var(--t3,#8a8f98)]">{t("pages.templates.pageDesc")}</p>
            </div>
            <LayoutTemplate className="size-8 shrink-0 text-[var(--line-strong,#d0d6e0)]" />
          </div>

          {/* 粘性筛选栏：搜索 / 来源 / 技术栈芯片 */}
          <div className="sticky top-0 z-10 -mx-1 space-y-3 rounded-[var(--r-md,12px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)]/95 p-3 shadow-[var(--shadow-1,0_1px_2px_rgb(16_24_40_/_0.04))] backdrop-blur-sm">
            <div className="flex flex-wrap items-center gap-2">
              <div className="relative min-w-[12rem] flex-1">
                <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-[var(--t3,#8a8f98)]" />
                <Input
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  placeholder={t("pages.templates.searchPlaceholder")}
                  className="pl-8"
                  aria-label={t("common.search")}
                />
                {searchQuery ? (
                  <button
                    type="button"
                    className="absolute top-1/2 right-2 -translate-y-1/2 rounded p-0.5 text-[var(--t3,#8a8f98)] hover:text-[var(--t1,#222326)]"
                    onClick={() => setSearchQuery("")}
                    aria-label={t("common.clear")}
                  >
                    <X className="size-3.5" />
                  </button>
                ) : null}
              </div>
              <SourceSegmented
                value={hasLocal || sourceFilter !== "local" ? sourceFilter : "builtin"}
                hasLocal={hasLocal}
                onChange={(s) => setSourceFilter(s)}
              />
              <Button
                variant="outline"
                size="sm"
                className="gap-1"
                disabled={refreshing || loading}
                onClick={() => void loadTemplates({ soft: true })}
              >
                <RefreshCw className={cn("size-3.5", refreshing && "animate-spin")} />
                {t("common.refresh")}
              </Button>
              <Button
                variant="outline"
                size="sm"
                className="gap-1"
                disabled={importing || loading}
                onClick={() => void importPackage()}
              >
                {importing ? <Loader2 className="size-3.5 animate-spin" /> : <Upload className="size-3.5" />}
                {t("pages.templates.importCta")}
              </Button>
              {hasActiveFilters ? (
                <Button variant="ghost" size="sm" onClick={clearFilters}>
                  {t("pages.templates.clearFilters")}
                </Button>
              ) : null}
            </div>
            {allStacks.length > 0 ? (
              <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label={t("pages.templates.stackFilterAria")}>
                <span className="mr-1 text-[0.7rem] font-medium text-[var(--t3,#8a8f98)]">{t("pages.templates.stackFilterLabel")}</span>
                {allStacks.map((stack) => {
                  const on = stackFilter.has(stack);
                  return (
                    <button
                      key={stack}
                      type="button"
                      onClick={() => toggleStack(stack)}
                      className={cn(
                        "rounded-full border px-2.5 py-0.5 text-[0.7rem] font-semibold transition-colors",
                        on
                          ? "border-[var(--st-accent,#5e6ad2)] bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent,#5e6ad2)]"
                          : "border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] text-[var(--t2,#62666d)] hover:bg-[var(--surface-2,#f3f4f5)]",
                      )}
                    >
                      {stack}
                    </button>
                  );
                })}
              </div>
            ) : null}
            {templates ? (
              <div className="text-[0.7rem] text-[var(--t3,#8a8f98)]">
                {t("pages.templates.countLabel", { n: visibleTemplates.length, total: templates.length })}
              </div>
            ) : null}
          </div>

          {loading ? (
            <div className="flex flex-col items-center justify-center gap-3 py-16" role="status">
              <Loader2 className="size-6 animate-spin text-[var(--st-accent,#5e6ad2)]" />
              <div className="text-[0.8rem] text-[var(--t3,#8a8f98)]">{t("pages.templates.loading")}</div>
              <div className="grid w-full grid-cols-1 gap-3 lg:grid-cols-2">
                {[0, 1, 2, 3].map((i) => (
                  <Card key={i} className="h-36 animate-pulse bg-[var(--surface-2,#f3f4f5)] p-4" />
                ))}
              </div>
            </div>
          ) : null}

          {loadError ? (
            <Card className="border-red-200 bg-[var(--st-danger-tint,#fdecec)] p-4 text-[0.8rem] text-[#DC2626]" role="alert">
              <div className="font-semibold">{t("pages.templates.loadFailed")}</div>
              <div className="mt-1 text-[var(--t2,#62666d)]">{loadError}</div>
              <Button variant="outline" size="sm" className="mt-3" onClick={() => void loadTemplates()}>
                {t("common.retry")}
              </Button>
            </Card>
          ) : null}

          {!loading && !loadError && templates ? (
            visibleTemplates.length > 0 ? (
              <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
                {visibleTemplates.map((tpl) => (
                  <TemplateCard
                    key={`${tpl.source}:${tpl.id}`}
                    template={tpl}
                    selected={tpl.id === selectedId}
                    onSelect={() => setSelectedId(tpl.id)}
                    onPreview={() => openDetail(tpl.id)}
                    onCreate={() => openCreate(tpl.id)}
                    onCompose={() => openWizard(tpl.id)}
                    onExport={tpl.source === "local" ? () => void exportPackage(tpl) : undefined}
                    exporting={exportingId === tpl.id}
                  />
                ))}
              </div>
            ) : (
              <Card className="flex flex-col items-center gap-2 p-10 text-center">
                <LayoutTemplate className="size-10 text-[var(--line-strong,#d0d6e0)]" />
                <div className="text-[0.9rem] font-semibold text-[var(--t1,#222326)]">
                  {templates.length === 0
                    ? t("pages.templates.noTemplates")
                    : sourceFilter === "local" && !searchQuery && stackFilter.size === 0
                      ? t("pages.templates.noLocal")
                      : t("pages.templates.noSearchMatch")}
                </div>
                <p className="max-w-md text-[0.78rem] leading-relaxed text-[var(--t3,#8a8f98)]">
                  {templates.length === 0 ? t("pages.templates.emptyHint") : t("pages.templates.emptyFilterHint")}
                </p>
                {hasActiveFilters ? (
                  <Button variant="outline" size="sm" className="mt-2" onClick={clearFilters}>
                    {t("pages.templates.clearFilters")}
                  </Button>
                ) : null}
              </Card>
            )
          ) : null}

          {/* 粘性操作条：选中模板后快速打开创建/预览，不推动画廊布局 */}
          {selected && !selected.invalid && !createOpen && !detailOpen ? (
            <div className="fixed right-6 bottom-6 z-20 flex max-w-[min(28rem,calc(100%-3rem))] items-center gap-2 rounded-xl border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] p-2 shadow-[var(--shadow-2,0_6px_20px_rgb(16_24_40_/_0.12))]">
              <div className="min-w-0 flex-1 px-2">
                <div className="truncate text-[0.8rem] font-semibold text-[var(--t1,#222326)]">{selected.name}</div>
                <div className="truncate text-[0.68rem] text-[var(--t3,#8a8f98)]">
                  {selected.source === "builtin" ? t("pages.templates.builtinShort") : t("pages.templates.srcLocal")} · v
                  {selected.version}
                </div>
              </div>
              <Button variant="ghost" size="sm" className="gap-1" onClick={() => openDetail(selected.id)}>
                <Eye className="size-3.5" /> {t("pages.templates.previewCta")}
              </Button>
              {selected.blocks?.length ? (
                <Button size="sm" onClick={() => openWizard(selected.id)}>
                  {t("pages.templates.comboCta")}
                </Button>
              ) : (
                <Button size="sm" onClick={() => openCreate(selected.id)}>
                  {t("pages.templates.useCta")}
                </Button>
              )}
            </div>
          ) : null}

          {displayOp ? (
            <div className="fixed bottom-6 left-1/2 z-20 w-[min(36rem,calc(100%-2rem))] -translate-x-1/2">
              <TemplateOperationCard
                op={displayOp}
                targetDir={targetDir || submittedTarget || ""}
                workspaceId={wsIdForCard}
                onOpenWorkspace={() => wsIdForCard && void openWs(wsIdForCard)}
              />
            </div>
          ) : null}
          {showFallback && submittedTarget && (!op || (op.state !== "succeeded" && op.state !== "failed")) ? (
            <Card className="border-[var(--line-strong,#d0d6e0)] p-3 text-[0.76rem] text-[var(--t2,#62666d)]">
              {t("pages.templates.noEventHint")}
              <Button variant="outline" size="sm" className="ml-2" onClick={() => void openWs(submittedTarget)}>
                {t("pages.templates.openTarget")}
              </Button>
            </Card>
          ) : null}
        </div>
      </div>

      {/* 只读预览浮层：不推动画廊 */}
      <Dialog open={detailOpen && !!selected} onOpenChange={(o) => !o && setDetailOpen(false)}>
        <DialogContent className="sm:max-w-lg" showCloseButton>
          {selected ? (
            <>
              <DialogHeader>
                <DialogTitle className="flex flex-wrap items-center gap-2">
                  <span>{selected.name}</span>
                  <Badge variant="secondary">
                    {selected.source === "builtin" ? t("pages.templates.builtinShort") : t("pages.templates.srcLocal")}
                  </Badge>
                  <Badge variant="secondary">v{selected.version}</Badge>
                </DialogTitle>
                <DialogDescription>{selected.description || t("pages.templates.pageDesc")}</DialogDescription>
              </DialogHeader>
              <div className="flex flex-wrap gap-1.5">
                {selected.stacks.map((s) => (
                  <Badge key={s} variant="outline">
                    {s}
                  </Badge>
                ))}
              </div>
              <div>
                <div className="mb-1.5 text-[0.72rem] font-semibold text-[var(--t2,#62666d)]">
                  {t("pages.templates.filesOverview", { n: selected.files.length })}
                </div>
                <ul className="max-h-48 space-y-0.5 overflow-auto rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f3f4f5)] p-2.5 font-mono text-[0.68rem] text-[var(--t2,#62666d)]">
                  {selected.files.map((f) => (
                    <li key={f} className="truncate" title={f}>
                      {f}
                    </li>
                  ))}
                </ul>
              </div>
              {selected.blocks?.length ? (
                <div>
                  <div className="mb-1.5 text-[0.72rem] font-semibold text-[var(--t2,#62666d)]">{t("pages.templates.blocksTitle")}</div>
                  <div className="flex flex-wrap gap-1.5">
                    {selected.blocks.map((b) => (
                      <Badge key={b.id} variant="outline" title={b.kind}>
                        {b.label || b.id}
                      </Badge>
                    ))}
                  </div>
                </div>
              ) : null}
              <DialogFooter>
                <Button variant="outline" onClick={() => setDetailOpen(false)}>
                  {t("common.close")}
                </Button>
                {selected.blocks?.length ? (
                  <Button onClick={() => openWizard(selected.id)}>{t("pages.templates.comboCta")}</Button>
                ) : (
                  <Button onClick={() => openCreate(selected.id)}>{t("pages.templates.useCta")}</Button>
                )}
              </DialogFooter>
            </>
          ) : null}
        </DialogContent>
      </Dialog>

      {/* 创建 / 组合向导浮层 */}
      <Dialog
        open={createOpen && !!selected && !selected.invalid}
        onOpenChange={(o) => {
          if (!o) closeCreate();
        }}
      >
        <DialogContent className="flex max-h-[min(90vh,44rem)] flex-col gap-0 overflow-hidden p-0 sm:max-w-2xl" showCloseButton>
          {selected ? (
            <>
              <div className="border-b border-[var(--line,#e6e6e6)] p-4 pr-12">
                <DialogHeader>
                  <DialogTitle className="flex flex-wrap items-center gap-2">
                    {wizardActive ? t("pages.templates.comboCta") : t("pages.templates.createHeading")}
                    <Badge variant="secondary">
                      {selected.source === "builtin" ? t("pages.templates.builtinShort") : t("pages.templates.srcLocal")}
                    </Badge>
                  </DialogTitle>
                  <DialogDescription>
                    <span className="font-medium text-[var(--t1,#222326)]">{selected.name}</span>
                    {selected.description ? ` — ${selected.description}` : ""}
                  </DialogDescription>
                </DialogHeader>
                {wizardActive ? (
                  <div className="mt-3">
                    <Stepper steps={WIZARD_STEPS} current={step} />
                  </div>
                ) : null}
              </div>

              <div className="min-h-0 flex-1 overflow-auto p-4">
                {wizardActive && step === 1 ? (
                  <div>
                    <div className="text-[0.78rem] font-semibold text-[var(--t1,#222326)]">
                      {t("pages.templates.blocksTitle")}
                      <span className="ml-2 font-normal text-[var(--t3,#8a8f98)]">{t("pages.templates.blocksHint")}</span>
                    </div>
                    <div className="mt-2 flex flex-col gap-1.5">
                      {activeBlocks.map((b) => {
                        const checked = selectedBlocks.includes(b.id);
                        const lockedBy = selectedBlocks
                          .filter((other) => other !== b.id && selected.blocks!.find((x) => x.id === other)?.requires.includes(b.id))
                          .map((d) => selected.blocks!.find((x) => x.id === d)?.label ?? d);
                        return (
                          <label
                            key={b.id}
                            className={cn(
                              "flex cursor-pointer items-center gap-2 rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] px-2.5 py-1.5 transition-colors duration-150",
                              checked ? "bg-[var(--st-accent-tint,#eef0fb)]" : "bg-[var(--surface,#fff)] hover:bg-[var(--surface-2,#f3f4f5)]",
                              lockedBy.length > 0 && "cursor-not-allowed opacity-60",
                            )}
                          >
                            <input
                              type="checkbox"
                              checked={checked}
                              disabled={lockedBy.length > 0}
                              onChange={() => toggleBlock(b.id)}
                              className="accent-[var(--st-accent,#5e6ad2)]"
                            />
                            <span className="text-[0.78rem] font-medium text-[var(--t1,#222326)]">{b.label}</span>
                            <Badge variant="outline" className="text-[10px]">
                              {b.kind}
                            </Badge>
                            {b.requires.length ? (
                              <span className="text-[0.68rem] text-[var(--t3,#8a8f98)]">
                                {t("pages.templates.dependsOn", { deps: b.requires.join(", ") })}
                              </span>
                            ) : null}
                            {lockedBy.length > 0 ? (
                              <span className="ml-auto text-[0.68rem] text-[var(--t3,#8a8f98)]">
                                {t("pages.templates.lockedBy", { names: lockedBy.join("、") })}
                              </span>
                            ) : null}
                          </label>
                        );
                      })}
                    </div>
                  </div>
                ) : null}

                {wizardActive && step === 2 ? (
                  <div>
                    {createFormFields}
                    {wizardServices.length > 0 ? (
                      <div className="mt-3">
                        <div className="text-[0.72rem] font-medium text-[var(--t2,#62666d)]">{t("pages.templates.portAssign")}</div>
                        <div className="mt-1.5 flex flex-wrap gap-3">
                          {wizardServices.map(({ svcId, port }) => (
                            <label key={svcId} className="flex items-center gap-1.5 text-[0.74rem] text-[var(--t1,#222326)]">
                              <span className="font-mono text-[var(--t2,#62666d)]">{svcId}</span>
                              <Input
                                type="number"
                                value={Number.isNaN(port) ? "" : port}
                                onChange={(e) => changePort(svcId, e.target.value)}
                                className="h-8 w-24 font-mono"
                              />
                            </label>
                          ))}
                        </div>
                        {portConflict ? (
                          <div className="mt-1.5 text-[0.74rem] text-[#DC2626]" role="alert">
                            {t("pages.templates.portConflict", {
                              port: portConflict.port,
                              a: portConflict.a,
                              b: portConflict.b,
                            })}
                          </div>
                        ) : portInvalid ? (
                          <div className="mt-1.5 text-[0.74rem] text-[#DC2626]" role="alert">
                            {t("pages.templates.portInvalid")}
                          </div>
                        ) : null}
                      </div>
                    ) : null}
                  </div>
                ) : null}

                {wizardActive && step === 3 ? (
                  <div>
                    <div className="flex flex-wrap items-center gap-2">
                      <Button
                        variant="soft"
                        size="sm"
                        className="gap-1"
                        disabled={previewing || !!portConflict || portInvalid || wizardServices.length === 0}
                        onClick={() => void runPreview()}
                      >
                        {previewing ? <Loader2 className="size-3.5 animate-spin" /> : <Eye className="size-3.5" />}
                        {previewing ? t("pages.templates.generating") : t("pages.templates.generatePreview")}
                      </Button>
                      {preview ? (
                        <span className="text-[0.72rem] text-[var(--st-ok-deep,#1e7e35)]">
                          {t("pages.templates.previewOk", {
                            services: Object.keys(preview.services).length,
                            files: preview.files.length,
                          })}
                        </span>
                      ) : (
                        <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.templates.previewNeeded")}</span>
                      )}
                    </div>
                    {preview ? (
                      <div className="mt-2 rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f3f4f5)] p-2.5">
                        <table className="w-full text-left font-mono text-[0.7rem] text-[var(--t1,#222326)]">
                          <thead>
                            <tr className="text-[var(--t3,#8a8f98)]">
                              <th className="py-0.5 pr-3 font-semibold">{t("pages.templates.colService")}</th>
                              <th className="py-0.5 pr-3 font-semibold">{t("pages.templates.colKind")}</th>
                              <th className="py-0.5 font-semibold">{t("pages.templates.colPort")}</th>
                            </tr>
                          </thead>
                          <tbody>
                            {Object.entries(preview.services).map(([id, svc]) => (
                              <tr key={id}>
                                <td className="py-0.5 pr-3">{id}</td>
                                <td className="py-0.5 pr-3">{String(svc.kind ?? "—")}</td>
                                <td className="py-0.5">{String(svc.port ?? "—")}</td>
                              </tr>
                            ))}
                          </tbody>
                        </table>
                        {preview.files.length ? (
                          <div className="mt-2 max-h-28 overflow-auto border-t border-[var(--line,#e6e6e6)] pt-2 font-mono text-[0.66rem] text-[var(--t3,#8a8f98)]">
                            {preview.files.map((f) => (
                              <div key={f} className="truncate" title={f}>
                                {f}
                              </div>
                            ))}
                          </div>
                        ) : null}
                        {preview.warnings.map((w) => (
                          <div key={w} className="mt-1 text-[0.72rem] text-[var(--st-warn-dot,#eab308)]">
                            {w}
                          </div>
                        ))}
                      </div>
                    ) : null}
                    {targetDir ? (
                      <div className="mt-3 truncate font-mono text-[0.68rem] text-[var(--t3,#8a8f98)]" title={targetDir}>
                        {t("pages.git.target", { path: targetDir })}
                      </div>
                    ) : null}
                  </div>
                ) : null}

                {!wizardActive ? <div>{createFormFields}</div> : null}
              </div>

              <div className="sticky bottom-0 flex items-center justify-between gap-3 border-t border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] p-3">
                <span className="truncate font-mono text-[0.68rem] text-[var(--t3,#8a8f98)]" title={targetDir}>
                  {targetDir && !(wizardActive && step === 3) ? t("pages.git.target", { path: targetDir }) : ""}
                </span>
                <div className="flex shrink-0 items-center gap-2">
                  {wizardActive && step > 1 ? (
                    <Button variant="outline" size="sm" onClick={() => setStep((s) => (s - 1) as 1 | 2)}>
                      {t("pages.templates.wPrev")}
                    </Button>
                  ) : (
                    <Button variant="outline" size="sm" onClick={closeCreate}>
                      {t("common.cancel")}
                    </Button>
                  )}
                  {wizardActive && step < 3 ? (
                    <Button size="sm" onClick={goNext}>
                      {t("pages.templates.wNext")}
                    </Button>
                  ) : (
                    <Button size="sm" disabled={submitting || opRunning || (wizardActive && !preview)} onClick={requestCreate}>
                      {submitting ? (
                        <>
                          <Loader2 className="size-3.5 animate-spin" /> {t("pages.templates.creating")}
                        </>
                      ) : (
                        t("pages.templates.createWs")
                      )}
                    </Button>
                  )}
                </div>
              </div>
            </>
          ) : null}
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={confirmCreate}
        title={t("pages.templates.createConfirmTitle")}
        description={
          selected
            ? t("pages.templates.createConfirmDesc", {
                name: selected.name,
                path: targetDir || joinPath(parentPath, dirName.trim()),
              })
            : undefined
        }
        confirmText={t("pages.templates.createWs")}
        onCancel={() => setConfirmCreate(false)}
        onConfirm={() => void submit()}
      />
    </div>
  );
}
