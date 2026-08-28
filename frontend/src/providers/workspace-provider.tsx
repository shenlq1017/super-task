import { createContext, use, useEffect, useRef, useState, type ReactNode } from "react";
import { isTauri } from "../ipc/invoke";
import { IpcFailure } from "../ipc/protocol";
import {
  apiOpenExplorer,
  apiToolchainProbe,
  apiWorkspaceClose,
  apiWorkspaceDetach,
  apiWorkspaceInit,
  apiWorkspaceOpen,
  apiWorkspaceScanDraft,
  apiYamlGet,
} from "../ipc/api";
import type { SuperTaskFile, ToolchainProbe } from "../ipc/protocol";
import {
  clearLastWorkspace,
  mergeRecents,
  readLastWorkspace,
  readRecents,
  writeLastWorkspace,
  writeRecents,
} from "../lib/workspace-storage";
import { useSession } from "./session-provider";

type WorkspaceState = {
  workspaceId: string | null;
  spec: SuperTaskFile | null;
  probe: ToolchainProbe | null;
  recents: string[];
  loading: boolean;
  error: string | null;
  warnings: string[];
  bootstrapped: boolean;
};

type WorkspaceActions = {
  open: (path: string) => Promise<void>;
  scanDraft: (path: string) => Promise<SuperTaskFile>;
  init: (path: string, spec: SuperTaskFile) => Promise<void>;
  close: () => Promise<void>;
  /** 从最近列表移除一条（本地 localStorage 维护，不涉及引擎）。 */
  removeRecent: (path: string) => void;
  openExplorer: (rel?: string) => void;
  probe: () => Promise<void>;
  refreshSpec: () => Promise<void>;
};

type WorkspaceContextValue = {
  state: WorkspaceState;
  actions: WorkspaceActions;
};

const WorkspaceContext = createContext<WorkspaceContextValue | null>(null);

/** 防止 React Strict Mode / 重复挂载时二次 restore。 */
let workspaceBootstrapDone = false;

function sameWorkspace(a: string | null, b: string): boolean {
  if (!a) return false;
  const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/$/, "").toLowerCase();
  return norm(a) === norm(b.trim());
}

async function closeEngineWorkspace(workspaceId: string | null) {
  try {
    await apiWorkspaceClose(workspaceId ?? "");
  } catch {
    // 关闭失败不阻断重新打开；open 失败时再向用户报错
  }
}

/// 切换工作区专用：移交而非终止。活进程转入后台，重开同根工作区时自动接管。
async function detachEngineWorkspace() {
  try {
    await apiWorkspaceDetach();
  } catch {
    // detach 失败不阻断切换：open 会按 ALREADY_IN_PROGRESS 兜底重试
  }
}

