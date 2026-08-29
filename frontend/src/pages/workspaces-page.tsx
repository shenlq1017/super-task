import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import {
  ExternalLink,
  FolderOpen,
  FolderSearch,
  Layers,
  X,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { cn } from "@/lib/utils";
import { useWorkspace } from "../providers/workspace-provider";
import { useOpenWorkspace } from "../lib/use-open-workspace";
import { WorkspacePkgCard } from "../components/workspace-pkg-card";
import { isTauri } from "../ipc/invoke";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

function wsName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

export function WorkspacesPage() {
  const ws = useWorkspace();
  const openWs = useOpenWorkspace();
  const navigate = useNavigate();
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);

  const pickDirectory = async () => {
    if (!isTauri()) {
      const p = window.prompt(t("common.inputWorkspacePath"));
      if (p) await openWs(p);
      return;
    }
    const selected = await openDialog({ directory: true, multiple: false });
    if (typeof selected === "string") await openWs(selected);
  };

  const switchTo = async (path: string) => {
    if (busy) return;
    setBusy(true);
    try {
      await openWs(path);
    } finally {
      setBusy(false);
    }
  };

  const closeCurrent = async () => {
    // 关闭会停掉引擎侧工作区（并停止其服务进程树），加一道确认
    if (!window.confirm(t("pages.workspaces.confirmClose", { name: wsName(ws.state.workspaceId ?? "") }))) return;
    setBusy(true);
    try {
      await ws.actions.close();
      navigate("/workspaces"); // 无跳转效果，仅保持在本页进入空态
    } finally {
      setBusy(false);
    }
  };

  const removeRecent = (path: string) => {
    if (!window.confirm(t("pages.workspaces.confirmRemoveRecent", { name: wsName(path) }))) return;
    ws.actions.removeRecent(path);
  };

  const current = ws.state.workspaceId;
  const spec = ws.state.spec;
  const serviceCount = Object.keys(spec?.services ?? {}).length;
  const scriptCount = Object.keys(spec?.scripts ?? {}).length;
  // 最近列表去掉当前已打开的（它已单独成卡）
  const others = ws.state.recents.filter((p) => p !== current);
  const empty = !current && others.length === 0;

  if (empty) {
    return (
      <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 overflow-auto p-8">
        <FolderOpen className="size-10 text-[var(--line-strong,#d0d6e0)]" />
        <div className="text-[0.95rem] font-semibold text-[var(--t1,#222326)]">{t("pages.workspaces.emptyTitle")}</div>
        <div className="text-[0.8rem] text-[var(--t3,#8a8f98)]">{t("pages.workspaces.emptyDesc")}</div>
        <Button onClick={() => void pickDirectory()} disabled={busy} className="mt-1 gap-1.5">
          <FolderSearch /> {t("pages.workspaces.pickDir")}
        </Button>
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto flex max-w-4xl flex-col gap-6">
          {/* 标题行 */}
          <div className="flex items-center justify-between gap-3">
            <div>
              <h2 className="text-[1.05rem] font-bold tracking-tight text-[var(--t1,#222326)]">{t("workspace.title")}</h2>
              <p className="mt-0.5 text-[0.78rem] text-[var(--t3,#8a8f98)]">
                {t("pages.workspaces.headerDesc")}
              </p>
            </div>
            <Button variant="outline" size="sm" onClick={() => void pickDirectory()} disabled={busy} className="gap-1">
              <FolderSearch /> {t("workspace.openOther")}
            </Button>
          </div>

          {/* 工作区包（1.7 §9.1：导出/导入入口归位工作区模块） */}
          <div>
            <div className="mb-2 text-[0.68rem] font-bold uppercase tracking-[0.1em] text-[var(--t3,#8a8f98)]">{t("pages.workspaces.pkgLabel")}</div>
            <WorkspacePkgCard />
          </div>

          {/* 当前工作区 */}
          {current ? (
            <div>
              <div className="mb-2 text-[0.68rem] font-bold uppercase tracking-[0.1em] text-[var(--t3,#8a8f98)]">{t("pages.workspaces.currentLabel")}</div>
              <Card
                className={cn(
                  "border-[rgb(94_106_210_/_0.45)] bg-[var(--st-accent-tint,#eef0fb)] p-4",
                  busy && "opacity-70",
                )}
              >
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div className="flex min-w-0 items-center gap-3">
                    <span className="grid size-9 shrink-0 place-items-center rounded-[9px] bg-gradient-to-br from-[#6E79DE] to-[#4F5AC8] text-[0.82rem] font-bold text-white shadow-[0_2px_8px_rgb(94_106_210_/_0.35)]">
                      {wsName(current).charAt(0).toUpperCase()}
                    </span>
                    <div className="min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="truncate text-[0.92rem] font-semibold text-[var(--t1,#222326)]">{wsName(current)}</span>
                        <Badge variant="soon" className="shrink-0">{t("pages.workspaces.currentBadge")}</Badge>
                      </div>
                      <div className="truncate font-mono text-[0.64rem] text-[var(--t3,#8a8f98)]" title={current}>
                        {current}
                      </div>
                      {spec?.name || serviceCount > 0 || scriptCount > 0 ? (
                        <div className="mt-1 flex items-center gap-1.5 text-[0.72rem] text-[var(--t2,#62666d)]">
                          <Layers className="size-3 shrink-0" />
                          {[spec?.name, t("pages.workspaces.serviceCount", { n: serviceCount }), t("pages.workspaces.scriptCount", { n: scriptCount })].filter(Boolean).join(" · ")}
                        </div>
                      ) : null}
                    </div>
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    <Button variant="outline" size="sm" className="gap-1 bg-transparent" onClick={() => ws.actions.openExplorer()} disabled={busy}>
                      <ExternalLink /> {t("pages.workspaces.explorer")}
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      className="gap-1 border-red-200 bg-transparent text-[#DC2626] hover:border-[#DC2626] hover:bg-[#FDECEC] hover:text-[#DC2626]"
                      onClick={() => void closeCurrent()}
                      disabled={busy}
                    >
                      <X /> {t("pages.workspaces.closeWorkspace")}
                    </Button>
                  </div>
                </div>
              </Card>
            </div>
          ) : null}

          {/* 最近列表 */}
          {others.length > 0 ? (
            <div>
              <div className="mb-2 text-[0.68rem] font-bold uppercase tracking-[0.1em] text-[var(--t3,#8a8f98)]">{t("pages.workspaces.recentUsed")}</div>
              <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
                {others.map((path) => (
                  <Card
                    key={path}
                    role="button"
                    tabIndex={0}
                    onClick={() => void switchTo(path)}
                    onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && void switchTo(path)}
                    title={t("pages.workspaces.switchTo", { path })}
                    className={cn(
                      "group relative cursor-pointer p-3.5 transition-all duration-150 hover:-translate-y-px hover:border-[var(--st-accent,#5e6ad2)] hover:shadow-[var(--shadow-2,0_6px_20px_rgb(16_24_40_/_0.09))] focus-visible:outline-2 focus-visible:outline-[var(--st-accent,#5e6ad2)]",
                      busy && "pointer-events-none opacity-60",
                    )}
                  >
                    <button
                      type="button"
                      aria-label={t("pages.workspaces.removeAria", { name: wsName(path) })}
                      title={t("pages.workspaces.removeRecentTitle")}
                      onClick={(e) => {
                        e.stopPropagation();
                        removeRecent(path);
                      }}
                      className="absolute right-2 top-2 grid size-5 place-items-center rounded-full text-[var(--t3,#8a8f98)] opacity-0 transition-all duration-150 hover:bg-[rgb(0_0_0_/_0.06)] hover:text-[var(--t1,#222326)] group-hover:opacity-100 focus-visible:opacity-100"
                    >
                      <X className="size-3" />
                    </button>
                    <div className="flex min-w-0 items-center gap-2 pr-5">
                      <span className="grid size-7 shrink-0 place-items-center rounded-[7px] bg-[rgb(0_0_0_/_0.05)] text-[0.68rem] font-bold text-[var(--t2,#62666d)]">
                        {wsName(path).charAt(0).toUpperCase()}
                      </span>
                      <span className="truncate text-[0.85rem] font-semibold text-[var(--t1,#222326)]">{wsName(path)}</span>
                    </div>
                    <div className="mt-1 truncate font-mono text-[0.62rem] text-[var(--t3,#8a8f98)]" title={path}>
                      {path}
                    </div>
                    <div className="mt-2.5 flex justify-end">
                      <Button
                        variant="outline"
                        size="sm"
                        className="h-6 gap-1 px-2 text-[0.72rem]"
                        onClick={(e) => {
                          e.stopPropagation();
                          void switchTo(path);
                        }}
                        disabled={busy}
                      >
                        {t("pages.workspaces.switch")}
                      </Button>
                    </div>
                  </Card>
                ))}
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}
