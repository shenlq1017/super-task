import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router-dom";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { formatIpcFailure } from "@/lib/error-messages";
import { groupedFeatures, navTranslationKey, type NavGroup } from "../features/registry";
import { useFeatures, useSession } from "../providers/session-provider";
import { useWorkspace } from "../providers/workspace-provider";
import { useToast } from "@/components/ui/toast";
import { CommandPalette } from "@/components/command-palette";
import { WorkspaceSwitcher } from "@/components/workspace-switcher";
import { StatusBar, type EnvItem } from "@/components/status-bar";
import { useOpenWorkspace } from "../lib/use-open-workspace";
import { readCompactPref, readLogFollowPref, writeCompactPref, writeLogFollowPref } from "@/lib/workspace-storage";
import { isTauri } from "../ipc/invoke";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Search,
  Play,
  ScanLine,
  FolderOpen,
  Folders,
  Radar,
  PanelLeftClose,
  PanelLeftOpen,
  Settings as SettingsIcon,
  SlidersHorizontal,
  ScrollText,
  KeyRound,
  Globe,
  LayoutTemplate,
  GitBranch,
  Container,
  Cloud,
  Sparkles,
} from "lucide-react";

export type ShellCtx = {
  compact: boolean;
  defaultFollow: boolean;
  setCompact: (v: boolean) => void;
  setDefaultFollow: (v: boolean) => void;
};

