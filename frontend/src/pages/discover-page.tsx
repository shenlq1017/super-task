import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";
import {
  ArrowDownWideNarrow,
  ChevronDown,
  Copy,
  Cpu,
  FileInput,
  FolderOpen,
  HardDrive,
  Magnet,
  Radar,
  RefreshCw,
  Square,
  X,
} from "lucide-react";
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
import { copyText } from "@/lib/copy-text";
import { fmtTime } from "@/lib/status";
import { toast as toastGlobal, useToast } from "@/components/ui/toast";
import { useOpenWorkspace } from "../lib/use-open-workspace";
import { useWorkspace } from "../providers/workspace-provider";
import { useYaml } from "../providers/yaml-provider";
import { apiAdoptPreview, apiAdoptApply, apiImportReadme, apiImportReadmeApply, apiSystemDiscover, apiSystemKillProcess, apiYamlGet } from "../ipc/api";
import { formatIpcFailure } from "../lib/error-messages";
import { IpcFailure } from "../ipc/protocol";
import type { AdoptChoice, AdoptPreviewOut, ForeignService, MergeChoice, ReadmePreviewOut } from "../ipc/protocol";
import { ScanPreviewPanel, type FieldChoice } from "@/components/scan-merge";
import { AdoptPanel } from "@/components/adopt-panel";

const REFRESH_MS = 30_000;
const PORT_DEBOUNCE_MS = 250;
const LS_KIND = "st:discover:kindFilter";
const LS_SHOW_OTHER = "st:discover:showOther";
const LS_SORT = "st:discover:sortBy";

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

type SortBy = "match" | "cpu" | "memory";

type PortMatch = { p: number; id: string; owned: boolean };

