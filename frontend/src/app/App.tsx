import { useEffect } from "react";
import { SessionProvider, useSession } from "../providers/session-provider";
import { WorkspaceProvider, useWorkspace } from "../providers/workspace-provider";
import { RuntimeProvider } from "../providers/runtime-provider";
import { LogsProvider } from "../providers/logs-provider";
import { YamlProvider } from "../providers/yaml-provider";
import { ToastProvider } from "../components/ui/toast";
import { migrateLocalRecents } from "../lib/migrate-recents";
import { AppRoutes } from "./routes";

/** app.load 完成且工作区 bootstrap 结束后，做一次 localStorage 最近工作区迁移。 */
function RecentsMigrator() {
  const { state } = useSession();
  const ws = useWorkspace();

  useEffect(() => {
    if (state.app && ws.state.bootstrapped) void migrateLocalRecents(state.app);
  }, [state.app, ws.state.bootstrapped]);

  return null;
}

function Gate() {
  const { meta, state } = useSession();
  if (!meta.ready && !state.error) {
    return (
      <p style={{ padding: 24, color: "var(--t2)" }} role="status">
        正在连接引擎…
      </p>
    );
  }
  return (
    <WorkspaceProvider>
      <RecentsMigrator />
      <RuntimeProvider>
        <LogsProvider>
          <YamlProvider>
            <AppRoutes />
          </YamlProvider>
        </LogsProvider>
      </RuntimeProvider>
    </WorkspaceProvider>
  );
}

export function App() {
  return (
    <SessionProvider>
      <ToastProvider>
        <Gate />
      </ToastProvider>
    </SessionProvider>
  );
}
