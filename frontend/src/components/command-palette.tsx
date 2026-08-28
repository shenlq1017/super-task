import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Search } from "lucide-react";
import { cn } from "@/lib/utils";
import { useRuntime } from "@/providers/runtime-provider";
import { useWorkspace } from "@/providers/workspace-provider";
import { useToast } from "@/components/ui/toast";
import { useOpenWorkspace } from "@/lib/use-open-workspace";
import { isTauri } from "@/ipc/invoke";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { Feature } from "@/ipc/protocol";
import { navLabel } from "@/features/registry";

type Cmd = { id: string; title: string; hint?: string; run: () => void };

export function CommandPalette({
  open,
  onClose,
  features,
}: {
  open: boolean;
  onClose: () => void;
  features: Feature[];
}) {
  const [q, setQ] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const navigate = useNavigate();
  const runtime = useRuntime();
  const ws = useWorkspace();
  const openWs = useOpenWorkspace();
  const { toast } = useToast();

  useEffect(() => {
    if (open) {
      setQ("");
      setActive(0);
      setTimeout(() => inputRef.current?.focus(), 0);
    }
  }, [open]);

  const commands = useMemo<Cmd[]>(() => {
    const nav: Cmd[] = features
      .filter((f) => f.status !== "soon")
      .map((f) => ({
        id: `nav:${f.id}`,
        title: `前往 ${navLabel(f.id)}`,
        hint: f.path,
        run: () => navigate(f.path),
      }));
    nav.push({ id: "nav:welcome", title: "前往 欢迎 / 打开工作区", hint: "/welcome", run: () => navigate("/welcome") });

    const actions: Cmd[] = [
      {
        id: "run:all",
        title: "启动全部服务",
        hint: "runtime.startAll",
        run: () => void runtime.actions.startAll().then(() => toast("已发起启动全部", "ok")),
      },
      {
        id: "stop:all",
        title: "停止全部服务",
        hint: "runtime.stopAll",
        run: () => {
          const n = Object.values(runtime.state.services).filter((s) => s.state === "running").length;
          if (n === 0) {
            toast("没有运行中的服务", "info");
            return;
          }
          // 面板即将关闭无法挂受控弹框：用原生确认（与主按钮同语义）
          if (!window.confirm(`停止全部 ${n} 个运行中的服务？各服务的整棵进程树将被结束。`)) return;
          void runtime.actions.stopAll().then(() => toast("已停止全部", "ok"));
        },
      },
      {
        id: "ws:switch",
        title: "切换工作区",
        hint: "打开目录选择器",
        run: async () => {
          if (!isTauri()) {
            const p = window.prompt("输入工作区目录路径");
            if (p) await openWs(p);
            return;
          }
          const selected = await openDialog({ directory: true, multiple: false });
          if (typeof selected === "string") await openWs(selected);
        },
      },
      {
        id: "ws:close",
        title: "关闭当前工作区",
        hint: "workspace.close",
        run: () => void ws.actions.close().then(() => navigate("/welcome")),
      },
      {
        id: "ws:rescan",
        title: "重新扫描工作区",
        hint: "workspace.scanDraft",
        run: async () => {
          if (!ws.state.workspaceId) return toast("没有已打开的工作区", "warn");
          try {
            await ws.actions.scanDraft(ws.state.workspaceId);
            toast("已重新扫描", "ok");
          } catch (e) {
            toast(e instanceof Error ? e.message : String(e), "err");
          }
        },
      },
    ];
    return [...nav, ...actions];
  }, [features, navigate, runtime, ws, openWs, toast]);

  const filtered = useMemo(() => {
    const t = q.trim().toLowerCase();
    if (!t) return commands;
    return commands.filter((c) => (c.title + " " + (c.hint ?? "")).toLowerCase().includes(t));
  }, [q, commands]);

  useEffect(() => {
    setActive((a) => Math.min(a, Math.max(0, filtered.length - 1)));
  }, [filtered.length]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-[120] flex items-start justify-center bg-[rgb(16_24_40_/_0.28)] pt-[14vh] backdrop-blur-[2px]"
      onClick={onClose}
    >
      <div
        className="w-[min(34rem,92vw)] overflow-hidden rounded-[var(--r-lg,16px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] shadow-[var(--shadow-2,0_6px_20px_rgb(16_24_40_/_0.09))] animate-in fade-in slide-in-from-top-1 duration-200"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2 border-b border-[var(--line,#e6e6e6)] px-4 py-3">
          <Search className="size-4 shrink-0 text-[var(--t3,#8a8f98)]" />
          <input
            ref={inputRef}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setActive((a) => Math.min(a + 1, filtered.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setActive((a) => Math.max(a - 1, 0));
              } else if (e.key === "Enter") {
                e.preventDefault();
                const c = filtered[active];
                if (c) {
                  c.run();
                  onClose();
                }
              } else if (e.key === "Escape") {
                onClose();
              }
            }}
            placeholder="搜索命令或页面…"
            className="flex-1 border-0 bg-transparent text-[0.92rem] text-[var(--t1,#222326)] outline-none placeholder:text-[var(--t3,#8a8f98)]"
          />
          <kbd className="rounded border border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] px-1.5 py-0.5 font-mono text-[0.58rem] text-[var(--t3,#8a8f98)]">
            Esc
          </kbd>
        </div>
        <div className="max-h-80 overflow-y-auto p-1.5">
          {filtered.length === 0 ? (
            <div className="px-3 py-6 text-center text-[0.78rem] text-[var(--t3,#8a8f98)]">无匹配命令</div>
          ) : (
            filtered.map((c, i) => (
              <button
                key={c.id}
                onMouseEnter={() => setActive(i)}
                onClick={() => {
                  c.run();
                  onClose();
                }}
                className={cn(
                  "flex w-full items-center gap-2 rounded-[var(--r-sm,8px)] px-2.5 py-2 text-left text-[0.8rem] transition-colors duration-150",
                  i === active
                    ? "bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent-hover,#4f5ac8)]"
                    : "text-[var(--t1,#222326)] hover:bg-[var(--st-accent-tint,#eef0fb)] hover:text-[var(--st-accent-hover,#4f5ac8)]",
                )}
              >
                <span>{c.title}</span>
                {c.hint ? <span className="ml-auto font-mono text-[0.64rem] text-[var(--t3,#8a8f98)]">{c.hint}</span> : null}
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