function runtimeColor(kind: string): string {
  if (kind === "java") return "#2E90FA";
  if (kind === "node") return "#27A644";
  if (kind === "python") return "#F79009";
  if (kind === "deno") return "#000000";
  if (kind === "bun") return "#FBF0DF";
  return "var(--t3,#8a8f98)";
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${bytes} B`;
}

function loadKindFilter(): Set<string> {
  try {
    const raw = localStorage.getItem(LS_KIND);
    if (!raw) return new Set();
    const arr = JSON.parse(raw) as unknown;
    if (!Array.isArray(arr)) return new Set();
    return new Set(arr.filter((k): k is string => typeof k === "string" && (KIND_ORDER as readonly string[]).includes(k)));
  } catch {
    return new Set();
  }
}

function loadShowOther(): boolean {
  try {
    return localStorage.getItem(LS_SHOW_OTHER) === "1";
  } catch {
    return false;
  }
}

function loadSortBy(): SortBy {
  try {
    const v = localStorage.getItem(LS_SORT);
    if (v === "cpu" || v === "memory" || v === "match") return v;
  } catch {
    /* ignore */
  }
  return "match";
}

/** CPU / 内存读数：首次差分采样或读取失败显示占位。 */
function MetricCell({ value, format, placeholder }: { value: number | null; format: (v: number) => string; placeholder: string }) {
  const { t } = useTranslation();
  if (value == null) {
    return (
      <span title={t("pages.discover.metricPending")} className="text-[0.75rem] whitespace-nowrap text-[var(--t3,#8a8f98)]">
        {placeholder}
      </span>
    );
  }
  return <span className="font-mono text-[0.75rem] whitespace-nowrap text-[var(--t1,#222326)]">{format(value)}</span>;
}

function DetailField({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex gap-3 text-[0.78rem] leading-relaxed">
      <span className="w-16 shrink-0 pt-0.5 text-[var(--t3,#8a8f98)]">{label}</span>
      <span className="min-w-0 flex-1 break-all text-[var(--t1,#222326)]">{children}</span>
    </div>
  );
}

function SummaryChip({
  label,
  value,
  tone = "default",
  title,
}: {
  label: string;
  value: number | string;
  tone?: "default" | "accent" | "warn" | "danger";
  title?: string;
}) {
  return (
    <span
      title={title}
      className={cn(
        "inline-flex h-6 items-center gap-1.5 rounded-full border px-2.5 text-[0.72rem] leading-none",
        tone === "accent" && "border-[rgb(94_106_210_/_0.35)] bg-[rgb(94_106_210_/_0.08)] text-[var(--st-accent,#5e6ad2)]",
        tone === "warn" && "border-[#f0d58a] bg-[#fdf6e3] text-[#B7791F]",
        tone === "danger" && "border-red-200 bg-[var(--st-danger-tint,#fdecec)] text-[#DC2626]",
        tone === "default" && "border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] text-[var(--t2,#62666d)]",
      )}
    >
      <span className="opacity-80">{label}</span>
      <span className="font-mono font-semibold tabular-nums text-[var(--t1,#222326)]">{value}</span>
    </span>
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
  const [loadFailed, setLoadFailed] = useState(false);
  const [lastRefresh, setLastRefresh] = useState<number | null>(null);
  const [showOther, setShowOther] = useState(loadShowOther);
  const [kindFilter, setKindFilter] = useState<Set<string>>(loadKindFilter);
  const [sortBy, setSortBy] = useState<SortBy>(loadSortBy);
  const [portQuery, setPortQuery] = useState("");
  const [portQueryDebounced, setPortQueryDebounced] = useState("");
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
  // 孤儿进程纳管（ipc.md §10.16）：发现结果 → generic 服务草稿 → 人确认写回
  const [adoptPreview, setAdoptPreview] = useState<AdoptPreviewOut | null>(null);
  const [adoptLoading, setAdoptLoading] = useState(false);
  const [adoptChecked, setAdoptChecked] = useState<Record<number, boolean>>({});
  const [adoptApplying, setAdoptApplying] = useState(false);
  // 命令面板「从 README 导入」经 /discover?readme=1 自动展开向导
  const [searchParams, setSearchParams] = useSearchParams();
  const autoReadmeDone = useRef(false);

  const dialogsOpen =
    detail != null ||
    killTarget != null ||
    readmePreview != null ||
    readmeLoading ||
    adoptPreview != null ||
    adoptLoading;

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setItems(await apiSystemDiscover());
      setLoadFailed(false);
      setLastRefresh(Date.now());
    } catch (e) {
      setLoadFailed(true);
      const msg = e instanceof Error && e.message ? e.message : "";
      toastGlobal(msg ? t("pages.discover.queryFailedWithMsg", { msg }) : t("pages.discover.queryFailed"), "err");
    } finally {
      setLoading(false);
    }
  }, [t]);

  // 端口筛选防抖
  useEffect(() => {
    const id = setTimeout(() => setPortQueryDebounced(portQuery), PORT_DEBOUNCE_MS);
    return () => clearTimeout(id);
  }, [portQuery]);

  // 持久化筛选 / 展开 / 排序偏好
  useEffect(() => {
    try {
      localStorage.setItem(LS_KIND, JSON.stringify([...kindFilter]));
    } catch {
      /* ignore */
    }
  }, [kindFilter]);

  useEffect(() => {
    try {
      localStorage.setItem(LS_SHOW_OTHER, showOther ? "1" : "0");
    } catch {
      /* ignore */
    }
  }, [showOther]);

  useEffect(() => {
    try {
      localStorage.setItem(LS_SORT, sortBy);
    } catch {
      /* ignore */
    }
  }, [sortBy]);

  // 自动刷新：详情 / 终止确认 / README 向导打开时暂停，避免打断操作
  useEffect(() => {
    void refresh();
    if (timer.current) clearInterval(timer.current);
    if (!dialogsOpen) {
      timer.current = setInterval(() => void refresh(), REFRESH_MS);
    }
    return () => {
      if (timer.current) clearInterval(timer.current);
    };
  }, [refresh, dialogsOpen]);

  // 切换工作区后 README 预览 / 纳管预览失效
  useEffect(() => {
    closeReadmeImport();
    closeAdopt();
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

  // 当前工作区占用的端口 → 服务（用于行级关联标注）。
  const portOwner = useMemo(() => {
    const m = new Map<number, { id: string; kind: string }>();
    for (const [id, svc] of Object.entries(ws.state.spec?.services ?? {})) {
      if (svc.port != null) m.set(svc.port, { id, kind: svc.kind });
    }
    return m;
  }, [ws.state.spec?.services]);

  /**
   * 占位进程是否归属当前工作区的该服务（与后端 discover::classify_port_owner 同口径）：
   * cwd / 命令行命中工作区根 + 程序类型兼容；compose 另认 docker 系进程。
   */
  const isOwnedByWorkspace = useCallback(
    (s: ForeignService, expectedKind: string): boolean => {
      const root = ws.state.workspaceId ?? "";
      if (!root) return false;
      const norm = (v: string) => v.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
      const rootN = norm(root);
      const cwdHit = !!s.cwd && (norm(s.cwd) === rootN || norm(s.cwd).startsWith(`${rootN}/`));
      const cmdHit = !!s.cmd_line && rootN.length >= 4 && norm(s.cmd_line).includes(rootN);
      if (expectedKind === "compose") {
        const n = s.name.toLowerCase();
        if (n.includes("docker") || n.includes("vpnkit") || n.includes("containerd")) return true;
        return cwdHit || cmdHit;
      }
      if (!cwdHit && !cmdHit) return false;
      if (expectedKind === "spring-boot") return s.kind === "java";
      if (expectedKind === "node") return s.kind === "node" || s.kind === "bun" || s.kind === "deno";
      if (expectedKind === "python") return s.kind === "python";
      return true; // go / generic：位置命中即归属
    },
    [ws.state.workspaceId],
  );

  const matchesFor = useCallback(
    (s: ForeignService): PortMatch[] =>
      s.ports
        .map((p) => {
          const owner = portOwner.get(p);
          if (!owner) return null;
          return { p, id: owner.id, owned: isOwnedByWorkspace(s, owner.kind) };
        })
        .filter((x): x is PortMatch => x !== null),
    [portOwner, isOwnedByWorkspace],
  );

  const matchRank = useCallback(
    (s: ForeignService): number => {
      const m = matchesFor(s);
      if (m.some((x) => x.owned)) return 0;
      if (m.length > 0) return 1;
      return 2;
    },
    [matchesFor],
  );

  // 筛选：kind 多选 + 端口子串匹配；空集合 = 全部，空 query = 不过滤
  const filteredItems = useMemo(() => {
    const q = portQueryDebounced.trim();
    const list = items.filter((s) => {
      if (kindFilter.size > 0 && !kindFilter.has(s.kind)) return false;
      if (q && !s.ports.some((p) => String(p).includes(q))) return false;
      return true;
    });
    const sorted = [...list];
    sorted.sort((a, b) => {
      const ra = matchRank(a);
      const rb = matchRank(b);
      if (ra !== rb) return ra - rb;
      const cpu = (s: ForeignService) => s.cpu_percent ?? -1;
      const mem = (s: ForeignService) => s.memory_bytes ?? -1;
      if (sortBy === "cpu") {
        // CPU 首次采样为空时退回内存排序，保证点击后行序有可见变化
        if (cpu(b) !== cpu(a)) return cpu(b) - cpu(a);
        if (mem(b) !== mem(a)) return mem(b) - mem(a);
      } else if (sortBy === "memory") {
        if (mem(b) !== mem(a)) return mem(b) - mem(a);
        if (cpu(b) !== cpu(a)) return cpu(b) - cpu(a);
      }
      return a.name.localeCompare(b.name) || a.pid - b.pid;
    });
    return sorted;
  }, [items, kindFilter, portQueryDebounced, matchRank, sortBy]);

  const kindCounts = useMemo(() => {
    const m = new Map<string, number>();
    for (const s of items) m.set(s.kind, (m.get(s.kind) ?? 0) + 1);
    return m;
  }, [items]);

  const summary = useMemo(() => {
    let matched = 0;
    let conflict = 0;
    let wsPorts = 0;
    for (const s of items) {
      const m = matchesFor(s);
      if (m.some((x) => x.owned)) matched += 1;
      else if (m.length > 0) conflict += 1;
    }
    wsPorts = portOwner.size;
    return { total: items.length, matched, conflict, wsPorts };
  }, [items, matchesFor, portOwner]);

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
    setPortQueryDebounced("");
  };

  const known = filteredItems.filter((s) => s.kind !== "other");
  const others = filteredItems.filter((s) => s.kind === "other");

  // 弹框里的对象要跟随刷新（进程可能已退出 / 读数更新），按 pid 重新对位
  const detailLive = detail ? (items.find((s) => s.pid === detail.pid) ?? detail) : null;
  const detailMatched = detailLive ? matchesFor(detailLive) : [];

  const copy = async (text: string, label: string) => {
    const ok = await copyText(text);
    if (ok) toast(t("pages.discover.copied", { label }), "ok");
    else toast(t("pages.discover.copyFailed"), "err");
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
      let baseHash = yaml.state.hash;
      if (!baseHash) baseHash = (await apiYamlGet()).hash;
      await apiImportReadmeApply(wid, readmePreview.readme_path, choices, baseHash);
      toast(t("pages.discover.readmeApplied", { n: choices.length }), "ok");
      closeReadmeImport();
      await yaml.actions.reload();
      await ws.actions.refreshSpec();
    } catch (e) {
      toast(e instanceof IpcFailure ? formatIpcFailure(e) : String(e), "err");
    } finally {
      setReadmeApplying(false);
    }
  };

  const cycleSort = () => {
    setSortBy((prev) => (prev === "match" ? "cpu" : prev === "cpu" ? "memory" : "match"));
  };

  // 孤儿进程纳管：dry-run 预览（core 纯计算；不杀进程不落盘）
  const openAdopt = async () => {
    const wid = ws.state.workspaceId;
    if (!wid || adoptLoading) return;
    setAdoptLoading(true);
    try {
      const out = await apiAdoptPreview(wid);
      setAdoptPreview(out);
      const next: Record<number, boolean> = {};
      for (const it of out.items) {
        if (it.selected) next[it.pid] = true;
      }
      setAdoptChecked(next);
    } catch (e) {
      toast(e instanceof IpcFailure ? formatIpcFailure(e) : String(e), "err");
    } finally {
      setAdoptLoading(false);
    }
  };

  const closeAdopt = () => {
    setAdoptPreview(null);
    setAdoptChecked({});
  };

  const adoptApplyCount = useMemo(() => {
    if (!adoptPreview) return 0;
    return adoptPreview.items.filter(
      (it) => (it.status === "adoptable" || it.status === "id_conflict") && adoptChecked[it.pid],
    ).length;
  }, [adoptPreview, adoptChecked]);

  const applyAdoptChoices = async () => {
    const wid = ws.state.workspaceId;
    if (!wid || !adoptPreview || adoptApplying) return;
    const choices: AdoptChoice[] = adoptPreview.items
      .filter((it) => (it.status === "adoptable" || it.status === "id_conflict") && adoptChecked[it.pid])
      .map((it) => ({ pid: it.pid, action: "add" as const }));
    if (choices.length === 0) {
      toast(t("pages.config.selectChangesFirst"), "warn");
      return;
    }
    setAdoptApplying(true);
    try {
      let baseHash = yaml.state.hash;
      if (!baseHash) baseHash = (await apiYamlGet()).hash;
      await apiAdoptApply(wid, choices, baseHash);
      toast(t("pages.discover.adopt.applied", { n: choices.length }), "ok");
      closeAdopt();
      await yaml.actions.reload();
      await ws.actions.refreshSpec();
    } catch (e) {
      toast(e instanceof IpcFailure ? formatIpcFailure(e) : String(e), "err");
    } finally {
      setAdoptApplying(false);
    }
  };

  const renderRow = (s: ForeignService) => {
    const matched = matchesFor(s);
    const owned = matched.filter((m) => m.owned);
    const conflicted = matched.filter((m) => !m.owned);
    return (
      <tr
        key={`${s.pid}-${s.name}`}
        onClick={() => setDetail(s)}
        title={t("pages.discover.rowTitle")}
        className={cn(
          "cursor-pointer border-b border-[var(--line,#e6e6e6)] transition-colors duration-100 last:border-0",
          owned.length > 0
            ? "bg-[rgb(94_106_210_/_0.07)] shadow-[inset_3px_0_0_0_var(--st-accent,#5e6ad2)] hover:bg-[rgb(94_106_210_/_0.12)]"
            : conflicted.length > 0
              ? "bg-[rgb(220_38_38_/_0.04)] shadow-[inset_3px_0_0_0_#DC2626] hover:bg-[rgb(220_38_38_/_0.08)]"
              : "hover:bg-[var(--surface-2,#f3f4f5)]",
        )}
      >
        <td className="px-3 py-2.5">
          <span className="flex min-w-0 items-center gap-2">
            <span className="size-1.5 shrink-0 rounded-full" style={{ background: runtimeColor(s.kind) }} />
            <span title={s.name} className="min-w-0 truncate font-mono text-[0.78rem] font-medium text-[var(--t1,#222326)]">
              {s.name}
            </span>
          </span>
        </td>
        <td className="px-3 py-2.5 font-mono text-[0.78rem] whitespace-nowrap text-[var(--t2,#62666d)]">{s.pid}</td>
        <td className="px-3 py-2.5">
          <MetricCell value={s.cpu_percent} format={(v) => `${v.toFixed(1)}%`} placeholder="—" />
        </td>
        <td className="px-3 py-2.5">
          <MetricCell value={s.memory_bytes} format={formatBytes} placeholder="—" />
        </td>
        <td className="px-3 py-2.5">
          <span className="flex flex-wrap items-center gap-1">
            {s.ports.slice(0, 2).map((p) => {
              const hit = matched.find((m) => m.p === p);
              return (
                <span
                  key={p}
                  className={cn(
                    "inline-flex h-5 items-center rounded-full px-1.5 font-mono text-[0.68rem] leading-none",
                    hit?.owned
                      ? "bg-[rgb(94_106_210_/_0.14)] text-[var(--st-accent,#5e6ad2)] ring-1 ring-[rgb(94_106_210_/_0.35)]"
                      : hit
                        ? "bg-[var(--st-danger-tint,#fdecec)] text-[#DC2626] ring-1 ring-red-200"
                        : "bg-[var(--surface-2,#f3f4f5)] text-[var(--primary,#5E6AD2)]",
                  )}
                >
                  {p}
                </span>
              );
            })}
            {s.ports.length > 2 ? (
              <span
                title={s.ports.join(", ")}
                className="inline-flex h-5 cursor-help items-center rounded-full bg-[var(--surface-2,#f3f4f5)] px-1.5 font-mono text-[0.68rem] leading-none text-[var(--t3,#8a8f98)]"
              >
                +{s.ports.length - 2}
              </span>
            ) : null}
          </span>
        </td>
        <td className="px-3 py-2.5">
          {s.cwd ? (
            <span title={s.cmd_line ?? undefined} className="block truncate font-mono text-[0.72rem] text-[var(--t2,#62666d)]">
              {s.cwd}
            </span>
          ) : (
            <span className="text-[0.75rem] text-[var(--t3,#8a8f98)]">—</span>
          )}
        </td>
        <td className="px-3 py-2.5">
          {owned.length > 0 ? (
            <span className="flex min-w-0 items-center gap-1.5">
              <Badge variant="soon" className="shrink-0">{t("pages.discover.matchedBadge")}</Badge>
              <span className="min-w-0 truncate text-[0.75rem] text-[var(--st-accent-hover,#4f5ac8)]">
                {owned.map((m) => m.id).join(", ")}
              </span>
            </span>
          ) : conflicted.length > 0 ? (
            <span
              className="flex min-w-0 items-center gap-1.5"
              title={t("pages.discover.portConflictTitle", { id: conflicted.map((m) => m.id).join(", "), name: s.name })}
            >
              <Badge variant="outline" className="shrink-0 border-red-200 bg-[var(--st-danger-tint,#fdecec)] text-[#DC2626]">
                {t("pages.discover.portConflictBadge")}
              </Badge>
              <span className="min-w-0 truncate text-[0.75rem] text-[#DC2626]">{conflicted.map((m) => m.id).join(", ")}</span>
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
            {(() => {
              // cwd 落在当前工作区内且未被现有服务认领 → 可发起纳管
              const root = ws.state.workspaceId ?? "";
              const norm = (v: string) => v.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
              const inRoot =
                !!root &&
                !!s.cwd &&
                (norm(s.cwd) === norm(root) || norm(s.cwd).startsWith(`${norm(root)}/`));
              if (!inRoot || owned.length > 0) return null;
              return (
                <button
                  type="button"
                  title={t("pages.discover.adopt.rowTitle", { name: s.name, pid: s.pid })}
                  onClick={() => void openAdopt()}
                  className="grid size-6 cursor-pointer place-items-center rounded-[var(--r-sm,8px)] text-[var(--t3,#8a8f98)] transition-colors duration-150 hover:bg-[rgb(94_106_210_/_0.1)] hover:text-[var(--st-accent,#5e6ad2)] active:scale-95"
                >
                  <Magnet className="size-3" />
                </button>
              );
            })()}
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

  const sortLabel =
    sortBy === "cpu"
      ? t("pages.discover.sortCpu")
      : sortBy === "memory"
        ? t("pages.discover.sortMemory")
        : t("pages.discover.sortMatch");

  // 表头排序标记：当前排序落在哪一列一目了然
  const sortMark = (col: SortBy) =>
    sortBy === col ? <span className="ml-0.5 font-mono text-[var(--st-accent,#5e6ad2)]">↓</span> : null;

  const listBody = () => {
    if (loading && items.length === 0) {
      return (
        <div className="flex flex-col items-center gap-3 rounded-[var(--r-lg,16px)] border border-dashed border-[var(--line-strong,#d0d6e0)] p-10">
          <RefreshCw className="size-8 animate-spin text-[var(--st-accent,#5e6ad2)]" />
          <div className="text-[0.88rem] font-semibold text-[var(--t1,#222326)]">{t("pages.discover.loadingTitle")}</div>
          <div className="text-[0.78rem] text-[var(--t3,#8a8f98)]">{t("pages.discover.loadingDesc")}</div>
        </div>
      );
    }
    if (loadFailed && items.length === 0) {
      return (
        <Card className="flex flex-col items-start gap-2 p-6">
          <p className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]" role="alert">
            {t("pages.discover.loadFailedTitle")}
          </p>
          <p className="text-[0.78rem] text-[var(--t3,#8a8f98)]">{t("pages.discover.loadFailedDesc")}</p>
          <Button variant="soft" size="sm" className="mt-1 gap-1" onClick={() => void refresh()} disabled={loading}>
            <RefreshCw className={cn("size-3.5", loading && "animate-spin")} /> {t("pages.discover.retry")}
          </Button>
        </Card>
      );
    }
    if (filteredItems.length === 0 && items.length > 0) {
      return (
        <div className="flex flex-col items-center gap-3 rounded-[var(--r-lg,16px)] border border-dashed border-[var(--line-strong,#d0d6e0)] p-10">
          <Radar className="size-9 text-[var(--line-strong,#d0d6e0)]" />
          <div className="text-[0.88rem] font-semibold text-[var(--t1,#222326)]">{t("pages.discover.filterNoMatchTitle")}</div>
          <div className="text-[0.78rem] text-[var(--t3,#8a8f98)]">{t("pages.discover.filterNoMatchDesc")}</div>
          <Button variant="outline" size="sm" className="mt-1 gap-1" onClick={clearFilters}>
            <X className="size-3.5" /> {t("pages.discover.filterClear")}
          </Button>
        </div>
      );
    }
    if (items.length === 0) {
      return (
        <div className="flex flex-col items-center gap-3 rounded-[var(--r-lg,16px)] border border-dashed border-[var(--line-strong,#d0d6e0)] p-10">
          <Radar className="size-9 text-[var(--line-strong,#d0d6e0)]" />
          <div className="text-[0.88rem] font-semibold text-[var(--t1,#222326)]">{t("pages.discover.emptyTitle")}</div>
          <div className="text-[0.78rem] text-[var(--t3,#8a8f98)]">{t("pages.discover.emptyDesc")}</div>
          {ws.state.workspaceId ? (
            <Button variant="outline" size="sm" className="mt-1 gap-1" onClick={() => void openReadmeImport()} disabled={readmeLoading}>
              <FileInput className={cn("size-3.5", readmeLoading && "animate-pulse")} />
              {readmeLoading ? t("pages.discover.readmeImporting") : t("pages.discover.readmeImportEmptyCta")}
            </Button>
          ) : null}
        </div>
      );
    }
    return (
      <Card className="overflow-hidden p-0">
        <div className="max-h-[min(62vh,720px)] overflow-auto">
          <table className="w-full min-w-[860px] table-fixed border-collapse text-left">
            <thead className="sticky top-0 z-[1]">
              <tr className="border-b border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
                <th className="w-[15%] px-3 py-2.5 font-semibold">{t("pages.discover.colProcess")}</th>
                <th className="w-[68px] px-3 py-2.5 font-semibold">PID</th>
                <th className="w-[60px] px-3 py-2.5 font-semibold">CPU{sortMark("cpu")}</th>
                <th className="w-[88px] px-3 py-2.5 font-semibold">{t("pages.discover.colMemory")}{sortMark("memory")}</th>
                <th className="w-[136px] px-3 py-2.5 font-semibold">{t("pages.discover.colPorts")}</th>
                <th className="px-3 py-2.5 font-semibold">{t("pages.discover.colCwd")}</th>
                <th className="w-[18%] px-3 py-2.5 font-semibold">{t("pages.discover.colMatch")}{sortMark("match")}</th>
                <th className="w-[92px] px-2 py-2.5" />
              </tr>
            </thead>
            <tbody>
              {known.map(renderRow)}
              {others.length > 0 ? (
                <tr className="bg-[var(--surface-2,#f3f4f5)]">
                  <td colSpan={8} className="p-0">
                    <button
                      type="button"
                      onClick={() => setShowOther((v) => !v)}
                      aria-expanded={showOther}
                      title={showOther ? t("common.collapse") : t("common.expand")}
                      className="flex w-full cursor-pointer items-center justify-between gap-3 px-4 py-2 text-left transition-colors duration-150 hover:bg-[rgb(0_0_0_/_0.04)]"
                    >
                      <span className="inline-flex min-w-0 items-center gap-1.5 text-[0.78rem] font-medium text-[var(--t1,#222326)]">
                        <ChevronDown
                          className={cn("size-3.5 shrink-0 text-[var(--t3,#8a8f98)] transition-transform duration-150", !showOther && "-rotate-90")}
                        />
                        <span className="truncate">{t("pages.discover.others", { n: others.length })}</span>
                      </span>
                      <span className="shrink-0 whitespace-nowrap text-[0.72rem] text-[var(--t3,#8a8f98)]">
                        {showOther ? t("common.collapse") : t("common.expand")} · {t("pages.discover.othersHint")}
                      </span>
                    </button>
                  </td>
                </tr>
              ) : null}
              {showOther ? others.map(renderRow) : null}
            </tbody>
          </table>
        </div>
      </Card>
    );
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto">
        <div className="sticky top-0 z-10 border-b border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)]/95 px-6 py-3 backdrop-blur-sm">
          <div className="mx-auto flex max-w-7xl flex-col gap-2.5">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0">
                <h2 className="text-[1.05rem] font-bold tracking-tight text-[var(--t1,#222326)]">{t("pages.discover.title")}</h2>
                <p className="mt-0.5 text-[0.78rem] text-[var(--t3,#8a8f98)]">
                  {t("pages.discover.desc", { secs: REFRESH_MS / 1000 })}
                  {dialogsOpen ? <span className="ml-1 text-[var(--st-warn,#9a6700)]">· {t("pages.discover.refreshPaused")}</span> : null}
                </p>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                {lastRefresh ? (
                  <span className="font-mono text-[0.66rem] text-[var(--t3,#8a8f98)]">
                    {t("pages.discover.refreshedAt", { time: fmtTime(lastRefresh) })}
                  </span>
                ) : null}
                {ws.state.workspaceId ? (
                  <>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => void openAdopt()}
                      disabled={adoptLoading}
                      className="gap-1"
                      title={t("pages.discover.adopt.hint")}
                    >
                      <Magnet className={cn("size-3.5", adoptLoading && "animate-pulse")} />
                      {adoptLoading ? t("pages.discover.adopt.importing") : t("pages.discover.adopt.import")}
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => void openReadmeImport()}
                      disabled={readmeLoading}
                      className="gap-1"
                      title={t("pages.discover.readmeImportHint")}
                    >
                      <FileInput className={cn("size-3.5", readmeLoading && "animate-pulse")} />
                      {readmeLoading ? t("pages.discover.readmeImporting") : t("pages.discover.readmeImport")}
                    </Button>
                  </>
                ) : (
                  <>
                    <Button variant="outline" size="sm" disabled className="gap-1" title={t("pages.config.openWsFirst")}>
                      <Magnet className="size-3.5" />
                      {t("pages.discover.adopt.import")}
                    </Button>
                    <Button variant="outline" size="sm" disabled className="gap-1" title={t("pages.config.openWsFirst")}>
                      <FileInput className="size-3.5" />
                      {t("pages.discover.readmeImport")}
                    </Button>
                  </>
                )}
                <Button variant="soft" size="sm" onClick={() => void refresh()} disabled={loading} className="gap-1">
                  <RefreshCw className={cn(loading && "animate-spin")} /> {t("common.refresh")}
                </Button>
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-2">
              <SummaryChip label={t("pages.discover.chipTotal")} value={summary.total} />
              <SummaryChip
                label={t("pages.discover.chipMatched")}
                value={summary.matched}
                tone="accent"
                title={t("pages.discover.chipMatchedHint")}
              />
              <SummaryChip
                label={t("pages.discover.chipConflict")}
                value={summary.conflict}
                tone={summary.conflict > 0 ? "danger" : "default"}
                title={t("pages.discover.chipConflictHint")}
              />
              <SummaryChip
                label={t("pages.discover.chipWsPorts")}
                value={summary.wsPorts}
                tone={ws.state.workspaceId ? "accent" : "default"}
                title={t("pages.discover.chipWsPortsHint")}
              />
            </div>

            <div className="flex flex-wrap items-center gap-2">
              <span role="group" aria-label={t("pages.discover.filterByKindAria")} className="flex flex-wrap items-center gap-1">
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
              <div className="relative">
                <Input
                  value={portQuery}
                  onChange={(e) => setPortQuery(e.target.value)}
                  placeholder={t("pages.discover.filterPortPlaceholder")}
                  aria-label={t("pages.discover.filterPortAria")}
                  inputMode="numeric"
                  className="h-7 w-44 rounded-full pr-7 text-[0.75rem] font-mono"
                />
                {portQuery ? (
                  <button
                    type="button"
                    title={t("pages.discover.clearPort")}
                    aria-label={t("pages.discover.clearPort")}
                    onClick={() => {
                      setPortQuery("");
                      setPortQueryDebounced("");
                    }}
                    className="absolute top-1/2 right-1.5 grid size-5 -translate-y-1/2 place-items-center rounded-full text-[var(--t3,#8a8f98)] hover:bg-[rgb(0_0_0_/_0.06)] hover:text-[var(--t1,#222326)]"
                  >
                    <X className="size-3" />
                  </button>
                ) : null}
              </div>
              <Button
                variant={sortBy === "match" ? "ghost" : "soft"}
                size="xs"
                onClick={cycleSort}
                className={cn("gap-1", sortBy !== "match" && "text-[var(--st-accent,#5e6ad2)]")}
                title={t("pages.discover.sortHint")}
              >
                {sortBy === "cpu" ? <Cpu className="size-3" /> : sortBy === "memory" ? <HardDrive className="size-3" /> : <ArrowDownWideNarrow className="size-3" />}
                {sortLabel}
              </Button>
              {hasFilter ? (
                <Button variant="ghost" size="xs" onClick={clearFilters} className="gap-1">
                  <X className="size-3" /> {t("pages.discover.filterClear")}
                </Button>
              ) : null}
            </div>
          </div>
        </div>

        <div className="mx-auto flex max-w-7xl flex-col gap-4 px-6 py-4">
          {listBody()}
          <p className="text-[0.72rem] leading-relaxed text-[var(--t3,#8a8f98)]">{t("pages.discover.footnote")}</p>
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
                <DialogTitle className="flex flex-wrap items-center gap-2">
                  <span className="size-2 rounded-full" style={{ background: runtimeColor(detailLive.kind) }} />
                  <span className="font-mono">{detailLive.name}</span>
                  <Badge variant="outline" className="font-mono text-[0.7rem]">
                    PID {detailLive.pid}
                  </Badge>
                  <Badge variant="secondary" className="font-mono text-[0.7rem]">
                    {detailLive.kind}
                  </Badge>
                </DialogTitle>
                <DialogDescription>{t("pages.discover.detailDesc", { secs: REFRESH_MS / 1000 })}</DialogDescription>
              </DialogHeader>

              <div className="grid gap-3 sm:grid-cols-2">
                <div className="rounded-[var(--r-md,12px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] p-3">
                  <div className="mb-1 text-[0.68rem] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">CPU</div>
                  <div className="font-mono text-[1.1rem] font-semibold text-[var(--t1,#222326)]">
                    <MetricCell value={detailLive.cpu_percent} format={(v) => `${v.toFixed(1)}%`} placeholder={t("pages.discover.cpuSampling")} />
                  </div>
                </div>
                <div className="rounded-[var(--r-md,12px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] p-3">
                  <div className="mb-1 text-[0.68rem] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">{t("pages.discover.fMemory")}</div>
                  <div className="font-mono text-[1.1rem] font-semibold text-[var(--t1,#222326)]">
                    <MetricCell value={detailLive.memory_bytes} format={formatBytes} placeholder={t("pages.discover.memFailed")} />
                  </div>
                </div>
              </div>

              <div className="flex flex-col gap-2.5">
                <DetailField label={t("pages.discover.colPorts")}>
                  <span className="flex flex-wrap items-center gap-1">
                    {detailLive.ports.map((p) => {
                      const hit = detailMatched.find((m) => m.p === p);
                      return (
                        <span
                          key={p}
                          className={cn(
                            "inline-flex h-5 items-center rounded-full px-1.5 font-mono text-[0.7rem] leading-none",
                            hit?.owned
                              ? "bg-[rgb(94_106_210_/_0.14)] text-[var(--st-accent,#5e6ad2)] ring-1 ring-[rgb(94_106_210_/_0.35)]"
                              : hit
                                ? "bg-[var(--st-danger-tint,#fdecec)] text-[#DC2626] ring-1 ring-red-200"
                                : "bg-[var(--surface-2,#f3f4f5)] text-[var(--primary,#5E6AD2)]",
                          )}
                        >
                          {p}
                        </span>
                      );
                    })}
                    {detailLive.ports.length > 0 ? (
                      <button
                        type="button"
                        title={t("pages.discover.copyPorts")}
                        onClick={() => void copy(detailLive.ports.join(", "), t("pages.discover.labelPorts"))}
                        className="ml-1 shrink-0 cursor-pointer text-[var(--t3,#8a8f98)] transition-colors duration-150 hover:text-[var(--t1,#222326)] active:scale-95"
                      >
                        <Copy className="size-3" />
                      </button>
                    ) : null}
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
                      {detailMatched.every((m) => m.owned) ? (
                        <Badge variant="soon" className="shrink-0">{t("pages.discover.matchedCurrent")}</Badge>
                      ) : (
                        <Badge
                          variant="outline"
                          className="shrink-0 border-red-200 bg-[var(--st-danger-tint,#fdecec)] text-[#DC2626]"
                          title={t("pages.discover.portConflictTitle", {
                            id: detailMatched.filter((m) => !m.owned).map((m) => m.id).join("、"),
                            name: detailLive.name,
                          })}
                        >
                          {t("pages.discover.portConflictBadge")}
                        </Badge>
                      )}
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

              <DialogFooter className="flex-wrap gap-2">
                <Button variant="outline" size="sm" onClick={() => void copy(String(detailLive.pid), ` PID ${detailLive.pid}`)}>
                  {t("pages.discover.copyPid")}
                </Button>
                {detailLive.ports.length > 0 ? (
                  <Button variant="outline" size="sm" onClick={() => void copy(detailLive.ports.join(", "), t("pages.discover.labelPorts"))}>
                    {t("pages.discover.copyPorts")}
                  </Button>
                ) : null}
                {detailLive.cwd ? (
                  <Button variant="outline" size="sm" onClick={() => void copy(detailLive.cwd!, t("pages.discover.labelPath"))}>
                    {t("pages.discover.copyPath")}
                  </Button>
                ) : null}
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

      <Dialog open={readmePreview != null} onOpenChange={(o) => !o && closeReadmeImport()}>
        <DialogContent className="flex max-h-[86vh] flex-col gap-0 overflow-hidden p-0 sm:max-w-3xl" showCloseButton={false}>
          {readmePreview ? (
            readmePreview.items.length === 0 && readmePreview.script_items.length === 0 ? (
              <div className="flex flex-col items-center gap-2 p-8">
                <FileInput className="size-8 text-[var(--line-strong,#d0d6e0)]" />
                <DialogHeader className="items-center text-center">
                  <DialogTitle>{t("pages.discover.readmeEmptyDraftTitle")}</DialogTitle>
                  <DialogDescription asChild>
                    <div className="space-y-1">
                      {readmePreview.warnings.map((w, i) => (
                        <div key={i}>{w}</div>
                      ))}
                    </div>
                  </DialogDescription>
                </DialogHeader>
                <Button variant="outline" size="sm" className="mt-2" onClick={closeReadmeImport}>
                  {t("common.close")}
                </Button>
              </div>
            ) : (
              <div className="flex min-h-0 flex-1 flex-col p-3">
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
          ) : null}
        </DialogContent>
      </Dialog>

      <Dialog open={adoptPreview != null} onOpenChange={(o) => !o && closeAdopt()}>
        <DialogContent className="flex max-h-[86vh] flex-col gap-0 overflow-hidden p-0 sm:max-w-3xl" showCloseButton={false}>
          {adoptPreview ? (
            adoptPreview.items.length === 0 ? (
              <div className="flex flex-col items-center gap-2 p-8">
                <Magnet className="size-8 text-[var(--line-strong,#d0d6e0)]" />
                <DialogHeader className="items-center text-center">
                  <DialogTitle>{t("pages.discover.adopt.emptyTitle")}</DialogTitle>
                  <DialogDescription asChild>
                    <div className="space-y-1">
                      {adoptPreview.warnings.map((w, i) => (
                        <div key={i}>{w}</div>
                      ))}
                    </div>
                  </DialogDescription>
                </DialogHeader>
                <Button variant="outline" size="sm" className="mt-2" onClick={closeAdopt}>
                  {t("common.close")}
                </Button>
              </div>
            ) : (
              <div className="flex min-h-0 flex-1 flex-col p-3">
                <AdoptPanel
                  preview={adoptPreview}
                  checked={adoptChecked}
                  onToggle={(pid, v) => setAdoptChecked((m) => ({ ...m, [pid]: v }))}
                  onSelectAll={(v) => {
                    const next: Record<number, boolean> = {};
                    for (const it of adoptPreview.items) {
                      if (it.status === "adoptable" || it.status === "id_conflict") next[it.pid] = v;
                    }
                    setAdoptChecked(next);
                  }}
                  applying={adoptApplying}
                  applyCount={adoptApplyCount}
                  onApply={() => void applyAdoptChoices()}
                  onClose={closeAdopt}
                />
              </div>
            )
          ) : null}
        </DialogContent>
      </Dialog>
    </div>
  );
}
