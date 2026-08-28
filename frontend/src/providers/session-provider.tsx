import { createContext, use, useEffect, useState, type ReactNode } from "react";
import { invoke } from "../ipc/invoke";
import { cmd, PROTOCOL, type AppLoadOut, type Feature, type HelloOut } from "../ipc/protocol";

type SessionState = {
  hello: HelloOut | null;
  app: AppLoadOut | null;
  error: string | null;
};

type SessionActions = {
  reload: () => Promise<void>;
};

type SessionMeta = {
  ready: boolean;
};

type SessionContextValue = {
  state: SessionState;
  actions: SessionActions;
  meta: SessionMeta;
};

const SessionContext = createContext<SessionContextValue | null>(null);

async function boot(): Promise<{ hello: HelloOut; app: AppLoadOut }> {
  const hello = await invoke<HelloOut>(cmd.SESSION_HELLO, {
    client: "ui",
    protocol: PROTOCOL,
  });
  const app = await invoke<AppLoadOut>(cmd.APP_LOAD);
  return { hello, app };
}

export function SessionProvider({ children }: { children: ReactNode }) {
  const [hello, setHello] = useState<HelloOut | null>(null);
  const [app, setApp] = useState<AppLoadOut | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = async () => {
    setError(null);
    try {
      const next = await boot();
      setHello(next.hello);
      setApp(next.app);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  useEffect(() => {
    void reload();
  }, []);

  const value: SessionContextValue = {
    state: { hello, app, error },
    actions: { reload },
    meta: { ready: hello !== null && app !== null },
  };

  return <SessionContext value={value}>{children}</SessionContext>;
}

export function useSession(): SessionContextValue {
  const ctx = use(SessionContext);
  if (!ctx) {
    throw new Error("useSession 必须在 SessionProvider 内");
  }
  return ctx;
}

export function useFeatures(): Feature[] {
  return useSession().state.hello?.features ?? [];
}
