import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import {
  ArrowDownAZ,
  Clock3,
  ExternalLink,
  FolderOpen,
  FolderSearch,
  Layers,
  Loader2,
  Search,
  X,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { useWorkspace } from "../providers/workspace-provider";
import { useOpenWorkspace } from "../lib/use-open-workspace";
import { WorkspacePkgCard } from "../components/workspace-pkg-card";
import { WorkspaceDataCard } from "../components/workspace-data-card";
import { isTauri } from "../ipc/invoke";
import { apiOpenExplorer } from "../ipc/api";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

const PREFS_KEY = "st:workspaces:prefs";

type SortMode = "recent" | "name" | "path";

type Prefs = {
  sort: SortMode;
  query: string;
};

function readPrefs(): Prefs {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (!raw) return { sort: "recent", query: "" };
    const parsed = JSON.parse(raw) as Partial<Prefs>;
    const sort: SortMode =
      parsed.sort === "name" || parsed.sort === "path" || parsed.sort === "recent" ? parsed.sort : "recent";
    return { sort, query: typeof parsed.query === "string" ? parsed.query : "" };
  } catch {
    return { sort: "recent", query: "" };
  }
}

function writePrefs(prefs: Prefs) {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch {
    /* ignore quota / private mode */
  }
}

function wsName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

type ConfirmState =
  | { kind: "close"; name: string }
  | { kind: "forget"; path: string; name: string }
  | null;

