import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, RotateCw, SquareTerminal } from "lucide-react";
import "@xterm/xterm/css/xterm.css";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { isTauri } from "@/ipc/invoke";
import { mockListen } from "@/ipc/mock";
import {
  apiTermClose,
  apiTermOpen,
  apiTermResize,
  apiTermWrite,
} from "@/ipc/api";
import { event, type TermEventEnvelope } from "@/ipc/protocol";
import { cn } from "@/lib/utils";

/**
 * 运行页「终端」Tab 共享视图（ipc.md §10.15）。
 * 复用 xterm.js 渲染；PTY 会话由后端托管（cwd/环境链来自引擎），本组件只做
 * open/write/resize/close 转发与 st.term 订阅。会话随组件卸载（切 Tab）关闭，
 * 与「终端不后台长驻」的产品语义一致。
 */

// 与运行页命令行块一致的深色终端风（#191B20 底）
const TERM_THEME = {
  background: "#191B20",
  foreground: "#E7E9EC",
  cursor: "#7B84EA",
  cursorAccent: "#191B20",
  selectionBackground: "#3A3F52",
  black: "#191B20",
  red: "#E5484D",
  green: "#46A758",
  yellow: "#F5A524",
  blue: "#5E6AD2",
  magenta: "#BF5AF2",
  cyan: "#2AA7A7",
  white: "#E7E9EC",
};

type Phase = "connecting" | "ready" | "exited" | "error";

