import { Navigate, Route, Routes, useLocation } from "react-router-dom";
import type { ComponentType, ReactNode } from "react";
import { AppShell } from "./AppShell";
import { ComingSoonPage } from "../pages/coming-soon-page";
import {
  ConfigPage,
  EnvPage,
  LogsPage,
  RunPage,
  SettingsPage,
  WelcomePage,
  WorkspacesPage,
} from "../pages/live-pages";
import { DiscoverPage } from "../pages/discover-page";
import { TemplatesPage } from "../pages/templates-page";
import { GitPage } from "../pages/git-page";
import { useFeatures } from "../providers/session-provider";
import { useWorkspace } from "../providers/workspace-provider";
import { OperationProvider } from "../providers/operation-provider";

const LIVE_PAGES: Record<string, ComponentType> = {
  run: RunPage,
  logs: LogsPage,
  config: ConfigPage,
  env: EnvPage,
  workspaces: WorkspacesPage,
  discover: DiscoverPage,
  templates: TemplatesPage,
  git: GitPage,
  settings: SettingsPage,
};

function WorkspaceBootstrap({ children }: { children: ReactNode }) {
  const ws = useWorkspace();
  const loc = useLocation();

  if (!ws.state.bootstrapped) {
    return (
      <div className="flex flex-1 items-center justify-center text-[0.875rem] text-[var(--t3,#8a8f98)]" role="status">
        正在恢复工作区…
      </div>
    );
  }

  if (ws.state.workspaceId && (loc.pathname === "/" || loc.pathname === "/welcome")) {
    return <Navigate to="/run" replace />;
  }

  return <>{children}</>;
}

export function AppRoutes() {
  const features = useFeatures();

  return (
    <OperationProvider>
      <Routes>
        <Route element={<AppShell />}>
          <Route
            path="/welcome"
            element={
              <WorkspaceBootstrap>
                <WelcomePage />
              </WorkspaceBootstrap>
            }
          />
          {features.map((f) => {
            const Page = f.status === "soon" ? ComingSoonPage : (LIVE_PAGES[f.id] ?? ComingSoonPage);
            return (
              <Route
                key={f.id}
                path={f.path}
                element={
                  <WorkspaceBootstrap>
                    <Page />
                  </WorkspaceBootstrap>
                }
              />
            );
          })}
          <Route path="/" element={<Navigate to="/welcome" replace />} />
          <Route path="*" element={<Navigate to="/welcome" replace />} />
        </Route>
      </Routes>
    </OperationProvider>
  );
}
