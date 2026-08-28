import { createContext, use, useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/components/ui/toast";
import { isTauri } from "../ipc/invoke";
import {
  IpcFailure,
  type RuntimeSnapshot,
  type ServiceRuntimeView,
  type ScriptRuntimeView,
  type ServiceMetrics,
} from "../ipc/protocol";
import {
  apiRuntimeSnapshot,
  apiMetricsSnapshot,
  apiMetricsSubscribe,
  apiMetricsUnsubscribe,
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
  metrics: Record<string, ServiceMetrics | null>;
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
    cb({
      protocol: 1,
      workspace_id: "",
      services: e.payload.payload.services,
      script: e.payload.payload.script,
      metrics: e.payload.payload.metrics,
    });
  });
  return un;
}

export function RuntimeProvider({ children }: { children: ReactNode }) {
  const { state: ws } = useWorkspace();
  const { t } = useTranslation();
  const wsId = ws.workspaceId;
  const [services, setServices] = useState<Record<string, ServiceRuntimeView>>({});
  const [script, setScript] = useState<ScriptRuntimeView | null>(null);
  const [metrics, setMetrics] = useState<Record<string, ServiceMetrics | null>>({});
  const [error, setError] = useState<string | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const prevRef = useRef<Record<string, ServiceRuntimeView>>({});
  const apply = (snap: RuntimeSnapshot) => {
    // 1.2 §8.5 崩溃通知：非 stop 退出（exit_reason=crash）时应用内 toast；
    // 只含服务 id 与退出码，不含日志行或环境变量。
    const prev = prevRef.current;
    for (const s of Object.values(snap.services)) {
      const before = prev[s.id];
      if (
        s.state === "exited" &&
        s.exit_reason === "crash" &&
        before &&
        before.state !== "exited"
      ) {
        toast(t("operations.serviceCrash", { id: s.id, code: s.last_exit?.code ?? "?" }), "err");
      }
    }
    prevRef.current = snap.services;
    setServices(snap.services);
    setScript(snap.script ?? null);
    setMetrics(snap.metrics ?? {});
  };

  useEffect(() => {
    if (!wsId) {
      setServices({});
      setScript(null);
      setMetrics({});
      return;
    }
    let alive = true;
    (async () => {
      try {
        const snap = await apiRuntimeSnapshot();
        if (alive) apply(snap);
        await apiMetricsSubscribe(wsId);
        const metrics = await apiMetricsSnapshot(wsId);
        if (alive) setMetrics(metrics.services);
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
      void apiMetricsUnsubscribe(wsId).catch(() => undefined);
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
    state: {
      snapshot: wsId ? { protocol: 1, workspace_id: wsId, services, script, metrics } : null,
      services,
      script,
      metrics,
      error,
    },
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