export function TerminalView({
  workspaceId,
  serviceId,
  className,
}: {
  workspaceId: string;
  serviceId?: string | null;
  className?: string;
}) {
  const { t } = useTranslation();
  const hostRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionRef = useRef<number | null>(null);
  const disposedRef = useRef(false);
  const [phase, setPhase] = useState<Phase>("connecting");
  const [exitCode, setExitCode] = useState<number | null>(null);
  const [errMsg, setErrMsg] = useState<string | null>(null);
  const [reopenKey, setReopenKey] = useState(0);
  const [hintVisible, setHintVisible] = useState(false);

  // 会话就绪后短暂提示「切 Tab 会结束会话」，数秒后淡出，不常驻占位
  useEffect(() => {
    if (phase !== "ready") return;
    setHintVisible(true);
    const timer = window.setTimeout(() => setHintVisible(false), 6000);
    return () => window.clearTimeout(timer);
  }, [phase, reopenKey]);

  useEffect(() => {
    disposedRef.current = false;
    let unlisten: (() => void) | null = null;
    let ro: ResizeObserver | null = null;
    let resizeTimer: number | null = null;

    const term = new Terminal({
      fontFamily: "'Geist Mono', 'Cascadia Mono', Consolas, monospace",
      fontSize: 12.5,
      cursorBlink: true,
      convertEol: false,
      theme: TERM_THEME,
      scrollback: 5000,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon());
    termRef.current = term;
    fitRef.current = fit;

    const write = (data: string) => {
      if (!disposedRef.current) term.write(data);
    };

    const attach = (sessionId: number) => {
      sessionRef.current = sessionId;
      term.onData((data) => {
        void apiTermWrite(sessionId, data).catch(() => {});
      });
    };

    const fitNow = () => {
      if (disposedRef.current || !hostRef.current) return;
      try {
        fit.fit();
        const sid = sessionRef.current;
        if (sid != null && term.cols > 2 && term.rows > 2) {
          void apiTermResize(sid, term.cols, term.rows).catch(() => {});
        }
      } catch {
        /* 容器尺寸为 0 时忽略（Tab 切换中间态） */
      }
    };

    let alive = true;
    const open = async () => {
      if (!hostRef.current) return;
      term.open(hostRef.current);
      ro = new ResizeObserver(() => {
        if (resizeTimer != null) window.clearTimeout(resizeTimer);
        resizeTimer = window.setTimeout(fitNow, 100);
      });
      ro.observe(hostRef.current);

      // st.term 订阅（Tauri 事件 / mock 桥同形信封）
      const onEnvelope = (e: unknown) => {
        const env = e as TermEventEnvelope;
        const p = env?.payload;
        if (!p || p.session_id !== sessionRef.current) return;
        if (p.kind === "output" && p.data) {
          write(p.data);
        } else if (p.kind === "exited") {
          setExitCode(p.exit_code ?? null);
          setPhase("exited");
        }
      };
      // st.term 订阅（Tauri 事件 / mock 桥同形信封）。订阅失败必须落到 error 态，
      // 否则 open() 未捕获的 rejection 会让界面永远停在 connecting。
      try {
        if (isTauri()) {
          const mod = (await import("@tauri-apps/api/event")) as any;
          const listen = mod.listen as (name: string, cb: (e: any) => void) => Promise<() => void>;
          const un = await listen(event.TERM, (e: any) => onEnvelope(e?.payload));
          if (!alive) {
            un();
          } else {
            unlisten = un;
          }
        } else {
          unlisten = mockListen(event.TERM, onEnvelope);
        }
      } catch (e) {
        if (alive) {
          setErrMsg(e instanceof Error ? e.message : String(e));
          setPhase("error");
          return;
        }
      }
      if (!alive) return;

      try {
        const cols = Math.max(term.cols, 20);
        const rows = Math.max(term.rows, 8);
        const out = await apiTermOpen({
          workspaceId,
          serviceId: serviceId ?? null,
          cols,
          rows,
        });
        if (!alive) {
          void apiTermClose(out.session_id).catch(() => {});
          return;
        }
        attach(out.session_id);
        setPhase("ready");
        fitNow();
        term.focus();
      } catch (e) {
        setErrMsg(e instanceof Error ? e.message : String(e));
        setPhase("error");
      }
    };

    void open().catch((e) => {
      // 兜底：任何未捕获的 open 失败都落到可重开的 error 态，不留在 connecting
      if (!disposedRef.current) {
        setErrMsg(e instanceof Error ? e.message : String(e));
        setPhase("error");
      }
    });

    return () => {
      disposedRef.current = true;
      alive = false;
      if (resizeTimer != null) window.clearTimeout(resizeTimer);
      ro?.disconnect();
      if (unlisten) unlisten();
      const sid = sessionRef.current;
      sessionRef.current = null;
      if (sid != null) void apiTermClose(sid).catch(() => {});
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
    // 重开由 reopenKey 触发：整体重建会话
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspaceId, serviceId, reopenKey]);

  const reopen = () => {
    setExitCode(null);
    setErrMsg(null);
    setReopenKey((k) => k + 1);
  };

  return (
    <div className={cn("flex min-h-0 flex-1 flex-col", className)}>
      <div className="relative min-h-0 flex-1">
        {/* 背景铺满：xterm 只画行高，底部不足一行的空隙也要是黑的，随外层卡片圆角裁切对齐 */}
        <div ref={hostRef} className="absolute inset-0 bg-[#191B20]" />
        {phase === "connecting" ? (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-[#191B20] text-[0.8rem] text-[#9AA0AB]">
            <Loader2 className="mr-2 size-4 animate-spin" /> {t("pages.run.termConnecting")}
          </div>
        ) : null}
        {phase === "exited" || phase === "error" ? (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-[#191B20]/92 text-center">
            <p className="text-[0.82rem] text-[#9AA0AB]">
              {phase === "exited"
                ? t("pages.run.termExited", { code: exitCode ?? "—" })
                : errMsg || t("pages.run.termFailed")}
            </p>
            <button
              onClick={reopen}
              className="inline-flex cursor-pointer items-center gap-1.5 rounded-[var(--r-sm,8px)] border border-[#3A3F52] bg-[#23262D] px-3 py-1.5 text-[0.76rem] font-medium text-[#E7E9EC] transition-colors duration-150 hover:border-[#7B84EA] hover:bg-[#2B2E36]"
            >
              <RotateCw className="size-3.5" /> {t("pages.run.termReopen")}
            </button>
          </div>
        ) : null}
        <div
          className={cn(
            "pointer-events-none absolute bottom-2 left-1/2 flex -translate-x-1/2 items-center gap-1.5 rounded-[var(--r-sm,8px)] border border-[#2B2E36] bg-[#23262D]/95 px-3 py-1 text-[0.68rem] font-medium text-[#D5D8DE] transition-opacity duration-500",
            phase === "ready" && hintVisible ? "opacity-100" : "opacity-0"
          )}
        >
          <SquareTerminal className="size-3" />
          {t("pages.run.termHint")}
        </div>
      </div>
    </div>
  );
}
