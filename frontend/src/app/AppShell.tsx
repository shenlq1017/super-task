import { useEffect, useState } from "react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router-dom";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { cn } from "@/lib/utils";
import { groupedFeatures, navLabel, GROUP_TITLE, type NavGroup } from "../features/registry";
import { useFeatures, useSession } from "../providers/session-provider";
import { useWorkspace } from "../providers/workspace-provider";
import { useRuntime } from "../providers/runtime-provider";
import { useToast } from "@/components/ui/toast";
import { CommandPalette } from "@/components/command-palette";
import { WorkspaceSwitcher } from "@/components/workspace-switcher";
import { useOpenWorkspace } from "../lib/use-open-workspace";
import { isTauri } from "../ipc/invoke";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Search,
  Play,
  Square,
  ScanLine,
  FolderOpen,
  Folders,
  Radar,
  PanelLeftClose,
  PanelLeftOpen,
  Settings as SettingsIcon,
  Settings2,
  Command,
  Wrench,
  FileText,
  LayoutTemplate,
  GitBranch,
  Container,
  Network,
  Cloud,
  Sparkles,
} from "lucide-react";

export type ShellCtx = { compact: boolean; defaultFollow: boolean };

function WatchingPulse({ active }: { active: boolean }) {
  return (
    <span className="inline-flex items-center gap-1.5 text-[var(--st-ok-deep,#1e7e35)]">
      <span className="flex h-3 items-end gap-[2px]">
        {[0, 1, 2, 3].map((i) => (
          <span
            key={i}
            className={cn("w-px rounded-sm bg-[var(--st-ok,#27a644)]", active ? "animate-pulse" : "opacity-40")}
            style={{ height: active ? (i % 2 ? "12px" : "8px") : "6px", animationDelay: `${i * 120}ms` }}
          />
        ))}
      </span>
      {active ? "watching" : "idle"}
    </span>
  );
}

function ProbeChip({ name, found, version }: { name: string; found: boolean; version: string | null }) {
  return (
    <span className="inline-flex items-center gap-1 whitespace-nowrap">
      <span
        className="size-1.5 shrink-0 rounded-full"
        style={{ background: found ? "var(--st-ok,#27a644)" : "var(--st-warn-dot,#eab308)" }}
      />
      <span className="font-semibold text-[var(--t1,#222326)]">{name}</span>
      {found ? (
        <span className="text-[var(--t3,#8a8f98)]">{version ?? "✓"}</span>
      ) : null}
    </span>
  );
}

function SidebarItem({
  to,
  label,
  icon,
  soon,
  version,
  collapsed,
  onAction,
}: {
  to?: string;
  label: string;
  icon: React.ReactNode;
  soon?: boolean;
  version?: string;
  collapsed: boolean;
  onAction?: () => void;
}) {
  // 对齐原型：左侧 3px 紫条作为 active 指示；hover 滑出，active 常驻
  const renderInner = (isActive: boolean) => (
    <>
      <span
        aria-hidden
        className={cn(
          "pointer-events-none absolute left-0 top-1.5 bottom-1.5 w-[3px] rounded-full bg-[var(--st-accent,#5e6ad2)] transition-transform duration-200 ease-[cubic-bezier(.22,1,.36,1)]",
          isActive ? "scale-y-100" : "scale-y-0 group/sb:scale-y-100",
        )}
      />
      <span
        className={cn(
          "flex size-4 shrink-0 items-center justify-center transition-colors",
          isActive && "text-[var(--st-accent,#5e6ad2)]",
        )}
      >
        {icon}
      </span>
      <span className={cn("truncate", collapsed && "hidden")}>{label}</span>
      {soon && version ? (
        <Badge variant="soon" className={cn("ml-auto", collapsed && "hidden")}>
          {version}
        </Badge>
      ) : null}
    </>
  );
  const baseCls =
    "group/sb relative flex items-center gap-2.5 rounded-[var(--r-sm,8px)] px-2.5 py-1.5 text-[0.83rem] font-medium no-underline transition-colors duration-150 text-[var(--t2,#62666d)] hover:bg-[rgb(0_0_0_/_0.045)] hover:text-[var(--t1,#222326)]";
  // 原型：active = 白色 + 1px 阴影 + 1px 内描边（inset），无外环
  const activeCls =
    "bg-[var(--surface,#fff)] text-[var(--t1,#222326)] shadow-[0_1px_2px_rgb(16_24_40_/_0.05),inset_0_0_0_1px_var(--line,#e6e6e6)]";
  const soonCls = "cursor-not-allowed opacity-60 hover:bg-transparent hover:text-[var(--t3,#8a8f98)]";
  if (soon || !to) {
    return (
      <div
        title={soon ? `${label}（尚未提供）` : label}
        onClick={onAction}
        className={cn(baseCls, soon && soonCls)}
      >
        {renderInner(false)}
      </div>
    );
  }
  return (
    <NavLink
      to={to}
      title={label}
      className={({ isActive }) => cn(baseCls, isActive && activeCls)}
    >
      {({ isActive }) => renderInner(isActive)}
    </NavLink>
  );
}

