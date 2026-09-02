import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";
import { RefreshCw, Copy, Radar, FolderOpen, FileInput, Square, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { toast as toastGlobal, useToast } from "@/components/ui/toast";
import { useOpenWorkspace } from "../lib/use-open-workspace";
import { useWorkspace } from "../providers/workspace-provider";
import { useYaml } from "../providers/yaml-provider";
import { apiImportReadme, apiImportReadmeApply, apiSystemDiscover, apiSystemKillProcess, apiYamlGet } from "../ipc/api";
import { formatIpcFailure } from "../lib/error-messages";
import { IpcFailure } from "../ipc/protocol";
import type { ForeignService, MergeChoice, ReadmePreviewOut } from "../ipc/protocol";
import { ScanPreviewPanel, type FieldChoice } from "@/components/scan-merge";

const REFRESH_MS = 30_000;

/** 发现页可筛选的运行时类型（与 core 的 INTERESTING_PREFIXES + other 对齐）。 */
const KIND_ORDER = ["java", "node", "python", "deno", "bun", "other"] as const;
const KIND_LABEL_KEY: Record<string, string> = {
  java: "kindJava",
  node: "kindNode",
  python: "kindPython",
  deno: "kindDeno",
  bun: "kindBun",
  other: "kindOther",
};

function runtimeColor(kind: string): string {
  if (kind === "java") return "#2E90FA";
  if (kind === "node") return "#27A644";
  if (kind === "python") return "#F79009";
  return "var(--t3,#8a8f98)";
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${bytes} B`;
}

/** CPU / 内存读数：首次差分采样或读取失败显示占位。 */
function MetricCell({ value, format, placeholder }: { value: number | null; format: (v: number) => string; placeholder: string }) {
  const { t } = useTranslation();
  if (value == null) {
    return (
      <span title={t("pages.discover.metricPending")} className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
        {placeholder}
      </span>
    );
  }
  return <span className="font-mono text-[0.75rem] text-[var(--t1,#222326)]">{format(value)}</span>;
}

function DetailField({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex gap-3 text-[0.78rem] leading-relaxed">
      <span className="w-14 shrink-0 pt-0.5 text-[var(--t3,#8a8f98)]">{label}</span>
      <span className="min-w-0 flex-1 break-all text-[var(--t1,#222326)]">{children}</span>
    </div>
  );
}

/**
 * 系统服务发现：列出本机正在监听端口的 java/node/python 等进程，
 * 展示 CPU / 内存占用，并与当前工作区 supertask.yaml 的服务端口做关联标注；
 * 支持把进程工作目录一键打开为工作区（快速切换）。
 */
export function DiscoverPage() {
  const ws = useWorkspace();
  const yaml = useYaml();
  const { toast } = useToast();
  const { t } = useTranslation();
  const openWorkspace = useOpenWorkspace();
  const [items, setItems] = useState<ForeignService[]>([]);
  const [loading, setLoading] = useState(false);
  const [showOther, setShowOther] = useState(false);
  const [kindFilter, setKindFilter] = useState<Set<string>>(() => new Set());
  const [portQuery, setPortQuery] = useState("");
  const [detail, setDetail] = useState<ForeignService | null>(null);
  const [killTarget, setKillTarget] = useState<ForeignService | null>(null);
  const timer = useRef<ReturnType<typeof setInterval> | null>(null);

  // 2.1：从 README 导入（ipc.md §10.13）——scan 骨架 + README 草稿走同一 merge 向导
  const [readmePreview, setReadmePreview] = useState<ReadmePreviewOut | null>(null);
  const [readmeLoading, setReadmeLoading] = useState(false);
  const [readmeAddChecked, setReadmeAddChecked] = useState<Record<string, boolean>>({});
  const [readmeScriptChecked, setReadmeScriptChecked] = useState<Record<string, boolean>>({});
  const [readmeFieldChoices, setReadmeFieldChoices] = useState<Record<string, Record<string, FieldChoice>>>({});
  const [readmeApplying, setReadmeApplying] = useState(false);
  // 命令面板「从 README 导入」经 /discover?readme=1 自动展开向导
  const [searchParams, setSearchParams] = useSearchParams();
  const autoReadmeDone = useRef(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setItems(await apiSystemDiscover());
    } catch (e) {
      // 不再静默：读端口表失败必须让用户知道这不是「没有服务」
      const msg = e instanceof Error && e.message ? e.message : "";
      toastGlobal(msg ? t("pages.discover.queryFailedWithMsg", { msg }) : t("pages.discover.queryFailed"), "err");
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    void refresh();
    timer.current = setInterval(() => void refresh(), REFRESH_MS);
    return () => {
      if (timer.current) clearInterval(timer.current);
    };
  }, [refresh]);

  // 切换工作区后 README 预览失效
  useEffect(() => {
    closeReadmeImport();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ws.state.workspaceId]);

  // ?readme=1（命令面板入口）：工作区就绪后自动展开一次
  useEffect(() => {
    if (autoReadmeDone.current) return;
    if (searchParams.get("readme") !== "1") return;
    if (!ws.state.workspaceId) return;
    autoReadmeDone.current = true;
    void openReadmeImport();
    setSearchParams({}, { replace: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [searchParams, ws.state.workspaceId]);

  // 当前工作区占用的端口 → 服务 id 映射（用于行级关联标注）
  const portOwner = new Map<number, string>();
  for (const [id, svc] of Object.entries(ws.state.spec?.services ?? {})) {
    if (svc.port != null) portOwner.set(svc.port, id);
  }

  // 筛选：kind 多选 + 端口子串匹配；空集合 = 全部，空 query = 不过滤
  const filteredItems = useMemo(() => {
    const q = portQuery.trim();
    return items.filter((s) => {
      if (kindFilter.size > 0 && !kindFilter.has(s.kind)) return false;
      if (q && !s.ports.some((p) => String(p).includes(q))) return false;
      return true;
    });
  }, [items, kindFilter, portQuery]);

  const kindCounts = useMemo(() => {
    const m = new Map<string, number>();
    for (const s of items) m.set(s.kind, (m.get(s.kind) ?? 0) + 1);
    return m;
  }, [items]);

  const hasFilter = kindFilter.size > 0 || portQuery.trim() !== "";

  const toggleKind = (k: string) => {
    setKindFilter((prev) => {
      const next = new Set(prev);
      if (next.has(k)) next.delete(k);
      else next.add(k);
      return next;
    });
    if (k === "other") setShowOther(true);
  };

  const clearFilters = () => {
    setKindFilter(new Set());
    setPortQuery("");
  };

  const known = filteredItems.filter((s) => s.kind !== "other");
  const others = filteredItems.filter((s) => s.kind === "other");

  // 弹框里的对象要跟随刷新（进程可能已退出 / 读数更新），按 pid 重新对位
  const detailLive = detail ? (items.find((s) => s.pid === detail.pid) ?? detail) : null;
  const detailMatched = detailLive
    ? detailLive.ports.map((p) => ({ p, id: portOwner.get(p) })).filter((x) => x.id)
    : [];

  const copy = async (text: string, label: string) => {
    await navigator.clipboard?.writeText(text);
    toast(t("pages.discover.copied", { label }), "ok");
  };

  const openAsWorkspace = async (s: ForeignService) => {
    if (!s.cwd) return;
    setDetail(null);
    await openWorkspace(s.cwd);
  };

  /** 杀整棵进程树：core 护栏（pid≤4 / 自身 / 非监听 pid 拒绝）+ 二次确认。 */
  const confirmKill = async () => {
    const target = killTarget;
    setKillTarget(null);
    if (!target) return;
    try {
      await apiSystemKillProcess(target.pid);
      toast(t("pages.discover.killed", { name: target.name, pid: target.pid }), "ok");
      if (detail?.pid === target.pid) setDetail(null);
    } catch (e) {
      toast(e instanceof IpcFailure ? e.message : String(e), "err");
    }
    void refresh();
  };

  // 2.1：README 导入预览（确定性纯计算；未发现 README 时 warnings 给人话提示，非错误）
  const openReadmeImport = async () => {
    const wid = ws.state.workspaceId;
    if (!wid || readmeLoading) return;
    setReadmeLoading(true);
    try {
      const out = await apiImportReadme(wid);
      setReadmePreview(out);
      setReadmeAddChecked({});
      setReadmeScriptChecked({});
      setReadmeFieldChoices({});
    } catch (e) {
      toast(e instanceof IpcFailure ? formatIpcFailure(e) : String(e), "err");
    } finally {
      setReadmeLoading(false);
    }
  };

  const closeReadmeImport = () => {
    setReadmePreview(null);
    setReadmeAddChecked({});
    setReadmeScriptChecked({});
    setReadmeFieldChoices({});
  };

  const readmeApplyCount = useMemo(() => {
    if (!readmePreview) return 0;
    let n = 0;
    for (const it of readmePreview.items) {
      if ((it.status === "added" || it.status === "id_conflict") && readmeAddChecked[it.service_id]) n += 1;
      if (it.status === "match_diff" && it.field_diffs.some((f) => readmeFieldChoices[it.service_id]?.[f] === "update")) n += 1;
    }
    for (const it of readmePreview.script_items) {
      if (it.status === "added" && readmeScriptChecked[it.script_id]) n += 1;
    }
    return n;
  }, [readmePreview, readmeAddChecked, readmeScriptChecked, readmeFieldChoices]);

  const applyReadmeChoices = async () => {
    const wid = ws.state.workspaceId;
    if (!wid || !readmePreview || readmeApplying) return;
    const choices: MergeChoice[] = [];
    for (const it of readmePreview.items) {
      if (it.status === "added") {
        if (readmeAddChecked[it.service_id]) choices.push({ id: it.service_id, action: "add" });
      } else if (it.status === "id_conflict") {
        if (readmeAddChecked[it.service_id]) choices.push({ id: it.candidate_id ?? it.service_id, action: "add" });
      } else if (it.status === "match_diff") {
        const fields = it.field_diffs.filter((f) => readmeFieldChoices[it.service_id]?.[f] === "update");
        if (fields.length > 0) choices.push({ id: it.service_id, action: "update", fields });
      }
    }
    for (const it of readmePreview.script_items) {
      if (it.status === "added" && readmeScriptChecked[it.script_id]) {
        choices.push({ id: it.script_id, action: "add", target: "script" });
      }
    }
    if (choices.length === 0) {
      toast(t("pages.config.selectChangesFirst"), "warn");
      return;
    }
    setReadmeApplying(true);
    try {
      // base_hash 优先取 yaml-provider 当前值；无 hash 时先 yaml.get
      let baseHash = yaml.state.hash;
      if (!baseHash) baseHash = (await apiYamlGet()).hash;
      await apiImportReadmeApply(wid, readmePreview.readme_path, choices, baseHash);
      toast(t("pages.discover.readmeApplied", { n: choices.length }), "ok");
      closeReadmeImport();
      await yaml.actions.reload();
      await ws.actions.refreshSpec();
    } catch (e) {
      // 本页无冲突对话框：YAML_CONFLICT 走 toast 人话提示（与真实语义对齐）
      toast(e instanceof IpcFailure ? formatIpcFailure(e) : String(e), "err");
    } finally {
      setReadmeApplying(false);
    }
  };

  const renderRow = (s: ForeignService) => {
    const matched = s.ports
      .map((p) => ({ p, id: portOwner.get(p) }))
      .filter((x) => x.id);
    return (
      <tr
        key={`${s.pid}-${s.name}`}
        onClick={() => setDetail(s)}
        title={t("pages.discover.rowTitle")}
        className={cn(
          "cursor-pointer border-b border-[var(--line,#e6e6e6)] transition-colors duration-100 last:border-0",
          matched.length > 0
            ? "bg-[rgb(94_106_210_/_0.04)] hover:bg-[rgb(94_106_210_/_0.10)]"
            : "hover:bg-[var(--surface-2,#f3f4f5)]",
        )}
      >
        <td className="px-4 py-2.5">
          <span className="inline-flex items-center gap-2">
            <span className="size-1.5 rounded-full" style={{ background: runtimeColor(s.kind) }} />
            <span className="font-mono text-[0.78rem] font-medium text-[var(--t1,#222326)]">{s.name}</span>
          </span>
        </td>
        <td className="px-4 py-2.5 font-mono text-[0.78rem] text-[var(--t2,#62666d)]">{s.pid}</td>
        <td className="px-4 py-2.5">
          <MetricCell value={s.cpu_percent} format={(v) => `${v.toFixed(1)}%`} placeholder="—" />
        </td>
        <td className="px-4 py-2.5">
          <MetricCell value={s.memory_bytes} format={formatBytes} placeholder="—" />
        </td>
        <td className="px-4 py-2.5">
          <span className="flex flex-wrap gap-1">
            {s.ports.slice(0, 8).map((p) => (
              <span key={p} className="inline-flex h-5 items-center rounded-full bg-[var(--surface-2,#f3f4f5)] px-1.5 font-mono text-[0.68rem] leading-none text-[var(--primary,#5E6AD2)]">
                {p}
              </span>
            ))}
            {s.ports.length > 8 ? (
              <span className="font-mono text-[0.68rem] text-[var(--t3,#8a8f98)]">+{s.ports.length - 8}</span>
            ) : null}
          </span>
        </td>
        <td className="max-w-[220px] px-4 py-2.5">
          {s.cwd ? (
            <span
              title={s.cmd_line ?? undefined}
              className="block truncate font-mono text-[0.72rem] text-[var(--t2,#62666d)]"
            >
              {s.cwd}
            </span>
          ) : (
            <span className="text-[0.75rem] text-[var(--t3,#8a8f98)]">—</span>
          )}
        </td>
        <td className="px-4 py-2.5">
          {matched.length > 0 ? (
            <span className="inline-flex items-center gap-1.5">
              <Badge variant="soon" className="shrink-0">{t("pages.discover.matchedBadge")}</Badge>
              <span className="truncate text-[0.75rem] text-[var(--st-accent-hover,#4f5ac8)]">
                {matched.map((m) => m.id).join(", ")}
              </span>
            </span>
          ) : (
            <span className="text-[0.75rem] text-[var(--t3,#8a8f98)]">—</span>
          )}
        </td>
        <td className="px-2 py-2.5 text-right">
          <span className="inline-flex items-center gap-0.5" onClick={(e) => e.stopPropagation()}>
            {s.cwd ? (
              <button
                type="button"
                title={t("pages.discover.openAsWorkspaceTitle", { cwd: s.cwd })}
                onClick={() => void openAsWorkspace(s)}
                className="grid size-6 cursor-pointer place-items-center rounded-[var(--r-sm,8px)] text-[var(--t3,#8a8f98)] transition-colors duration-150 hover:bg-[rgb(0_0_0_/_0.06)] hover:text-[var(--st-accent,#5e6ad2)] active:scale-95"
              >
                <FolderOpen className="size-3" />
              </button>
            ) : null}
            <button
              type="button"
              title={t("pages.discover.killTreeTitle", { name: s.name, pid: s.pid })}
              onClick={() => setKillTarget(s)}
              className="grid size-6 cursor-pointer place-items-center rounded-[var(--r-sm,8px)] text-[var(--t3,#8a8f98)] transition-colors duration-150 hover:bg-[var(--st-danger-tint,#fdecec)] hover:text-[var(--st-danger,#dc2626)] active:scale-95"
            >
              <Square className="size-3" />
            </button>
            <button
              type="button"
              title={t("pages.discover.copyPidTitle", { pid: s.pid })}
              onClick={() => void copy(String(s.pid), ` PID ${s.pid}`)}
              className="grid size-6 cursor-pointer place-items-center rounded-[var(--r-sm,8px)] text-[var(--t3,#8a8f98)] transition-colors duration-150 hover:bg-[rgb(0_0_0_/_0.06)] hover:text-[var(--t1,#222326)] active:scale-95"
            >
              <Copy className="size-3" />
            </button>
          </span>
        </td>
      </tr>
    );
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto flex max-w-5xl flex-col gap-4">
          <div className="flex items-center justify-between gap-3">
            <div>
              <h2 className="text-[1.05rem] font-bold tracking-tight text-[var(--t1,#222326)]">{t("pages.discover.title")}</h2>
              <p className="mt-0.5 text-[0.78rem] text-[var(--t3,#8a8f98)]">
                {t("pages.discover.desc", { secs: REFRESH_MS / 1000 })}
              </p>
            </div>
            <Button variant="outline" size="sm" onClick={() => void openReadmeImport()} disabled={!ws.state.workspaceId || readmeLoading} className="gap-1" title={ws.state.workspaceId ? t("pages.discover.readmeImportHint") : t("pages.config.openWsFirst")}>
              <FileInput className={cn("size-3.5", readmeLoading && "animate-pulse")} />
              {readmeLoading ? t("pages.discover.readmeImporting") : t("pages.discover.readmeImport")}
            </Button>
            <Button variant="soft" size="sm" onClick={() => void refresh()} disabled={loading} className="gap-1">
              <RefreshCw className={cn(loading && "animate-spin")} /> {t("common.refresh")}
            </Button>
          </div>

          {items.length > 0 ? (
            <div className="flex flex-wrap items-center gap-2">
              <span
                role="group"
                aria-label={t("pages.discover.filterByKindAria")}
                className="flex flex-wrap items-center gap-1"
              >
                {KIND_ORDER.map((k) => {
                  const active = kindFilter.has(k);
                  const count = kindCounts.get(k) ?? 0;
                  return (
                    <button
                      key={k}
                      type="button"
                      aria-pressed={active}
                      onClick={() => toggleKind(k)}
                      disabled={count === 0 && !active}
                      className={cn(
                        "inline-flex h-5 cursor-pointer items-center gap-1 rounded-full border px-2 text-[0.72rem] leading-none transition-colors duration-150 active:scale-95",
                        active
                          ? "border-[var(--primary,#5E6AD2)] bg-[rgb(94_106_210_/_0.08)] text-[var(--primary,#5E6AD2)] hover:bg-[rgb(94_106_210_/_0.16)]"
                          : "border-[var(--line-strong,#d0d6e0)] text-[var(--t2,#62666d)] hover:border-[var(--t3,#8a8f98)] hover:bg-[var(--surface-2,#f3f4f5)]",
                        count === 0 && !active && "cursor-not-allowed opacity-50 hover:border-[var(--line-strong,#d0d6e0)] hover:bg-transparent active:scale-100",
                      )}
                    >
                      <span className="size-1.5 rounded-full" style={{ background: runtimeColor(k) }} />
                      {t(`pages.discover.${KIND_LABEL_KEY[k]}`)}
                      <span className="font-mono text-[0.66rem] opacity-70">{count}</span>
                    </button>
                  );
                })}
              </span>
              <Input
                value={portQuery}
                onChange={(e) => setPortQuery(e.target.value)}
                placeholder={t("pages.discover.filterPortPlaceholder")}
                aria-label={t("pages.discover.filterPortAria")}
                inputMode="numeric"
                className="h-7 w-40 rounded-full text-[0.75rem] font-mono"
              />
              {hasFilter ? (
                <Button variant="ghost" size="xs" onClick={clearFilters} className="gap-1">
                  <X className="size-3" /> {t("pages.discover.filterClear")}
                </Button>
              ) : null}
            </div>
          ) : null}

          {readmePreview ? (
            readmePreview.items.length === 0 && readmePreview.script_items.length === 0 ? (
              <Card className="flex flex-col items-center gap-2 p-8">
                <FileInput className="size-8 text-[var(--line-strong,#d0d6e0)]" />
                <div className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">
                  {t("pages.discover.readmeEmptyDraftTitle")}
                </div>
                {readmePreview.warnings.map((w, i) => (
                  <div key={i} className="text-[0.78rem] text-[var(--t3,#8a8f98)]">{w}</div>
                ))}
                <Button variant="outline" size="sm" className="mt-1" onClick={closeReadmeImport}>
                  {t("common.close")}
                </Button>
              </Card>
            ) : (
              <div className="flex h-[68vh] flex-col">
                <ScanPreviewPanel
                  preview={readmePreview}
                  titleKey="pages.discover.readmeImportTitle"
                  ariaKey="pages.discover.readmeImportAria"
                  headerExtra={
                    readmePreview.readme_path ? (
                      <span className="font-mono text-[0.68rem] text-[var(--t3,#8a8f98)]">
                        {t("pages.discover.readmeSource", { path: readmePreview.readme_path })}
                      </span>
                    ) : null
                  }
                  scriptItems={readmePreview.script_items}
                  scriptChecked={readmeScriptChecked}
                  onToggleScript={(id, v) => setReadmeScriptChecked((m) => ({ ...m, [id]: v }))}
                  addChecked={readmeAddChecked}
                  onToggleAdd={(id, v) => setReadmeAddChecked((m) => ({ ...m, [id]: v }))}
                  onSelectAllAddable={(v) => {
                    const next: Record<string, boolean> = {};
                    for (const it of readmePreview.items) {
                      if (it.status === "added" || it.status === "id_conflict") next[it.service_id] = v;
                    }
                    setReadmeAddChecked(next);
                  }}
                  fieldChoices={readmeFieldChoices}
                  onFieldChoice={(id, f, c) =>
                    setReadmeFieldChoices((m) => ({ ...m, [id]: { ...(m[id] ?? {}), [f]: c } }))
                  }
                  applying={readmeApplying}
                  applyCount={readmeApplyCount}
                  onApply={() => void applyReadmeChoices()}
                  onClose={closeReadmeImport}
                />
              </div>
            )
          ) : filteredItems.length === 0 && items.length > 0 ? (
            <div className="flex flex-col items-center gap-3 rounded-[var(--r-lg,16px)] border border-dashed border-[var(--line-strong,#d0d6e0)] p-10">
              <Radar className="size-9 text-[var(--line-strong,#d0d6e0)]" />
              <div className="text-[0.88rem] font-semibold text-[var(--t1,#222326)]">{t("pages.discover.filterNoMatchTitle")}</div>
              <div className="text-[0.78rem] text-[var(--t3,#8a8f98)]">{t("pages.discover.filterNoMatchDesc")}</div>
              <Button variant="outline" size="sm" className="mt-1 gap-1" onClick={clearFilters}>
                <X className="size-3.5" /> {t("pages.discover.filterClear")}
              </Button>
            </div>
          ) : items.length === 0 && !loading ? (
            <div className="flex flex-col items-center gap-3 rounded-[var(--r-lg,16px)] border border-dashed border-[var(--line-strong,#d0d6e0)] p-10">
              <Radar className="size-9 text-[var(--line-strong,#d0d6e0)]" />
              <div className="text-[0.88rem] font-semibold text-[var(--t1,#222326)]">{t("pages.discover.emptyTitle")}</div>
              <div className="text-[0.78rem] text-[var(--t3,#8a8f98)]">{t("pages.discover.emptyDesc")}</div>
            </div>
          ) : (
            <>
              <Card className="overflow-hidden p-0">
                <table className="w-full border-collapse text-left">
                  <thead>
                    <tr className="border-b border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
                      <th className="px-4 py-2.5 font-semibold">{t("pages.discover.colProcess")}</th>
                      <th className="px-4 py-2.5 font-semibold">PID</th>
                      <th className="px-4 py-2.5 font-semibold">CPU</th>
                      <th className="px-4 py-2.5 font-semibold">{t("pages.discover.colMemory")}</th>
                      <th className="px-4 py-2.5 font-semibold">{t("pages.discover.colPorts")}</th>
                      <th className="px-4 py-2.5 font-semibold">{t("pages.discover.colCwd")}</th>
                      <th className="px-4 py-2.5 font-semibold">{t("pages.discover.colMatch")}</th>
                      <th className="px-2 py-2.5" />
                    </tr>
                  </thead>
                  <tbody>{known.map(renderRow)}</tbody>
                </table>
              </Card>

              {others.length > 0 ? (
                <Card className="p-0">
                  <button
                    type="button"
                    onClick={() => setShowOther((v) => !v)}
                    aria-expanded={showOther}
                    className="flex w-full cursor-pointer items-center justify-between gap-3 px-4 py-2.5 text-left transition-colors duration-150 hover:bg-[var(--surface-2,#f3f4f5)]"
                  >
                    <span className="text-[0.78rem] font-medium text-[var(--t1,#222326)]">
                      {t("pages.discover.others", { n: others.length })}
                    </span>
                    <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">
                      {showOther ? t("common.collapse") : t("common.expand")} · {t("pages.discover.othersHint")}
                    </span>
                  </button>
                  {showOther ? (
                    <table className="w-full border-collapse border-t border-[var(--line,#e6e6e6)] text-left">
                      <tbody>{others.map(renderRow)}</tbody>
                    </table>
                  ) : null}
                </Card>
              ) : null}
            </>
          )}

          <p className="text-[0.72rem] leading-relaxed text-[var(--t3,#8a8f98)]">
            {t("pages.discover.footnote")}
          </p>
        </div>
      </div>

      <ConfirmDialog
        open={killTarget != null}
        destructive
        title={t("pages.discover.killConfirmTitle", { name: killTarget?.name ?? "", pid: killTarget?.pid ?? "" })}
        description={
          <>
            {t("pages.discover.killConfirmDesc")}
            {killTarget?.cwd ? (
              <span className="mt-1 block font-mono text-[0.72rem] text-[var(--t2,#62666d)]">{killTarget.cwd}</span>
            ) : null}
          </>
        }
        confirmText={t("pages.discover.killForce")}
        onConfirm={() => void confirmKill()}
        onCancel={() => setKillTarget(null)}
      />

      <Dialog open={detailLive != null} onOpenChange={(o) => !o && setDetail(null)}>
        <DialogContent className="sm:max-w-xl">
          {detailLive ? (
            <>
              <DialogHeader>
                <DialogTitle className="flex items-center gap-2">
                  <span className="size-2 rounded-full" style={{ background: runtimeColor(detailLive.kind) }} />
                  <span className="font-mono">{detailLive.name}</span>
                  <span className="font-mono text-[0.75rem] font-normal text-[var(--t3,#8a8f98)]">
                    PID {detailLive.pid}
                  </span>
                </DialogTitle>
                <DialogDescription>
                  {t("pages.discover.detailDesc", { secs: REFRESH_MS / 1000 })}
                </DialogDescription>
              </DialogHeader>

              <div className="flex flex-col gap-2">
                <DetailField label={t("pages.discover.fRuntime")}>{detailLive.kind}</DetailField>
                <DetailField label="CPU">
                  <MetricCell value={detailLive.cpu_percent} format={(v) => `${v.toFixed(1)}%`} placeholder={t("pages.discover.cpuSampling")} />
                </DetailField>
                <DetailField label={t("pages.discover.fMemory")}>
                  <MetricCell value={detailLive.memory_bytes} format={formatBytes} placeholder={t("pages.discover.memFailed")} />
                </DetailField>
                <DetailField label={t("pages.discover.colPorts")}>
                  <span className="flex flex-wrap gap-1">
                    {detailLive.ports.map((p) => (
                      <span key={p} className="inline-flex h-5 items-center rounded-full bg-[var(--surface-2,#f3f4f5)] px-1.5 font-mono text-[0.7rem] leading-none text-[var(--primary,#5E6AD2)]">
                        {p}
                      </span>
                    ))}
                  </span>
                </DetailField>
                <DetailField label={t("pages.discover.colCwd")}>
                  {detailLive.cwd ? (
                    <span className="inline-flex items-start gap-1.5">
                      <span className="font-mono text-[0.72rem]">{detailLive.cwd}</span>
                      <button
                        type="button"
                        title={t("pages.discover.copyPath")}
                        onClick={() => void copy(detailLive.cwd!, t("pages.discover.labelPath"))}
                        className="shrink-0 cursor-pointer text-[var(--t3,#8a8f98)] transition-colors duration-150 hover:text-[var(--t1,#222326)] active:scale-95"
                      >
                        <Copy className="size-3" />
                      </button>
                    </span>
                  ) : (
                    <span className="text-[var(--t3,#8a8f98)]">—</span>
                  )}
                </DetailField>
                <DetailField label={t("pages.discover.fCmdline")}>
                  {detailLive.cmd_line ? (
                    <span className="inline-flex items-start gap-1.5">
                      <span className="max-h-24 overflow-y-auto font-mono text-[0.72rem]">{detailLive.cmd_line}</span>
                      <button
                        type="button"
                        title={t("pages.discover.copyCmdline")}
                        onClick={() => void copy(detailLive.cmd_line!, t("pages.discover.labelCmdline"))}
                        className="shrink-0 cursor-pointer text-[var(--t3,#8a8f98)] transition-colors duration-150 hover:text-[var(--t1,#222326)] active:scale-95"
                      >
                        <Copy className="size-3" />
                      </button>
                    </span>
                  ) : (
                    <span className="text-[var(--t3,#8a8f98)]">—</span>
                  )}
                </DetailField>
                <DetailField label={t("pages.discover.fWorkspace")}>
                  {detailMatched.length > 0 ? (
                    <span className="inline-flex flex-wrap items-center gap-1.5">
                      <Badge variant="soon" className="shrink-0">{t("pages.discover.matchedCurrent")}</Badge>
                      <span className="text-[0.75rem] text-[var(--st-accent-hover,#4f5ac8)]">
                        {detailMatched.map((m) => `${m.id} (${m.p})`).join("、")}
                      </span>
                    </span>
                  ) : detailLive.cwd ? (
                    <span className="text-[var(--t3,#8a8f98)]">{t("pages.discover.noPortMatch")}</span>
                  ) : (
                    <span className="text-[var(--t3,#8a8f98)]">—</span>
                  )}
                </DetailField>
              </div>

              <DialogFooter>
                <Button variant="outline" size="sm" onClick={() => void copy(String(detailLive.pid), ` PID ${detailLive.pid}`)}>
                  {t("pages.discover.copyPid")}
                </Button>
                <Button
                  variant="destructive"
                  size="sm"
                  onClick={() => {
                    setDetail(null);
                    setKillTarget(detailLive);
                  }}
                >
                  {t("pages.discover.killProcessTree")}
                </Button>
                {detailLive.cwd ? (
                  <Button size="sm" className="gap-1" onClick={() => void openAsWorkspace(detailLive)}>
                    <FolderOpen /> {t("pages.discover.openAsWorkspace")}
                  </Button>
                ) : null}
              </DialogFooter>
            </>
          ) : null}
        </DialogContent>
      </Dialog>
    </div>
  );
}
