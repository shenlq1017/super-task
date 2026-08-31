import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { Cloud, LogOut, Users } from "lucide-react";
import { ToastProvider } from "@/components/ui/toast";
import { AuthProvider, useAuth } from "@/providers/auth";
import { buttonVariants } from "@/components/ui/button";
import { LoginPage } from "@/pages/login-page";
import { AccountsPage } from "@/pages/accounts-page";
import { t } from "@/lib/labels";
import { cn } from "@/lib/utils";

function Shell({ children }: { children: React.ReactNode }) {
  const { session, signOut } = useAuth();
  return (
    <div className="flex min-h-full flex-col">
      <header className="flex h-12 shrink-0 items-center gap-2 border-b border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] px-4">
        <span className="grid size-6 place-items-center rounded-[var(--r-sm,8px)] bg-[var(--st-accent,#5e6ad2)] text-white">
          <Cloud className="size-3.5" />
        </span>
        <span className="text-[0.82rem] font-semibold text-[var(--t1,#222326)]">{t("app.title")}</span>
        <span className="ml-3 inline-flex items-center gap-1 text-[0.78rem] font-medium text-[var(--t1,#222326)]">
          <Users className="size-3.5 text-[var(--t3,#8a8f98)]" /> {t("app.accounts")}
        </span>
        <span className="ml-auto flex items-center gap-3">
          {session ? (
            <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">
              {t("app.signedInAs")} <span className="font-mono text-[var(--t2,#62666d)]">{session.email}</span>
            </span>
          ) : null}
          <button
            type="button"
            className={cn(buttonVariants({ variant: "outline", size: "sm" }), "gap-1")}
            onClick={signOut}
          >
            <LogOut className="size-3.5" /> {t("app.logout")}
          </button>
        </span>
      </header>
      <main className="min-h-0 flex-1">{children}</main>
    </div>
  );
}

export function App() {
  return (
    <ToastProvider>
      <AuthProvider>
        <HashRouter>
          <AppRoutes />
        </HashRouter>
      </AuthProvider>
    </ToastProvider>
  );
}

function AppRoutes() {
  const { session } = useAuth();
  const authed = session !== null;
  const home = authed ? "/accounts" : "/login";
  return (
    <Routes>
      <Route path="/login" element={authed ? <Navigate to={home} replace /> : <LoginPage />} />
      <Route
        path="/accounts"
        element={
          authed ? (
            <Shell>
              <AccountsPage />
            </Shell>
          ) : (
            <Navigate to={home} replace />
          )
        }
      />
      <Route path="*" element={<Navigate to={home} replace />} />
    </Routes>
  );
}