export function WorkspacesPage() {
  const ws = useWorkspace();
  const openWs = useOpenWorkspace();
  const navigate = useNavigate();
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [busyPath, setBusyPath] = useState<string | null>(null);
  const [prefs, setPrefs] = useState<Prefs>(() => readPrefs());
  const [confirm, setConfirm] = useState<ConfirmState>(null);

  useEffect(() => {
    writePrefs(prefs);
  }, [prefs]);

  const pickDirectory = async () => {
    if (busy) return;
    if (!isTauri()) {
      const p = window.prompt(t("common.inputWorkspacePath"));
      if (p) {
        setBusy(true);
        setBusyPath(p);
        try {
          await openWs(p);
        } finally {
          setBusy(false);
          setBusyPath(null);
        }
      }
      return;
    }
    const selected = await openDialog({ directory: true, multiple: false });
    if (typeof selected === "string") {
      setBusy(true);
      setBusyPath(selected);
      try {
        await openWs(selected);
      } finally {
        setBusy(false);
        setBusyPath(null);
      }
    }
  };

  const switchTo = async (path: string) => {
    if (busy || path === ws.state.workspaceId) return;
    setBusy(true);
    setBusyPath(path);
    try {
      await openWs(path);
    } finally {
      setBusy(false);
      setBusyPath(null);
    }
  };

  const openInExplorer = (path: string) => {
    if (busy) return;
    if (path === ws.state.workspaceId) {
      ws.actions.openExplorer();
      return;
    }
    void apiOpenExplorer(path);
  };

  const requestCloseCurrent = () => {
    if (!ws.state.workspaceId || busy) return;
    setConfirm({ kind: "close", name: wsName(ws.state.workspaceId) });
  };

  const requestForget = (path: string) => {
    if (busy) return;
    setConfirm({ kind: "forget", path, name: wsName(path) });
  };

  const onConfirm = () => {
    const c = confirm;
    setConfirm(null);
    if (!c) return;
    if (c.kind === "close") {
      void (async () => {
        setBusy(true);
        try {
          await ws.actions.close();
          navigate("/workspaces");
        } finally {
          setBusy(false);
          setBusyPath(null);
        }
      })();
      return;
    }
    ws.actions.removeRecent(c.path);
  };

  const current = ws.state.workspaceId;
  const spec = ws.state.spec;
  const serviceCount = Object.keys(spec?.services ?? {}).length;
  const scriptCount = Object.keys(spec?.scripts ?? {}).length;
  const others = useMemo(
    () => ws.state.recents.filter((p) => p !== current),
    [ws.state.recents, current],
  );
  const empty = !current && others.length === 0;

  const filtered = useMemo(() => {
    const q = prefs.query.trim().toLowerCase();
    let list = others;
    if (q) {
      list = list.filter((p) => p.toLowerCase().includes(q) || wsName(p).toLowerCase().includes(q));
    }
    if (prefs.sort === "name") {
      list = [...list].sort((a, b) => wsName(a).localeCompare(wsName(b), undefined, { sensitivity: "base" }));
    } else if (prefs.sort === "path") {
      list = [...list].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));
    }
    return list;
  }, [others, prefs.query, prefs.sort]);

  if (empty) {
    return (
      <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-4 overflow-auto p-8">
        <div className="grid size-16 place-items-center rounded-2xl bg-[var(--st-accent-tint,#eef0fb)] shadow-[inset_0_0_0_1px_rgb(94_106_210_/_0.18)]">
          <FolderOpen className="size-8 text-[var(--st-accent,#5e6ad2)]" />
        </div>
        <div className="max-w-md text-center">
          <div className="text-[1.05rem] font-semibold tracking-tight text-[var(--t1,#222326)]">
            {t("pages.workspaces.emptyTitle")}
          </div>
          <div className="mt-1.5 text-[0.8rem] leading-relaxed text-[var(--t3,#8a8f98)]">
            {t("pages.workspaces.emptyDesc")}
          </div>
        </div>
        <Button onClick={() => void pickDirectory()} disabled={busy} className="mt-1 gap-1.5">
          {busy ? <Loader2 className="animate-spin" /> : <FolderSearch />}
          {t("pages.workspaces.pickDir")}
        </Button>
        <p className="text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.workspaces.emptyHint")}</p>
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col" aria-busy={busy}>
      <div className="min-h-0 flex-1 overflow-auto p-5 sm:p-6">
        <div className="mx-auto flex max-w-5xl flex-col gap-5">
          <div className="flex flex-wrap items-end justify-between gap-3">
            <div className="min-w-0">
              <h2 className="text-[1.05rem] font-bold tracking-tight text-[var(--t1,#222326)]">{t("workspace.title")}</h2>
              <p className="mt-0.5 text-[0.78rem] leading-relaxed text-[var(--t3,#8a8f98)]">
                {t("pages.workspaces.headerDesc")}
              </p>
            </div>
            <Button variant="outline" size="sm" onClick={() => void pickDirectory()} disabled={busy} className="gap-1">
              {busy && !busyPath ? <Loader2 className="animate-spin" /> : <FolderSearch />}
              {t("workspace.openOther")}
            </Button>
          </div>

          <div>
            <div className="mb-2 text-[0.68rem] font-bold uppercase tracking-[0.1em] text-[var(--t3,#8a8f98)]">
              {t("pages.workspaces.pkgLabel")}
            </div>
            <WorkspacePkgCard />
          </div>

          <div>
            <div className="mb-2 text-[0.68rem] font-bold uppercase tracking-[0.1em] text-[var(--t3,#8a8f98)]">
              {t("pages.workspaces.dataTitle")}
            </div>
            <WorkspaceDataCard />
          </div>

          {current ? (
            <div>
              <div className="mb-2 text-[0.68rem] font-bold uppercase tracking-[0.1em] text-[var(--t3,#8a8f98)]">
                {t("pages.workspaces.currentLabel")}
              </div>
              <Card
                className={cn(
                  "relative overflow-hidden border-[rgb(94_106_210_/_0.45)] bg-[var(--st-accent-tint,#eef0fb)] p-4",
                  busy && "opacity-80",
                )}
              >
                <div className="pointer-events-none absolute -right-8 -top-10 size-36 rounded-full bg-[rgb(94_106_210_/_0.08)]" />
                <div className="relative flex flex-wrap items-start justify-between gap-3">
                  <div className="flex min-w-0 items-start gap-3">
                    <span className="grid size-10 shrink-0 place-items-center rounded-[10px] bg-gradient-to-br from-[#6E79DE] to-[#4F5AC8] text-[0.9rem] font-bold text-white shadow-[0_2px_8px_rgb(94_106_210_/_0.35)]">
                      {wsName(current).charAt(0).toUpperCase()}
                    </span>
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="truncate text-[0.95rem] font-semibold text-[var(--t1,#222326)]">
                          {wsName(current)}
                        </span>
                        <Badge variant="soon" className="shrink-0">
                          {t("pages.workspaces.currentBadge")}
                        </Badge>
                      </div>
                      <div
                        className="mt-0.5 truncate font-mono text-[0.64rem] text-[var(--t3,#8a8f98)]"
                        title={current}
                      >
                        {current}
                      </div>
                      <div className="mt-2 flex flex-wrap items-center gap-1.5">
                        {spec?.name ? (
                          <span className="inline-flex items-center gap-1 rounded-full bg-[rgb(255_255_255_/_0.7)] px-2 py-0.5 text-[0.7rem] text-[var(--t2,#62666d)] shadow-[inset_0_0_0_1px_rgb(94_106_210_/_0.15)]">
                            <Layers className="size-3" />
                            {spec.name}
                          </span>
                        ) : null}
                        <span className="inline-flex items-center rounded-full bg-[rgb(255_255_255_/_0.7)] px-2 py-0.5 text-[0.7rem] text-[var(--t2,#62666d)] shadow-[inset_0_0_0_1px_rgb(94_106_210_/_0.15)]">
                          {t("pages.workspaces.serviceCount", { n: serviceCount })}
                        </span>
                        <span className="inline-flex items-center rounded-full bg-[rgb(255_255_255_/_0.7)] px-2 py-0.5 text-[0.7rem] text-[var(--t2,#62666d)] shadow-[inset_0_0_0_1px_rgb(94_106_210_/_0.15)]">
                          {t("pages.workspaces.scriptCount", { n: scriptCount })}
                        </span>
                      </div>
                    </div>
                  </div>
                  <div className="flex shrink-0 flex-wrap items-center gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      className="gap-1 bg-[rgb(255_255_255_/_0.65)]"
                      onClick={() => openInExplorer(current)}
                      disabled={busy}
                      title={current}
                    >
                      <ExternalLink /> {t("pages.workspaces.explorer")}
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      className="gap-1 border-red-200 bg-[rgb(255_255_255_/_0.65)] text-[#DC2626] hover:border-[#DC2626] hover:bg-[#FDECEC] hover:text-[#DC2626]"
                      onClick={requestCloseCurrent}
                      disabled={busy}
                    >
                      <X /> {t("pages.workspaces.closeWorkspace")}
                    </Button>
                  </div>
                </div>
                {busy && busyPath === current ? (
                  <div className="absolute inset-0 grid place-items-center bg-[rgb(255_255_255_/_0.45)] backdrop-blur-[1px]">
                    <span className="inline-flex items-center gap-1.5 rounded-full bg-white px-3 py-1 text-[0.75rem] font-medium text-[var(--t2,#62666d)] shadow">
                      <Loader2 className="size-3.5 animate-spin text-[var(--st-accent,#5e6ad2)]" />
                      {t("pages.workspaces.switching")}
                    </span>
                  </div>
                ) : null}
              </Card>
            </div>
          ) : null}

          {others.length > 0 ? (
            <div>
              <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
                <div className="text-[0.68rem] font-bold uppercase tracking-[0.1em] text-[var(--t3,#8a8f98)]">
                  {t("pages.workspaces.recentUsed")}
                  <span className="ml-1.5 font-medium normal-case tracking-normal text-[var(--t3,#8a8f98)]">
                    ({filtered.length}/{others.length})
                  </span>
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <div className="relative min-w-[12rem] flex-1 sm:max-w-xs">
                    <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-[var(--t3,#8a8f98)]" />
                    <Input
                      value={prefs.query}
                      onChange={(e) => setPrefs((p) => ({ ...p, query: e.target.value }))}
                      placeholder={t("pages.workspaces.searchPlaceholder")}
                      className="h-8 pl-8 text-[0.78rem]"
                      aria-label={t("pages.workspaces.searchPlaceholder")}
                      disabled={busy}
                    />
                    {prefs.query ? (
                      <button
                        type="button"
                        className="absolute right-2 top-1/2 grid size-4 -translate-y-1/2 place-items-center rounded text-[var(--t3,#8a8f98)] hover:text-[var(--t1,#222326)]"
                        onClick={() => setPrefs((p) => ({ ...p, query: "" }))}
                        aria-label={t("common.clear")}
                      >
                        <X className="size-3" />
                      </button>
                    ) : null}
                  </div>
                  <div className="inline-flex overflow-hidden rounded-[var(--r-sm,8px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)]">
                    {(
                      [
                        { id: "recent", icon: Clock3, label: t("pages.workspaces.sortRecent") },
                        { id: "name", icon: ArrowDownAZ, label: t("pages.workspaces.sortName") },
                        { id: "path", icon: FolderOpen, label: t("pages.workspaces.sortPath") },
                      ] as const
                    ).map((opt) => (
                      <button
                        key={opt.id}
                        type="button"
                        disabled={busy}
                        title={opt.label}
                        aria-pressed={prefs.sort === opt.id}
                        onClick={() => setPrefs((p) => ({ ...p, sort: opt.id }))}
                        className={cn(
                          "inline-flex h-8 items-center gap-1 px-2.5 text-[0.72rem] font-medium transition-colors",
                          prefs.sort === opt.id
                            ? "bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent,#5e6ad2)]"
                            : "text-[var(--t2,#62666d)] hover:bg-[rgb(0_0_0_/_0.035)]",
                        )}
                      >
                        <opt.icon className="size-3.5" />
                        <span className="hidden sm:inline">{opt.label}</span>
                      </button>
                    ))}
                  </div>
                </div>
              </div>

              {filtered.length === 0 ? (
                <Card className="flex flex-col items-center gap-2 p-8 text-center">
                  <Search className="size-6 text-[var(--t3,#8a8f98)]" />
                  <div className="text-[0.85rem] font-medium text-[var(--t1,#222326)]">
                    {t("pages.workspaces.noMatch")}
                  </div>
                  <Button variant="outline" size="sm" onClick={() => setPrefs((p) => ({ ...p, query: "" }))}>
                    {t("common.clear")}
                  </Button>
                </Card>
              ) : (
                <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
                  {filtered.map((path) => {
                    const name = wsName(path);
                    const rowBusy = busy && busyPath === path;
                    return (
                      <Card
                        key={path}
                        role="button"
                        tabIndex={busy ? -1 : 0}
                        onClick={() => void switchTo(path)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault();
                            void switchTo(path);
                          }
                        }}
                        title={t("pages.workspaces.switchTo", { path })}
                        className={cn(
                          "group relative cursor-pointer p-3 transition-all duration-150 hover:-translate-y-px hover:border-[var(--st-accent,#5e6ad2)] hover:shadow-[var(--shadow-2,0_6px_20px_rgb(16_24_40_/_0.09))] focus-visible:outline-2 focus-visible:outline-[var(--st-accent,#5e6ad2)]",
                          busy && "pointer-events-none opacity-60",
                          rowBusy && "opacity-100",
                        )}
                        aria-busy={rowBusy}
                      >
                        <div className="flex min-w-0 items-center gap-2 pr-1">
                          <span className="grid size-7 shrink-0 place-items-center rounded-[7px] bg-[rgb(0_0_0_/_0.05)] text-[0.68rem] font-bold text-[var(--t2,#62666d)]">
                            {name.charAt(0).toUpperCase()}
                          </span>
                          <span className="min-w-0 flex-1 truncate text-[0.85rem] font-semibold text-[var(--t1,#222326)]">
                            {name}
                          </span>
                        </div>
                        <div className="mt-1 truncate font-mono text-[0.62rem] text-[var(--t3,#8a8f98)]" title={path}>
                          {path}
                        </div>
                        <div className="mt-2.5 flex flex-wrap items-center justify-end gap-1.5">
                          <Button
                            variant="outline"
                            size="sm"
                            className="h-6 gap-1 px-2 text-[0.72rem]"
                            title={path}
                            onClick={(e) => {
                              e.stopPropagation();
                              openInExplorer(path);
                            }}
                            disabled={busy}
                          >
                            <ExternalLink className="size-3" />
                            {t("pages.workspaces.explorer")}
                          </Button>
                          <Button
                            variant="outline"
                            size="sm"
                            className="h-6 gap-1 px-2 text-[0.72rem] text-[#DC2626] hover:border-[#DC2626] hover:bg-[#FDECEC] hover:text-[#DC2626]"
                            aria-label={t("pages.workspaces.removeAria", { name })}
                            title={t("pages.workspaces.removeRecentTitle")}
                            onClick={(e) => {
                              e.stopPropagation();
                              requestForget(path);
                            }}
                            disabled={busy}
                          >
                            <X className="size-3" />
                            {t("pages.workspaces.forget")}
                          </Button>
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
                            {rowBusy ? <Loader2 className="size-3 animate-spin" /> : null}
                            {t("pages.workspaces.open")}
                          </Button>
                        </div>
                        {rowBusy ? (
                          <div className="absolute inset-0 grid place-items-center rounded-[inherit] bg-[rgb(255_255_255_/_0.55)]">
                            <Loader2 className="size-4 animate-spin text-[var(--st-accent,#5e6ad2)]" />
                          </div>
                        ) : null}
                      </Card>
                    );
                  })}
                </div>
              )}
            </div>
          ) : current ? (
            <Card className="flex flex-col items-center gap-2 p-6 text-center">
              <Clock3 className="size-5 text-[var(--t3,#8a8f98)]" />
              <div className="text-[0.8rem] text-[var(--t3,#8a8f98)]">{t("workspace.noRecent")}</div>
            </Card>
          ) : null}
        </div>
      </div>

      <ConfirmDialog
        open={confirm?.kind === "close"}
        title={t("pages.workspaces.confirmCloseTitle")}
        description={
          confirm?.kind === "close" ? t("pages.workspaces.confirmClose", { name: confirm.name }) : undefined
        }
        destructive
        confirmText={t("pages.workspaces.closeWorkspace")}
        onConfirm={onConfirm}
        onCancel={() => setConfirm(null)}
      />
      <ConfirmDialog
        open={confirm?.kind === "forget"}
        title={t("pages.workspaces.confirmRemoveTitle")}
        description={
          confirm?.kind === "forget"
            ? t("pages.workspaces.confirmRemoveRecent", { name: confirm.name })
            : undefined
        }
        destructive
        confirmText={t("pages.workspaces.forget")}
        onConfirm={onConfirm}
        onCancel={() => setConfirm(null)}
      />
    </div>
  );
}
