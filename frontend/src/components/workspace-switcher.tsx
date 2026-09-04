import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  FolderOpen,
  FolderSearch,
  LayoutGrid,
  Loader2,
  Search,
  X,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useWorkspace } from "@/providers/workspace-provider";
import { useOpenWorkspace } from "@/lib/use-open-workspace";
import { useUnsavedGuard } from "@/providers/unsaved-guard";
import { isTauri } from "@/ipc/invoke";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

function wsNameOf(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

export function WorkspaceSwitcher({ collapsed }: { collapsed: boolean }) {
  const ws = useWorkspace();
  const openWs = useOpenWorkspace();
  const { confirmLeave } = useUnsavedGuard();
  const navigate = useNavigate();
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const [busy, setBusy] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const wsName = ws.state.workspaceId ? wsNameOf(ws.state.workspaceId) : t("common.noWorkspace");

  const otherRecents = useMemo(
    () => ws.state.recents.filter((p) => p !== ws.state.workspaceId),
    [ws.state.recents, ws.state.workspaceId],
  );

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return otherRecents;
    return otherRecents.filter(
      (p) => p.toLowerCase().includes(q) || wsNameOf(p).toLowerCase().includes(q),
    );
  }, [otherRecents, query]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  useEffect(() => {
    if (!open) {
      setQuery("");
      setActiveIndex(0);
      return;
    }
    const id = window.setTimeout(() => searchRef.current?.focus(), 0);
    return () => window.clearTimeout(id);
  }, [open]);

  useEffect(() => {
    setActiveIndex(0);
  }, [query]);

  useEffect(() => {
    if (!open || !listRef.current) return;
    const el = listRef.current.querySelector<HTMLElement>(`[data-ws-idx="${activeIndex}"]`);
    el?.scrollIntoView({ block: "nearest" });
  }, [activeIndex, open]);

  const pickDirectory = async () => {
    if (busy) return;
    setOpen(false);
    if (!isTauri()) {
      const p = window.prompt(t("common.inputWorkspacePath"));
      if (!p) return;
      setBusy(true);
      try {
        await openWs(p);
      } finally {
        setBusy(false);
      }
      return;
    }
    const selected = await openDialog({ directory: true, multiple: false });
    if (typeof selected !== "string") return;
    setBusy(true);
    try {
      await openWs(selected);
    } finally {
      setBusy(false);
    }
  };

  const switchTo = async (path: string) => {
    if (busy || path === ws.state.workspaceId) {
      setOpen(false);
      return;
    }
    setOpen(false);
    setBusy(true);
    try {
      await openWs(path);
    } finally {
      setBusy(false);
    }
  };

  const closeWorkspace = async () => {
    if (busy) return;
    setOpen(false);
    if (!(await confirmLeave())) return;
    setBusy(true);
    try {
      await ws.actions.close();
      navigate("/welcome");
    } finally {
      setBusy(false);
    }
  };

  const goWorkspacesPage = () => {
    setOpen(false);
    navigate("/workspaces");
  };

  const onPopoverKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
      return;
    }
    if (filtered.length === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIndex((i) => (i + 1) % filtered.length);
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex((i) => (i - 1 + filtered.length) % filtered.length);
      return;
    }
    if (e.key === "Home") {
      e.preventDefault();
      setActiveIndex(0);
      return;
    }
    if (e.key === "End") {
      e.preventDefault();
      setActiveIndex(filtered.length - 1);
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const path = filtered[activeIndex];
      if (path) void switchTo(path);
    }
  };

  return (
    <div ref={rootRef} className="relative mt-1">
      <button
        type="button"
        onClick={() => !busy && setOpen((v) => !v)}
        disabled={busy}
        className={cn(
          "flex w-full items-center gap-2 rounded-[var(--r-sm,8px)] px-1.5 py-2 text-left transition-colors duration-150 hover:bg-[rgb(0_0_0_/_0.045)]",
          open && "bg-[rgb(0_0_0_/_0.045)]",
          collapsed && "justify-center px-1",
          busy && "cursor-wait opacity-70",
        )}
        title={ws.state.workspaceId ?? t("workspace.switch")}
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-busy={busy}
      >
        <span className="grid size-7 shrink-0 place-items-center rounded-[9px] bg-gradient-to-br from-[#6E79DE] to-[#4F5AC8] text-[0.82rem] font-bold text-white shadow-[0_2px_8px_rgb(94_106_210_/_0.35)]">
          {busy ? <Loader2 className="size-3.5 animate-spin" /> : wsName.charAt(0).toUpperCase()}
        </span>
        <span className={cn("min-w-0 flex-1", collapsed && "hidden")}>
          <span className="block truncate text-[0.88rem] font-semibold leading-tight tracking-[-0.01em] text-[var(--t1,#222326)]">
            {wsName}
          </span>
          <span
            className="block truncate font-mono text-[0.58rem] text-[var(--t3,#8a8f98)]"
            title={ws.state.workspaceId ?? undefined}
          >
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
            "absolute left-0 top-[calc(100%+4px)] z-[120] w-[min(19rem,calc(100vw-2rem))] overflow-hidden rounded-[var(--r-md,12px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] shadow-[var(--shadow-2,0_6px_20px_rgb(16_24_40_/_0.09))]",
            collapsed && "left-12 w-80",
          )}
          onClick={(e) => e.stopPropagation()}
          onKeyDown={onPopoverKeyDown}
          role="listbox"
          aria-label={t("workspace.title")}
        >
          <div className="flex items-center justify-between gap-2 border-b border-[var(--line,#e6e6e6)] px-3 py-2">
            <div className="text-[0.58rem] font-bold uppercase tracking-[0.1em] text-[var(--t3,#8a8f98)]">
              {t("workspace.title")}
            </div>
            <button
              type="button"
              onClick={goWorkspacesPage}
              className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[0.68rem] font-medium text-[var(--st-accent,#5e6ad2)] hover:bg-[var(--st-accent-tint,#eef0fb)]"
            >
              <LayoutGrid className="size-3" />
              {t("workspace.managePage")}
            </button>
          </div>

          {ws.state.workspaceId ? (
            <div
              className="flex items-center gap-2 border-b border-[var(--line,#e6e6e6)] bg-[var(--st-accent-tint,#eef0fb)] px-3 py-2"
              title={ws.state.workspaceId}
            >
              <FolderOpen className="size-3.5 shrink-0 text-[var(--st-accent,#5e6ad2)]" />
              <div className="min-w-0 flex-1">
                <div className="truncate text-[0.8rem] font-semibold text-[var(--t1,#222326)]">{wsName}</div>
                <div className="truncate font-mono text-[0.62rem] text-[var(--t3,#8a8f98)]">{ws.state.workspaceId}</div>
              </div>
            </div>
          ) : null}

          {otherRecents.length > 0 ? (
            <div className="border-b border-[var(--line,#e6e6e6)] p-1.5">
              <div className="relative">
                <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-[var(--t3,#8a8f98)]" />
                <input
                  ref={searchRef}
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder={t("workspace.searchRecent")}
                  className="h-8 w-full rounded-[var(--r-sm,8px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] pl-7 pr-2 text-[0.75rem] text-[var(--t1,#222326)] outline-none placeholder:text-[var(--t3,#8a8f98)] focus-visible:border-[var(--st-accent,#5e6ad2)] focus-visible:ring-[2px] focus-visible:ring-[var(--st-accent-tint,#eef0fb)]"
                  aria-label={t("workspace.searchRecent")}
                />
              </div>
            </div>
          ) : null}

          {otherRecents.length > 0 ? (
            <div ref={listRef} className="max-h-52 overflow-y-auto p-1">
              <div className="px-2 py-1 text-[0.58rem] font-bold uppercase tracking-[0.1em] text-[var(--t3,#8a8f98)]">
                {t("workspace.recent")}
              </div>
              {filtered.length === 0 ? (
                <div className="px-2 py-3 text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("workspace.noSearchMatch")}</div>
              ) : (
                filtered.map((path, idx) => {
                  const name = wsNameOf(path);
                  const active = idx === activeIndex;
                  return (
                    <button
                      key={path}
                      type="button"
                      role="option"
                      aria-selected={active}
                      data-ws-idx={idx}
                      onMouseEnter={() => setActiveIndex(idx)}
                      onClick={() => void switchTo(path)}
                      className={cn(
                        "flex w-full flex-col rounded-[var(--r-sm,8px)] px-2 py-1.5 text-left transition-colors duration-150",
                        active
                          ? "bg-[var(--st-accent-tint,#eef0fb)] text-[var(--t1,#222326)]"
                          : "text-[var(--t1,#222326)] hover:bg-[rgb(0_0_0_/_0.045)]",
                      )}
                      title={path}
                    >
                      <span className="truncate text-[0.8rem] font-medium">{name}</span>
                      <span className="truncate font-mono text-[0.62rem] text-[var(--t3,#8a8f98)]">{path}</span>
                    </button>
                  );
                })
              )}
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
            <button
              type="button"
              onClick={goWorkspacesPage}
              className="flex w-full items-center gap-2 rounded-[var(--r-sm,8px)] px-2 py-1.5 text-[0.78rem] font-medium text-[var(--t1,#222326)] transition-colors hover:bg-[rgb(0_0_0_/_0.045)]"
            >
              <LayoutGrid className="size-3.5 text-[var(--st-accent,#5e6ad2)]" />
              {t("workspace.managePage")}
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