export function WorkspaceProvider({ children }: { children: ReactNode }) {
  const { state: session } = useSession();
  const [workspaceId, setWorkspaceId] = useState<string | null>(null);
  const [spec, setSpec] = useState<SuperTaskFile | null>(null);
  const [probe, setProbe] = useState<ToolchainProbe | null>(null);
  const [recents, setRecents] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [bootstrapped, setBootstrapped] = useState(false);

  // open/close 是异步闭包，需要拿到最新 workspaceId；ref 避免闭包读到旧值
  const workspaceIdRef = useRef<string | null>(null);
  workspaceIdRef.current = workspaceId;

  const commitWorkspace = (id: string, nextSpec: SuperTaskFile, nextWarnings: string[]) => {
    setWorkspaceId(id);
    setSpec(nextSpec);
    setWarnings(nextWarnings);
    setRecents((prev) => {
      const nextRecents = [id, ...prev.filter((x) => x !== id)].slice(0, 8);
      writeRecents(nextRecents);
      return nextRecents;
    });
    writeLastWorkspace(id);
  };

  const open = async (path: string) => {
    const target = path.trim();
    if (!target) return;

    if (sameWorkspace(workspaceId, target)) {
      try {
        const v = await apiYamlGet();
        setSpec(v.spec);
      } catch {
        /* ignore */
      }
      return;
    }

    setLoading(true);
    setError(null);
    try {
      // 切换语义：移交正在运行的服务而不是停掉它们
      await detachEngineWorkspace();
      const out = await apiWorkspaceOpen(target);
      commitWorkspace(out.workspace_id, out.spec, out.warnings);
    } catch (e) {
      if (e instanceof IpcFailure && e.code === "ALREADY_IN_PROGRESS") {
        // 引擎态与前端态可能不同步：强制关掉引擎侧残留工作区后重试一次
        await closeEngineWorkspace(null);
        const out = await apiWorkspaceOpen(target);
        commitWorkspace(out.workspace_id, out.spec, out.warnings);
        return;
      }
      const err = e instanceof IpcFailure ? e.message : String(e);
      setError(err);
      throw e;
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (!session.app || workspaceBootstrapDone) return;
    workspaceBootstrapDone = true;

    void (async () => {
      try {
        const app = session.app!;
        setProbe(app.probe);
        const merged = mergeRecents(app.recents, readRecents());
        setRecents(merged);
        writeRecents(merged);

        const restoreLast = app.prefs.restoreLast ?? true;
        const last = readLastWorkspace();
        if (restoreLast && last) {
          try {
            await open(last);
          } catch {
            clearLastWorkspace();
          }
        } else if (!isTauri() && merged.length > 0) {
          // 浏览器 mock：无 last 时回退到第一条 recent
          try {
            await open(merged[0]);
          } catch {
            /* ignore */
          }
        }
      } catch {
        /* non-fatal */
      } finally {
        // 无论 restore 是否成功、是否被 StrictMode 卸载取消，
        // “尝试恢复”这一步都已发生，必须放行路由，否则卡在恢复页。
        setBootstrapped(true);
      }
    })();

    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session.app]);

  const scanDraft = async (path: string): Promise<SuperTaskFile> => {
    setLoading(true);
    setError(null);
    try {
      const out = await apiWorkspaceScanDraft(path);
      setWarnings(out.warnings);
      return out.spec;
    } catch (e) {
      const err = e instanceof IpcFailure ? e.message : String(e);
      setError(err);
      throw e;
    } finally {
      setLoading(false);
    }
  };

  const init = async (path: string, s: SuperTaskFile) => {
    setLoading(true);
    setError(null);
    try {
      await closeEngineWorkspace(workspaceIdRef.current);
      const out = await apiWorkspaceInit(path, s);
      commitWorkspace(out.workspace_id, out.spec, out.warnings);
    } catch (e) {
      if (e instanceof IpcFailure && e.code === "ALREADY_IN_PROGRESS") {
        await closeEngineWorkspace(null);
        const out = await apiWorkspaceInit(path, s);
        commitWorkspace(out.workspace_id, out.spec, out.warnings);
        return;
      }
      const err = e instanceof IpcFailure ? e.message : String(e);
      setError(err);
      throw e;
    } finally {
      setLoading(false);
    }
  };

  const close = async () => {
    await closeEngineWorkspace(workspaceIdRef.current);
    setWorkspaceId(null);
    setSpec(null);
    setWarnings([]);
    clearLastWorkspace();
  };

  const removeRecent = (path: string) => {
    setRecents((prev) => {
      const next = prev.filter((x) => x !== path);
      writeRecents(next);
      return next;
    });
    // 移除的是恢复目标时，避免下次启动恢复到一个不在列表里的工作区
    if (readLastWorkspace() === path) clearLastWorkspace();
  };

  const openExplorer = (rel?: string) => {
    if (!workspaceId) return;
    void apiOpenExplorer(workspaceId, rel);
  };

  const probeNow = async () => {
    try {
      setProbe(await apiToolchainProbe());
    } catch {
      /* ignore */
    }
  };

  const refreshSpec = async () => {
    try {
      const v = await apiYamlGet();
      setSpec(v.spec);
    } catch {
      /* ignore */
    }
  };

  const value: WorkspaceContextValue = {
    state: { workspaceId, spec, probe, recents, loading, error, warnings, bootstrapped },
    actions: { open, scanDraft, init, close, removeRecent, openExplorer, probe: probeNow, refreshSpec },
  };

  return <WorkspaceContext value={value}>{children}</WorkspaceContext>;
}

export function useWorkspace(): WorkspaceContextValue {
  const ctx = use(WorkspaceContext);
  if (!ctx) throw new Error("useWorkspace 必须在 WorkspaceProvider 内");
  return ctx;
}

export { isTauri };
