import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronRight, Eye, FolderSearch, LayoutTemplate, Loader2 } from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { cn } from "@/lib/utils";
import { useToast } from "@/components/ui/toast";
import { apiTemplatesCreate, apiTemplatesList, apiTemplatesPreview, type TemplatesCreateArgs } from "../ipc/api";
import { isTauri } from "../ipc/invoke";
import { IpcFailure, type OpState, type TemplateBlockSummary, type TemplateSource, type TemplateSummary, type TemplatesPreviewOut } from "../ipc/protocol";
import { operationResultWorkspaceId, useOperations, type OperationState } from "../providers/operation-provider";
import { opErrorLabel } from "../lib/status";
import { useOpenWorkspace } from "../lib/use-open-workspace";

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

function joinPath(parent: string, name: string): string {
  const p = parent.trim().replace(/[\\/]+$/, "");
  const sep = p.includes("\\") ? "\\" : "/";
  return p ? `${p}${sep}${name}` : name;
}

/** 长操作进度卡片：state 本地化 + message + 有 progress 才显示进度条。 */
function TemplateOperationCard({ op, targetDir }: { op: OperationState; targetDir: string }) {
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
        <span className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">{t("pages.templates.opTitle")}</span>
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
        <div className="mt-2 text-[0.78rem] leading-relaxed text-[#DC2626]">
          {opErrorLabel(op.error_code)}
          {targetDir ? <span className="block text-[var(--t2,#62666d)]">{t("pages.git.target", { path: targetDir })}</span> : null}
        </div>
      ) : null}
    </Card>
  );
}

const SOURCE_TABS: { key: TemplateSource; labelKey: string }[] = [
  { key: "builtin", labelKey: "pages.templates.srcBuiltin" },
  { key: "local", labelKey: "pages.templates.srcLocal" },
];

