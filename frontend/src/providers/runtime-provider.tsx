import { createContext, use, useEffect, useRef, useState, type ReactNode } from "react";
import { isTauri } from "../ipc/invoke";
import { IpcFailure, type RuntimeSnapshot, type ServiceRuntimeView, type ScriptRuntimeView } from "../ipc/protocol";
import {
  apiRuntimeSnapshot,
  apiStartAll,
  apiStartOne,
  apiStopAll,
  apiStopOne,
  apiRestartOne,
} from "../ipc/api";
import { useWorkspace } from "./workspace-provider";

type RuntimeState = {
  snapshot: RuntimeSnapshot | null;
  services: Record<string, ServiceRuntimeView>;
  script: ScriptRuntimeView | null;
  error: string | null;
};

type RuntimeActions = {
  startOne: (id: string) => Promise<void>;
  stopOne: (id: string) => Promise<void>;
  restartOne: (id: string) => Promise<void>;
  startAll: () => Promise<void>;
  stopAll: () => Promise<void>;
  clearError: () => void;
};

type RuntimeContextValue = { state: RuntimeState; actions: RuntimeActions };

const RuntimeContext = createContext<RuntimeContextValue | null>(null);

async function listenRuntime(cb: (s: RuntimeSnapshot) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  const mod = (await import("@tauri-apps/api/event")) as any;
  const listen = mod.listen as (event: string, handler: (e: any) => void) => Promise<() => void>;
  const un = await listen("st.runtime", (e: any) => {
    cb({ protocol: 1, workspace_id: "", services: e.payload.payload.services, script: e.payload.payload.script });
  });
  return un;
}

export function RuntimeProvider({ children }: { children: ReactNode }) {
  const { state: ws } = useWorkspace();
  const wsId = ws.workspaceId;
  const [services, setServices] = useState<Record<string, ServiceRuntimeView>>({});
  const [script, setScript] = useState<ScriptRuntimeView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const apply = (snap: RuntimeSnapshot) => {
    setServices(snap.services);
    setScript(snap.script ?? null);
  };

  useEffect(() => {
    if (!wsId) {
      setServices({});
      setScript(null);
      return;
    }
    let alive = true;
    (async () => {
      try {
        const snap = await apiRuntimeSnapshot();
        if (alive) apply(snap);
      } catch (e) {
        if (alive) setError(e instanceof IpcFailure ? e.message : String(e));
      }
    })();
    let unlisten: (() => void) | null = null;
    listenRuntime((snap) => {
      if (alive) apply(snap);
    }).then((u) => {
      unlisten = u;
    });
    // polling fallback (also drives mock mode)
    pollRef.current = setInterval(async () => {
      try {
        const snap = await apiRuntimeSnapshot();
        if (alive) apply(snap);
      } catch {
        /* ignore transient */
      }
    }, 1500);
    return () => {
      alive = false;
      if (pollRef.current) clearInterval(pollRef.current);
      if (unlisten) unlisten();
    };
  }, [wsId]);

  const wrap = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(e instanceof IpcFailure ? e.message : String(e));
    }
  };

  const value: RuntimeContextValue = {
    state: { snapshot: wsId ? { protocol: 1, workspace_id: wsId, services, script } : null, services, script, error },
    actions: {
      startOne: (id) => wrap(() => apiStartOne(id)),
      stopOne: (id) => wrap(() => apiStopOne(id)),
      restartOne: (id) => wrap(() => apiRestartOne(id)),
      startAll: () => wrap(() => apiStartAll()),
      stopAll: () => wrap(() => apiStopAll()),
      clearError: () => setError(null),
    },
  };

  return <RuntimeContext value={value}>{children}</RuntimeContext>;
}

export function useRuntime(): RuntimeContextValue {
  const ctx = use(RuntimeContext);
  if (!ctx) throw new Error("useRuntime 必须在 RuntimeProvider 内");
  return ctx;
}