function WatchingPulse({ active }: { active: boolean }) {
  return (
    <span className="inline-flex items-center gap-1.5 text-[var(--st-ok-deep)]">
      <span className="flex h-3 items-end gap-[2px]">
        {[0, 1, 2, 3].map((i) => (
          <span
            key={i}
            className={cn("w-px rounded-sm bg-[var(--st-ok)]", active ? "animate-pulse" : "opacity-40")}
            style={{ height: active ? (i % 2 ? "12px" : "8px") : "6px", animationDelay: `${i * 120}ms` }}
          />
        ))}
      </span>
      {active ? "watching" : "idle"}
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
  const { t } = useTranslation();
  // Active: surface card + thin accent rail. Hover rail is neutral so purple stays for selection only.
  const renderInner = (isActive: boolean) => (
    <>
      <span
        aria-hidden
        className={cn(
          "pointer-events-none absolute left-0 top-1.5 bottom-1.5 w-[3px] rounded-full transition-transform duration-200 ease-[var(--st-ease)]",
          isActive ? "scale-y-100 bg-[var(--st-accent)]" : "scale-y-0 bg-[var(--line-strong)] group-hover/sb:scale-y-100",
        )}
      />
      <span
        className={cn(
          "flex size-4 shrink-0 items-center justify-center transition-colors",
          isActive ? "text-[var(--st-accent)]" : "text-[var(--t3)] group-hover/sb:text-[var(--t2)]",
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
    "group/sb relative flex items-center gap-2.5 rounded-[var(--r-sm)] px-2.5 py-[0.4rem] text-[0.84rem] font-medium no-underline transition-colors duration-150 text-[var(--t2)] hover:bg-[var(--surface-2)] hover:text-[var(--t1)]";
  const activeCls =
    "bg-[var(--surface)] text-[var(--t1)] shadow-[var(--shadow-1),inset_0_0_0_1px_var(--line)]";
  const soonCls = "cursor-not-allowed opacity-50 hover:bg-transparent hover:text-[var(--t3)]";
  if (soon || !to) {
    return (
      <div
        title={soon ? t("common.soonHint", { label }) : label}
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
  const { toast } = useToast();
  const { t } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const openWs = useOpenWorkspace();

  /** feature id → 本地化导航文案（labelKey → `nav.*`）。 */
  const navText = (id: string): string => {
    const key = navTranslationKey(id);
    return key ? t(key) : id;
  };

  const [collapsed, setCollapsed] = useState(false);
  const [cmdOpen, setCmdOpen] = useState(false);
  const [follow, setFollow] = useState(() => readLogFollowPref());
  const [compact, setCompact] = useState(() => readCompactPref());

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
      const p = window.prompt(t("common.inputWorkspacePath"));
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
      toast(t("common.rescanned"), "ok");
    } catch (e) {
      toast(formatIpcFailure(e), "err");
    }
  };

  const probe = state.app?.probe;
  const probeItems: EnvItem[] = probe
    ? [
        { name: "JDK", ok: probe.java.found, v: probe.java.version },
        { name: "Maven", ok: probe.maven.found, v: probe.maven.version },
        { name: "Node", ok: probe.node.found, v: probe.node.version },
        { name: "pnpm", ok: probe.pnpm.found, v: probe.pnpm.version },
      ]
    : [];

  const groups = groupedFeatures(features);
  const cur = features.find((f) => f.path === location.pathname);
  const section = cur ? navText(cur.id) : t("nav.welcome");
  const wsName =
    ws.state.workspaceId?.split(/[\\/]/).filter(Boolean).pop() ?? t("common.noWorkspace");

  const shellCtx: ShellCtx = {
    compact,
    defaultFollow: follow,
    setCompact: (v) => {
      setCompact(v);
      writeCompactPref(v);
    },
    setDefaultFollow: (v) => {
      setFollow(v);
      writeLogFollowPref(v);
    },
  };

  return (
    <div className="flex h-full flex-col overflow-hidden bg-[var(--bg)]">
      <div className="flex min-h-0 flex-1">
        {/* Sidebar: workspace + nav only — command search lives in the header */}
        <aside
          className={cn(
            "flex shrink-0 flex-col border-r border-[var(--line)] bg-[var(--bg)] p-2 transition-all",
            collapsed ? "w-[3.25rem]" : "w-[14.5rem]",
          )}
        >
          <WorkspaceSwitcher collapsed={collapsed} />

          <nav className="mt-2 flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto">
            {groups.map((g) => (
              <div key={g.group} className="flex flex-col gap-0.5">
                <div
                  className={cn(
                    "px-2.5 pb-1 pt-2.5 text-[10px] font-semibold uppercase tracking-[0.08em] text-[var(--t3)]",
                    collapsed && "hidden",
                  )}
                >
                  {t(`groups.${g.group as NavGroup}`)}
                </div>
                {g.items.map((f) => (
                  <SidebarItem
                    key={f.id}
                    to={f.path}
                    label={navText(f.id)}
                    icon={NAV_ICONS[f.id] ?? <SlidersHorizontal className="size-4" />}
                    soon={f.status === "soon"}
                    version={f.since}
                    collapsed={collapsed}
                  />
                ))}
              </div>
            ))}
          </nav>

          <div className="mt-1 flex flex-col gap-0.5 border-t border-[var(--line)] pt-2">
            <button
              onClick={() => navigate("/settings")}
              title={t("nav.settings")}
              className={cn(
                "group/sb relative flex items-center gap-2.5 rounded-[var(--r-sm)] px-2.5 py-[0.4rem] text-[0.84rem] font-medium text-[var(--t2)] transition-colors duration-150 hover:bg-[var(--surface-2)] hover:text-[var(--t1)]",
                location.pathname === "/settings" &&
                  "bg-[var(--surface)] text-[var(--t1)] shadow-[var(--shadow-1),inset_0_0_0_1px_var(--line)]",
              )}
            >
              <SettingsIcon className="size-4 shrink-0" />
              <span className={cn("truncate", collapsed && "hidden")}>{t("nav.settings")}</span>
            </button>
            <button
              onClick={() => setCollapsed((v) => !v)}
              className="flex items-center gap-2.5 rounded-[var(--r-sm)] px-2.5 py-[0.4rem] text-[0.84rem] text-[var(--t3)] transition-colors hover:bg-[var(--surface-2)] hover:text-[var(--t2)]"
              title={collapsed ? t("common.expandSidebar") : t("common.collapseSidebar")}
            >
              {collapsed ? <PanelLeftOpen className="size-4 shrink-0" /> : <PanelLeftClose className="size-4 shrink-0" />}
              <span className={cn("truncate", collapsed && "hidden")}>{t("common.collapseSidebar")}</span>
            </button>
          </div>
        </aside>

        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          <header className="flex h-12 shrink-0 items-center gap-3 border-b border-[var(--line)] bg-[var(--surface)] px-3.5">
            <div className="flex min-w-0 items-center gap-1.5 text-sm">
              <span className="font-semibold tracking-tight text-[var(--t1)]">{section}</span>
              <span className="text-[var(--line-strong)]">/</span>
              <span className="truncate font-mono text-[11px] text-[var(--t2)]" title={ws.state.workspaceId ?? ""}>
                {wsName}
              </span>
            </div>

            <button
              onClick={() => setCmdOpen(true)}
              className="mx-auto flex h-8 max-w-[28rem] flex-1 items-center gap-2 rounded-[var(--r-sm)] border border-[var(--line)] bg-[var(--bg)] px-3 text-[var(--t3)] transition-colors hover:border-[var(--line-strong)] hover:bg-[var(--surface-2)] hover:text-[var(--t2)]"
              title={t("common.searchCommandTitle")}
            >
              <Search className="size-3.5" />
              <span className="truncate text-[0.8rem]">{t("common.commandPaletteSearch")}</span>
              <kbd className="ml-auto rounded-[5px] border border-[var(--line)] bg-[var(--surface)] px-1.5 py-0.5 font-mono text-[10px] leading-none text-[var(--t3)] shadow-[var(--shadow-1)]">
                ⌘K
              </kbd>
            </button>

            <div className="flex shrink-0 items-center gap-1.5">
              <Button variant="ghost" size="sm" className="gap-1 text-[var(--t2)]" onClick={onScan} title={t("common.scanWorkspaceTitle")}>
                <ScanLine /> <span className={cn(collapsed && "hidden")}>{t("common.scan")}</span>
              </Button>
              <Button variant="outline" size="sm" className="gap-1" onClick={onDir} title={t("common.openWorkspaceDir")}>
                <FolderOpen /> <span className={cn(collapsed && "hidden")}>{t("common.directory")}</span>
              </Button>
            </div>
          </header>
          <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
            {ws.state.error ? (
              <div className="m-3 rounded-lg border border-[var(--st-danger-ring)] bg-[var(--st-danger-tint)] px-3 py-2 text-sm text-[var(--st-danger)]">{ws.state.error}</div>
            ) : (
              <Outlet context={shellCtx} />
            )}
          </main>
        </div>
      </div>

      <StatusBar
        env={probeItems}
        left={<WatchingPulse active={!!ws.state.workspaceId} />}
        right={
          <>
            <span className="flex items-center gap-1.5 text-[var(--st-ok-deep)]">
              <span className="size-1.5 rounded-full bg-[var(--st-ok)]" />
              {ws.state.workspaceId ? t("common.workspaceSynced") : t("common.noWorkspace")}
            </span>
            <span className="text-[var(--line-strong)]">·</span>
            <span>v{state.hello?.product_version ?? "1.0"}</span>
          </>
        }
      />

      <CommandPalette open={cmdOpen} onClose={() => setCmdOpen(false)} features={features} />
    </div>
  );
}

const NAV_ICONS: Record<string, React.ReactNode> = {
  run: <Play className="size-4" />,
  logs: <ScrollText className="size-4" />,
  config: <SlidersHorizontal className="size-4" />,
  env: <KeyRound className="size-4" />,
  workspaces: <Folders className="size-4" />,
  discover: <Radar className="size-4" />,
  templates: <LayoutTemplate className="size-4" />,
  git: <GitBranch className="size-4" />,
  docker: <Container className="size-4" />,
  gateway: <Globe className="size-4" />,
  cloud: <Cloud className="size-4" />,
  ai: <Sparkles className="size-4" />,
};


