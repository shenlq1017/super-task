import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { ChevronDown, FolderOpen, FolderSearch, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { useWorkspace } from "@/providers/workspace-provider";
import { useOpenWorkspace } from "@/lib/use-open-workspace";
import { useUnsavedGuard } from "@/providers/unsaved-guard";
import { isTauri } from "@/ipc/invoke";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

export function WorkspaceSwitcher({ collapsed }: { collapsed: boolean }) {
  const ws = useWorkspace();
  const openWs = useOpenWorkspace();
  const { confirmLeave } = useUnsavedGuard();
  const navigate = useNavigate();
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  const wsName =
    ws.state.workspaceId?.split(/[\\/]/).filter(Boolean).pop() ?? t("common.noWorkspace");

  // 当前工作区已在顶部单独展示，最近列表里排除自身
  const otherRecents = ws.state.recents.filter((p) => p !== ws.state.workspaceId);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  const pickDirectory = async () => {
    setOpen(false);
    if (!isTauri()) {
      const p = window.prompt(t("common.inputWorkspacePath"));
      if (p) await openWs(p);
      return;
    }
    const selected = await openDialog({ directory: true, multiple: false });
    if (typeof selected === "string") await openWs(selected);
  };

  const switchTo = async (path: string) => {
    setOpen(false);
    if (path === ws.state.workspaceId) return;
    await openWs(path);
  };

  const closeWorkspace = async () => {
    setOpen(false);
    // 关闭工作区会停服务并清空 spec，先过未保存守卫
    if (!(await confirmLeave())) return;
    await ws.actions.close();
    navigate("/welcome");
  };

  return (
    <div ref={rootRef} className="relative mt-1">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className={cn(
          "flex w-full items-center gap-2 rounded-[var(--r-sm,8px)] px-1.5 py-2 text-left transition-colors duration-150 hover:bg-[rgb(0_0_0_/_0.045)]",
          open && "bg-[rgb(0_0_0_/_0.045)]",
          collapsed && "justify-center px-1",
        )}
        title={t("workspace.switch")}
        aria-expanded={open}
      >
        <span className="grid size-7 shrink-0 place-items-center rounded-[9px] bg-gradient-to-br from-[#6E79DE] to-[#4F5AC8] text-[0.82rem] font-bold text-white shadow-[0_2px_8px_rgb(94_106_210_/_0.35)]">
          {wsName.charAt(0).toUpperCase()}
        </span>
        <span className={cn("min-w-0 flex-1", collapsed && "hidden")}>
          <span className="block truncate text-[0.88rem] font-semibold leading-tight tracking-[-0.01em] text-[var(--t1,#222326)]">
            {wsName}
          </span>
          <span className="block truncate font-mono text-[0.58rem] text-[var(--t3,#8a8f98)]">
            {ws.state.workspaceId ?? t("workspace.pickHint")}
          </span>
        </span>
        <ChevronDown
          className={cn(
            "size-4 shrink-0 text-[var(--t3,#8a8f98)] transition-transform duration-200",
            collapsed && "hidden",
            open && "rotate-180",
          )}
        />
      </button>

      {open ? (
        <div
          className={cn(
            "absolute left-0 top-[calc(100%+4px)] z-[120] w-[min(18rem,calc(100vw-2rem))] overflow-hidden rounded-[var(--r-md,12px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] shadow-[var(--shadow-2,0_6px_20px_rgb(16_24_40_/_0.09))]",
            collapsed && "left-12 w-72",
          )}
          onClick={(e) => e.stopPropagation()}
        >
          <div className="border-b border-[var(--line,#e6e6e6)] px-3 py-2 text-[0.58rem] font-bold uppercase tracking-[0.1em] text-[var(--t3,#8a8f98)]">
            {t("workspace.title")}
          </div>

          {ws.state.workspaceId ? (
            <div className="flex items-center gap-2 border-b border-[var(--line,#e6e6e6)] bg-[var(--st-accent-tint,#eef0fb)] px-3 py-2">
              <FolderOpen className="size-3.5 shrink-0 text-[var(--st-accent,#5e6ad2)]" />
              <div className="min-w-0 flex-1">
                <div className="truncate text-[0.8rem] font-semibold text-[var(--t1,#222326)]">{wsName}</div>
                <div className="truncate font-mono text-[0.62rem] text-[var(--t3,#8a8f98)]">{ws.state.workspaceId}</div>
              </div>
            </div>
          ) : null}

          {otherRecents.length > 0 ? (
            <div className="max-h-48 overflow-y-auto p-1">
              <div className="px-2 py-1 text-[0.58rem] font-bold uppercase tracking-[0.1em] text-[var(--t3,#8a8f98)]">
                {t("workspace.recent")}
              </div>
              {otherRecents.map((path) => {
                const name = path.split(/[\\/]/).filter(Boolean).pop() ?? path;
                return (
                  <button
                    key={path}
                    type="button"
                    onClick={() => void switchTo(path)}
                    className={cn(
                      "flex w-full flex-col rounded-[var(--r-sm,8px)] px-2 py-1.5 text-left transition-colors duration-150",
                      "text-[var(--t1,#222326)] hover:bg-[rgb(0_0_0_/_0.045)]",
                    )}
                    title={path}
                  >
                    <span className="truncate text-[0.8rem] font-medium">{name}</span>
                    <span className="truncate font-mono text-[0.62rem] text-[var(--t3,#8a8f98)]">{path}</span>
                  </button>
                );
              })}
            </div>
          ) : (
            <div className="px-3 py-3 text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("workspace.noRecent")}</div>
          )}

          <div className="flex flex-col gap-0.5 border-t border-[var(--line,#e6e6e6)] p-1">
            <button
              type="button"
              onClick={() => void pickDirectory()}
              className="flex w-full items-center gap-2 rounded-[var(--r-sm,8px)] px-2 py-1.5 text-[0.78rem] font-medium text-[var(--t1,#222326)] transition-colors hover:bg-[rgb(0_0_0_/_0.045)]"
            >
              <FolderSearch className="size-3.5 text-[var(--st-accent,#5e6ad2)]" />
              {t("workspace.openOther")}
            </button>
            {ws.state.workspaceId ? (
              <button
                type="button"
                onClick={() => void closeWorkspace()}
                className="flex w-full items-center gap-2 rounded-[var(--r-sm,8px)] px-2 py-1.5 text-[0.78rem] font-medium text-[var(--st-danger,#dc2626)] transition-colors hover:bg-[var(--st-danger-tint,#fdecec)]"
              >
                <X className="size-3.5" />
                {t("palette.closeWorkspace")}
              </button>
            ) : null}
          </div>
        </div>
      ) : null}
    </div>
  );
}