function SourceSegmented({
  value,
  hasLocal,
  onChange,
}: {
  value: TemplateSource;
  hasLocal: boolean;
  onChange: (s: TemplateSource) => void;
}) {
  const { t } = useTranslation();
  // 没有 local 模板时不渲染分段，避免出现点了没内容的空 tab
  const tabs = SOURCE_TABS.filter((tab) => tab.key === "builtin" || hasLocal);
  if (tabs.length <= 1) return null;
  return (
    <div
      className="inline-flex items-center gap-0.5 rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] p-0.5"
      role="tablist"
      aria-label={t("pages.templates.sourceAria")}
    >
      {tabs.map((tab) => (
        <button
          key={tab.key}
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
}: {
  template: TemplateSummary;
  selected: boolean;
  onSelect: () => void;
}) {
  const { t } = useTranslation();
  const [filesOpen, setFilesOpen] = useState(false);
  // 清单损坏的本地模板：不可选不可建，仅展示原因
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
        <div className="flex shrink-0 items-center gap-1.5">
          <Badge variant="secondary" className="shrink-0" title={template.source === "builtin" ? t("pages.templates.builtinTitle") : t("pages.templates.localTitle")}>
            {template.source === "builtin" ? t("pages.templates.builtinShort") : t("pages.templates.srcLocal")}
          </Badge>
          <Badge variant="secondary" className="shrink-0" title={t("pages.templates.versionTitle")}>
            v{template.version}
          </Badge>
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

  const [templates, setTemplates] = useState<TemplateSummary[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [sourceFilter, setSourceFilter] = useState<TemplateSource>("builtin");

  const [parentPath, setParentPath] = useState("");
  const [dirName, setDirName] = useState("");
  const [dirNameError, setDirNameError] = useState<string | null>(null);
  const [paramValues, setParamValues] = useState<Record<string, string>>({});
  const [selectedBlocks, setSelectedBlocks] = useState<string[]>([]);
  const [portValues, setPortValues] = useState<Record<string, number>>({});
  const [preview, setPreview] = useState<TemplatesPreviewOut | null>(null);
  const [previewing, setPreviewing] = useState(false);
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
      toast(t("pages.templates.createdTo", { name: wsId.split(/[\\/]/).filter(Boolean).pop() ?? wsId }), "ok");
      void openWs(wsId);
    }
  }, [op, activeOpId, openWs, toast, t]);

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

  // 切换模板时重置向导：参数、块选择（默认全选）、端口、预览
  useEffect(() => {
    setParamValues({});
    setPreview(null);
    const blocks = templates?.find((t) => t.id === selectedId)?.blocks;
    setSelectedBlocks(blocks?.map((b) => b.id) ?? []);
    setPortValues(
      Object.fromEntries((blocks ?? []).flatMap((b) => b.services.map((s) => [s, b.default_port])).filter(([, v]) => v != null) as [string, number][]),
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
        return; // 用户取消
      } catch {
        // 插件不可用时降级为手动输入
      }
    }
    const p = window.prompt(t("pages.git.promptParent"), parentPath);
    if (p) setParentPath(p);
  };

  const visibleTemplates = templates?.filter((tpl) => tpl.source === sourceFilter) ?? [];
  const selected = templates?.find((tpl) => tpl.id === selectedId) ?? null;
  const opRunning = !!op && (op.state === "queued" || op.state === "running");
  const targetDir = selected && dirName.trim() ? joinPath(parentPath, dirName.trim()) : "";

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
    } catch (e) {
      // 同步校验失败（PathEscape / TARGET_NOT_EMPTY 等）：IpcFailure.message 已是中文
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    } finally {
      setSubmitting(false);
    }
  };

  /** 勾/取消块：选择时自动闭合依赖；取消被依赖块时拒绝并说明。 */
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

  // 组合向导的派生状态：选中块的服务端口视图 + 端口查重
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

  const loading = templates === null && !loadError;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto flex max-w-4xl flex-col gap-4">
          {/* 标题行 */}
          <div className="flex items-center justify-between gap-3">
            <div>
              <h2 className="text-[1.05rem] font-bold tracking-tight text-[var(--t1,#222326)]">{t("nav.templates")}</h2>
              <p className="mt-0.5 text-[0.78rem] text-[var(--t3,#8a8f98)]">
                {t("pages.templates.pageDesc")}
              </p>
            </div>
            <LayoutTemplate className="size-8 shrink-0 text-[var(--line-strong,#d0d6e0)]" />
          </div>

          {loading ? (
            <div className="flex items-center justify-center gap-2 py-12 text-[0.8rem] text-[var(--t3,#8a8f98)]" role="status">
              <Loader2 className="size-4 animate-spin" /> {t("pages.templates.loading")}
            </div>
          ) : null}

          {loadError ? (
            <Card className="border-red-200 bg-[var(--st-danger-tint,#fdecec)] p-4 text-[0.8rem] text-[#DC2626]" role="alert">
              {t("pages.templates.loadFailed")} {loadError}
            </Card>
          ) : null}

          {/* 来源分段 + 模板卡片 */}
          {templates && templates.length > 0 ? (
            <>
              <SourceSegmented
                value={sourceFilter}
                hasLocal={templates.some((t) => t.source === "local")}
                onChange={(s) => {
                  setSourceFilter(s);
                  // 切换来源后默认选中该来源下第一个可用模板
                  setSelectedId(templates.find((t) => t.source === s && !t.invalid)?.id ?? null);
                }}
              />
              {visibleTemplates.length > 0 ? (
                <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
                  {visibleTemplates.map((tpl) => (
                    <TemplateCard
                      key={tpl.id}
                      template={tpl}
                      selected={tpl.id === selectedId}
                      onSelect={() => setSelectedId(tpl.id)}
                    />
                  ))}
                </div>
              ) : (
                <Card className="p-6 text-center text-[0.8rem] text-[var(--t3,#8a8f98)]">
                  {sourceFilter === "local"
                    ? t("pages.templates.noLocal")
                    : t("pages.templates.noTemplates")}
                </Card>
              )}
            </>
          ) : null}

          {/* 创建表单 */}
          {selected ? (
            <Card className="p-4">
              <div className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">{t("pages.templates.createHeading")}</div>
              <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-[1fr_auto]">
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
              {/* 创建参数（模板清单 params 声明） */}
              {selected.params?.length ? (
                <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-2">
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
              {/* 组合向导（blocks 模板）：勾块 → 端口 → 预览 → 创建 */}
              {selected.blocks?.length ? (
                <div className="mt-4 rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] p-3">
                  <div className="text-[0.78rem] font-semibold text-[var(--t1,#222326)]">
                    {t("pages.templates.blocksTitle")}
                    <span className="ml-2 font-normal text-[var(--t3,#8a8f98)]">{t("pages.templates.blocksHint")}</span>
                  </div>
                  <div className="mt-2 flex flex-col gap-1.5">
                    {selected.blocks.map((b) => {
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
                          <Badge variant="outline" className="text-[10px]">{b.kind}</Badge>
                          {b.requires.length ? (
                            <span className="text-[0.68rem] text-[var(--t3,#8a8f98)]">{t("pages.templates.dependsOn", { deps: b.requires.join(", ") })}</span>
                          ) : null}
                          {lockedBy.length > 0 ? (
                            <span className="ml-auto text-[0.68rem] text-[var(--t3,#8a8f98)]">{t("pages.templates.lockedBy", { names: lockedBy.join("、") })}</span>
                          ) : null}
                        </label>
                      );
                    })}
                  </div>
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
                          {t("pages.templates.portConflict", { port: portConflict.port, a: portConflict.a, b: portConflict.b })}
                        </div>
                      ) : portInvalid ? (
                        <div className="mt-1.5 text-[0.74rem] text-[#DC2626]" role="alert">
                          {t("pages.templates.portInvalid")}
                        </div>
                      ) : null}
                    </div>
                  ) : null}
                  <div className="mt-3 flex items-center gap-2">
                    <Button variant="soft" size="sm" className="gap-1" disabled={previewing || !!portConflict || portInvalid || wizardServices.length === 0} onClick={() => void runPreview()}>
                      {previewing ? <Loader2 className="size-3.5 animate-spin" /> : <Eye className="size-3.5" />}
                      {previewing ? t("pages.templates.generating") : t("pages.templates.generatePreview")}
                    </Button>
                    {preview ? (
                      <span className="text-[0.72rem] text-[var(--st-ok-deep,#1e7e35)]">
                        {t("pages.templates.previewOk", { services: Object.keys(preview.services).length, files: preview.files.length })}
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
                            <th className="py-0.5 pr-3 font-semibold">kind</th>
                            <th className="py-0.5 font-semibold">port</th>
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
                      {preview.warnings.map((w) => (
                        <div key={w} className="mt-1 text-[0.72rem] text-[var(--st-warn-dot,#eab308)]">{w}</div>
                      ))}
                    </div>
                  ) : null}
                </div>
              ) : null}
              <div className="mt-4 flex items-center justify-between gap-3">
                <span className="truncate font-mono text-[0.68rem] text-[var(--t3,#8a8f98)]" title={targetDir}>
                  {targetDir ? t("pages.git.target", { path: targetDir }) : ""}
                </span>
                <Button
                  onClick={() => void submit()}
                  disabled={submitting || opRunning || !dirName.trim() || !parentPath.trim()}
                >
                  {t("pages.templates.createWs")}
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
