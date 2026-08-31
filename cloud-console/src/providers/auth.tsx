import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import * as api from "@/lib/api";

type AuthContextValue = {
  session: api.Session | null;
  signIn: (email: string, password: string) => Promise<void>;
  signOut: () => void;
};

const AuthContext = createContext<AuthContextValue | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [session, setSession] = useState<api.Session | null>(() => api.getSession());

  const signIn = useCallback(async (email: string, password: string) => {
    setSession(await api.login(email, password));
  }, []);

  const signOut = useCallback(() => {
    api.logout();
    setSession(null);
  }, []);

  const value = useMemo(() => ({ session, signIn, signOut }), [session, signIn, signOut]);
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth 必须在 AuthProvider 内使用");
  return ctx;
}