export function AppShell() {
  const features = useFeatures();
  const { state } = useSession();
  const ws = useWorkspace();
  const runtime = useRuntime();
  const { toast } = useToast();
  const navigate = useNavigate();
  const location = useLocation();
  const openWs = useOpenWorkspace();

  const [collapsed, setCollapsed] = useState(false);
  const [cmdOpen, setCmdOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [follow, setFollow] = useState(true);
  const [compact, setCompact] = useState(false);
  const [confirmStopAll, setConfirmStopAll] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setCmdOpen((v) => !v);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const pickDirectory = async () => {
    if (!isTauri()) {
      const p = window.prompt("输入工作区目录路径");
      if (p) await openWs(p);
      return;
    }
    const selected = await openDialog({ directory: true, multiple: false });
    if (typeof selected === "string") await openWs(selected);
  };

  const onDir = async () => {
    if (ws.state.workspaceId) {
      ws.actions.openExplorer();
      return;
    }
    await pickDirectory();
  };

  const onScan = async () => {
    if (!ws.state.workspaceId) {
      await pickDirectory();
      return;
    }
    try {
      await ws.actions.scanDraft(ws.state.workspaceId);
      await ws.actions.refreshSpec();
      toast("已重新扫描工作区", "ok");
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), "err");
    }
  };

  const probe = state.app?.probe;
  const probeItems = probe
    ? [
        { name: "JDK", ok: probe.java.found, v: probe.java.version },
        { name: "Maven", ok: probe.maven.found, v: probe.maven.version },
        { name: "Node", ok: probe.node.found, v: probe.node.version },
        { name: "pnpm", ok: probe.pnpm.found, v: probe.pnpm.version },
      ]
    : [];

  const groups = groupedFeatures(features);
  const cur = features.find((f) => f.path === location.pathname);
  const section = cur ? navLabel(cur.id) : "欢迎";
  const wsName =
    ws.state.workspaceId?.split(/[\\/]/).filter(Boolean).pop() ?? "未打开工作区";

  const shellCtx: ShellCtx = { compact, defaultFollow: follow };

  return (
    <div className="flex h-full flex-col overflow-hidden bg-[var(--bg,#f7f8f8)]">
      {/* ============ shell：侧栏直达顶（与原型一致），pagehead 在 main 列内 ============ */}
      <div className="flex min-h-0 flex-1">
        {/* ============ 侧栏 ============ */}
        <aside
          className={cn(
            "flex shrink-0 flex-col border-r border-[var(--line,#e6e6e6)] bg-[var(--bg,#f7f8f8)] p-2 transition-all",
            collapsed ? "w-[3.25rem]" : "w-[14.5rem]",
          )}
        >
          {/* 工作区切换（品牌名由窗口标题栏承载，不再重复） */}
          <WorkspaceSwitcher collapsed={collapsed} />

          {/* ⌘K 触发 */}
          <button
            onClick={() => setCmdOpen(true)}
            className="mt-1.5 flex items-center gap-2 rounded-full border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] px-3 py-1.5 text-[var(--t3,#8a8f98)] shadow-sm transition-all hover:border-[var(--line-strong,#d0d6e0)] hover:text-[var(--t2,#62666d)]"
          >
            <Command className="size-3.5" />
            <span className={cn("text-[0.74rem]", collapsed && "hidden")}>搜索 / 命令</span>
            <kbd className={cn("ml-auto font-mono text-[10px] text-[var(--t3,#8a8f98)]", collapsed && "hidden")}>⌘K</kbd>
          </button>

          {/* 导航分组 */}
          <nav className="mt-2 flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto">
            {groups.map((g) => (
              <div key={g.group} className="flex flex-col gap-0.5">
                <div
                  className={cn(
                    "px-2.5 pb-1 pt-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]",
                    collapsed && "hidden",
                  )}
                >
                  {GROUP_TITLE[g.group as NavGroup]}
                </div>
                {g.items.map((f) => (
                  <SidebarItem
                    key={f.id}
                    to={f.path}
                    label={navLabel(f.id)}
                    icon={NAV_ICONS[f.id] ?? <Wrench className="size-4" />}
                    soon={f.status === "soon"}
                    version={f.since}
                    collapsed={collapsed}
                  />
                ))}
              </div>
            ))}
          </nav>

          {/* 底部：设置 + 收起 */}
          <div className="mt-1 flex flex-col gap-0.5 border-t border-[var(--line,#e6e6e6)] pt-2">
            <div className="relative">
              <button
                onClick={() => setSettingsOpen((v) => !v)}
                className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-sm text-[var(--t2,#62666d)] transition-colors hover:bg-black/5 hover:text-[var(--t1,#222326)]"
              >
                <SettingsIcon className="size-4 shrink-0" />
                <span className={cn("truncate", collapsed && "hidden")}>设置</span>
              </button>
              {settingsOpen ? (
                <SettingsPopover
                  follow={follow}
                  health
                  compact={compact}
                  onFollow={setFollow}
                  onHealth={() => {}}
                  onCompact={setCompact}
                  onOpenSettings={() => {
                    setSettingsOpen(false);
                    navigate("/settings");
                  }}
                  collapsed={collapsed}
                />
              ) : null}
            </div>
            <button
              onClick={() => setCollapsed((v) => !v)}
              className="flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-sm text-[var(--t3,#8a8f98)] transition-colors hover:bg-black/5 hover:text-[var(--t2,#62666d)]"
              title={collapsed ? "展开侧栏" : "收起侧栏"}
            >
              {collapsed ? <PanelLeftOpen className="size-4 shrink-0" /> : <PanelLeftClose className="size-4 shrink-0" />}
              <span className={cn("truncate", collapsed && "hidden")}>收起侧栏</span>
            </button>
          </div>
        </aside>

        {/* ============ 主区：pagehead 收进 main 列（侧栏因此直达顶） ============ */}
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          <header className="flex h-[50px] shrink-0 items-center gap-3 border-b border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] px-3.5">
            <div className="flex items-center gap-1.5 text-sm">
              <span className="font-semibold text-[var(--t1,#222326)]">{section}</span>
              <span className="text-[var(--line-strong,#d0d6e0)]">/</span>
              <span className="truncate font-mono text-[11px] text-[var(--t2,#62666d)]" title={ws.state.workspaceId ?? ""}>
                {wsName}
              </span>
            </div>

            <button
              onClick={() => setCmdOpen(true)}
              className="mx-auto flex h-7 max-w-[28rem] flex-1 items-center gap-2 rounded-lg border border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] px-3 text-[var(--t3,#8a8f98)] transition-colors hover:border-[var(--line-strong,#d0d6e0)] hover:bg-[var(--surface,#fff)] hover:text-[var(--t2,#62666d)]"
              title="搜索 / 命令面板 (⌘K)"
            >
              <Search className="size-3.5" />
              <span className="truncate text-[0.8rem]">搜索服务、命令 · 跳转到配置…</span>
              <kbd className="ml-auto rounded border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] px-1.5 py-0.5 font-mono text-[11px] text-[var(--t3,#8a8f98)]">
                ⌘K
              </kbd>
            </button>

            <div className="flex shrink-0 items-center gap-1.5">
              <Button variant="ghost" size="sm" className="gap-1 text-[var(--t2,#62666d)]" onClick={onScan} title="扫描工作区（⌘R）">
                <ScanLine /> <span className={cn(collapsed && "hidden")}>扫描</span>
              </Button>
              <Button variant="outline" size="sm" className="gap-1" onClick={onDir} title="打开工作区目录">
                <FolderOpen /> <span className={cn(collapsed && "hidden")}>目录</span>
              </Button>
              <Button
                variant="outline"
                size="sm"
                className="gap-1 hover:border-[#DC2626] hover:bg-[#FDECEC] hover:text-[#DC2626]"
                onClick={() => {
                  const n = Object.values(runtime.state.services).filter((s) => s.state === "running").length;
                  if (n === 0) {
                    toast("没有运行中的服务", "info");
                    return;
                  }
                  setConfirmStopAll(true);
                }}
                disabled={!ws.state.workspaceId}
                title="停止全部（⌘.）"
              >
                <Square /> <span className={cn(collapsed && "hidden")}>停止</span>
              </Button>
              <Button
                variant="default"
                size="sm"
                className="gap-1"
                onClick={() => runtime.actions.startAll().then(() => toast("已发起启动全部", "ok"))}
                disabled={!ws.state.workspaceId}
                title="启动全部（⌘⇧R）"
              >
                <Play /> <span className={cn(collapsed && "hidden")}>启动全部</span>
              </Button>
            </div>
          </header>
          <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
            {ws.state.error ? (
              <div className="m-3 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">{ws.state.error}</div>
            ) : (
              <Outlet context={shellCtx} />
            )}
          </main>
        </div>
      </div>

      {/* ============ 状态栏：单行 heartbeat，快捷键提示移除（按钮 tooltip 已覆盖） ============ */}
      <footer className="flex h-[30px] shrink-0 items-center gap-2 overflow-hidden whitespace-nowrap border-t border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] px-3 font-mono text-[11px] text-[var(--t3,#8a8f98)]">
        <WatchingPulse active={!!ws.state.workspaceId} />
        <span className="text-[var(--line-strong,#d0d6e0)]">·</span>
        <span className="flex min-w-0 items-center gap-3 overflow-hidden">
          {probeItems.map((p) => (
            <ProbeChip key={p.name} name={p.name} found={p.ok} version={p.v} />
          ))}
        </span>
        <span className="ml-auto flex shrink-0 items-center gap-2 pl-3">
          <span className="flex items-center gap-1.5 text-[var(--st-ok-deep,#1e7e35)]">
            <span className="size-1.5 rounded-full bg-[var(--st-ok,#27a644)]" />
            {ws.state.workspaceId ? "yaml 已同步" : "未打开工作区"}
          </span>
          <span className="text-[var(--line-strong,#d0d6e0)]">·</span>
          <span>v{state.hello?.product_version ?? "1.0"}</span>
        </span>
      </footer>

      <CommandPalette open={cmdOpen} onClose={() => setCmdOpen(false)} features={features} />

      <ConfirmDialog
        open={confirmStopAll}
        title="停止全部服务？"
        description={`将停止 ${Object.values(runtime.state.services).filter((s) => s.state === "running").length} 个运行中的服务（各服务的整棵进程树）。`}
        confirmText="全部停止"
        cancelText="取消"
        destructive
        onConfirm={() => {
          setConfirmStopAll(false);
          runtime.actions.stopAll().then(() => toast("已停止全部", "ok"));
        }}
        onCancel={() => setConfirmStopAll(false)}
      />
    </div>
  );
}

const NAV_ICONS: Record<string, React.ReactNode> = {
  run: <Play className="size-4" />,
  logs: <FileText className="size-4" />,
  config: <Settings2 className="size-4" />,
  env: <Wrench className="size-4" />,
  workspaces: <Folders className="size-4" />,
  discover: <Radar className="size-4" />,
  templates: <LayoutTemplate className="size-4" />,
  git: <GitBranch className="size-4" />,
  docker: <Container className="size-4" />,
  gateway: <Network className="size-4" />,
  cloud: <Cloud className="size-4" />,
  ai: <Sparkles className="size-4" />,
};

function SettingsPopover({
  follow,
  health,
  compact,
    onFollow,
    onHealth,
    onCompact,
    onOpenSettings,
    collapsed,
  }: {
  follow: boolean;
  health: boolean;
  compact: boolean;
  onFollow: (v: boolean) => void;
  onHealth: (v: boolean) => void;
  onCompact: (v: boolean) => void;
  onOpenSettings: () => void;
  collapsed: boolean;
}) {
  const Row = ({
    label,
    on,
    onToggle,
    disabled,
  }: {
    label: string;
    on: boolean;
    onToggle: (v: boolean) => void;
    disabled?: boolean;
  }) => (
    <div className="flex items-center justify-between gap-2 rounded-lg px-2 py-1.5 text-[0.76rem] text-[var(--t1,#222326)]">
      <span>{label}</span>
      <button
        disabled={disabled}
        onClick={() => onToggle(!on)}
        className={cn(
          "relative h-[18px] w-[34px] rounded-full border transition-colors",
          on ? "border-[var(--st-accent,#5e6ad2)] bg-[var(--st-accent,#5e6ad2)]" : "border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)]",
          disabled && "opacity-50",
        )}
      >
        <span
          className="absolute top-1/2 size-3 rounded-full bg-white shadow-sm transition-all"
          style={{ left: on ? "17px" : "3px", transform: "translateY(-50%)" }}
        />
      </button>
    </div>
  );

  return (
    <div
      className={cn(
        "absolute bottom-[2.6rem] left-1 z-[110] w-56 rounded-xl border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] p-1.5 shadow-2xl",
        collapsed && "left-1",
      )}
      onClick={(e) => e.stopPropagation()}
    >
      <div className="px-2 py-1 text-[10px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
        设置
      </div>
      <Row label="跟随底部日志" on={follow} onToggle={onFollow} />
      <Row label="实时健康检查" on={health} onToggle={onHealth} disabled />
      <Row label="紧凑密度" on={compact} onToggle={onCompact} />
      <button
        onClick={onOpenSettings}
        className="mt-1 flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-[0.76rem] text-[var(--t2,#62666d)] hover:bg-black/5"
      >
        <SettingsIcon className="size-3.5" /> 打开设置页
      </button>
    </div>
  );
}
