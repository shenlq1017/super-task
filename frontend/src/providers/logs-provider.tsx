import { createContext, use, useEffect, useRef, useState, type ReactNode } from "react";
import { isTauri } from "../ipc/invoke";
import { type LogLine, type LogSource } from "../ipc/protocol";
import { apiLogsClearView, apiLogsSnapshot } from "../ipc/api";
import { useWorkspace } from "./workspace-provider";

type LogsState = {
  all: LogLine[];
  nextSeq: number;
};

type LogsActions = {
  clear: (source: LogSource) => Promise<void>;
  refresh: () => Promise<void>;
};

type LogsContextValue = { state: LogsState; actions: LogsActions };

const LogsContext = createContext<LogsContextValue | null>(null);

async function listenLogs(cb: (items: LogLine[]) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  const mod = (await import("@tauri-apps/api/event")) as any;
  const listen = mod.listen as (event: string, handler: (e: any) => void) => Promise<() => void>;
  const un = await listen("st.logs", (e: any) => cb(e.payload.payload.items));
  return un;
}

export function LogsProvider({ children }: { children: ReactNode }) {
  const { state: ws } = useWorkspace();
  const wsId = ws.workspaceId;
  const [all, setAll] = useState<LogLine[]>([]);
  const [nextSeq, setNextSeq] = useState(0);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const seed = async () => {
    try {
      const out = await apiLogsSnapshot(null, 2000);
      setAll(out.items);
      setNextSeq(out.next_seq);
    } catch {
      /* ignore */
    }
  };

  useEffect(() => {
    if (!wsId) {
      setAll([]);
      setNextSeq(0);
      return;
    }
    let alive = true;
    void seed();
    const onItems = (items: LogLine[]) => {
      if (!alive || items.length === 0) return;
      setAll((prev) => {
        const max = prev.length ? prev[prev.length - 1].seq : 0;
        const fresh = items.filter((l) => l.seq > max);
        const merged = fresh.length ? [...prev, ...fresh] : prev;
        return merged.length > 5000 ? merged.slice(merged.length - 5000) : merged;
      });
    };
    let unlisten: (() => void) | null = null;
    listenLogs(onItems).then((u) => {
      unlisten = u;
    });
    pollRef.current = setInterval(async () => {
      try {
        const out = await apiLogsSnapshot(null, 2000);
        setNextSeq(out.next_seq);
        setAll((prev) => {
          const max = prev.length ? prev[prev.length - 1].seq : 0;
          const fresh = out.items.filter((l) => l.seq > max);
          const merged = fresh.length ? [...prev, ...fresh] : prev;
          return merged.length > 5000 ? merged.slice(merged.length - 5000) : merged;
        });
      } catch {
        /* ignore */
      }
    }, 1200);
    return () => {
      alive = false;
      if (pollRef.current) clearInterval(pollRef.current);
      if (unlisten) unlisten();
    };
  }, [wsId]);

  const clear = async (source: LogSource) => {
    try {
      await apiLogsClearView(source);
      setAll((prev) => prev.filter((l) => !(l.source.kind === source.kind && l.source.id === source.id)));
    } catch {
      /* ignore */
    }
  };

  const refresh = async () => {
    await seed();
  };

  const value: LogsContextValue = {
    state: { all, nextSeq },
    actions: { clear, refresh },
  };

  return <LogsContext value={value}>{children}</LogsContext>;
}

export function useLogs(): LogsContextValue {
  const ctx = use(LogsContext);
  if (!ctx) throw new Error("useLogs 必须在 LogsProvider 内");
  return ctx;
}

export function sourceKey(s: LogSource): string {
  return `${s.kind}:${s.id}`;
}

export function filterLogs(all: LogLine[], source: LogSource | null): LogLine[] {
  if (!source) return all;
  return all.filter((l) => l.source.kind === source.kind && l.source.id === source.id);
}
