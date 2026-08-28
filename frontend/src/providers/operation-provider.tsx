import { createContext, use, useEffect, useState, type ReactNode } from "react";
import { isTauri } from "../ipc/invoke";
import { event, type OperationEventPayload } from "../ipc/protocol";
import { mockListen } from "../ipc/mock";

/** 单条 operation 的最新状态（新事件按 operation_id 覆盖）。 */
export type OperationState = OperationEventPayload["payload"];

type OperationsContextValue = {
  /** 按首次到达顺序排列的 operation 快照 */
  operations: OperationState[];
  /** 任一 operation 处于 queued/running */
  active: boolean;
  get: (operationId: string) => OperationState | null;
};

const OperationsContext = createContext<OperationsContextValue | null>(null);

async function listenOperation(cb: (payload: OperationState) => void): Promise<() => void> {
  if (!isTauri()) {
    // 浏览器 mock：mock.ts 内的事件桥（mockEmit → mockListen）
    return mockListen(event.OPERATION, (envelope) => {
      cb((envelope as OperationEventPayload).payload);
    });
  }
  const mod = (await import("@tauri-apps/api/event")) as any;
  const listen = mod.listen as (event: string, handler: (e: any) => void) => Promise<() => void>;
  const un = await listen(event.OPERATION, (e: any) => {
    // Tauri 侧事件 payload 即完整信封，内层 payload 才是 operation 状态
    cb((e.payload as OperationEventPayload).payload);
  });
  return un;
}

export function OperationProvider({ children }: { children: ReactNode }) {
  const [ops, setOps] = useState<Map<string, OperationState>>(new Map());

  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | null = null;
    listenOperation((p) => {
      if (!alive) return;
      setOps((prev) => {
        const next = new Map(prev);
        next.set(p.operation_id, p);
        return next;
      });
    }).then((u) => {
      if (alive) unlisten = u;
      else u();
    });
    return () => {
      alive = false;
      if (unlisten) unlisten();
    };
  }, []);

  const operations = Array.from(ops.values());
  const active = operations.some((o) => o.state === "queued" || o.state === "running");

  const value: OperationsContextValue = {
    operations,
    active,
    get: (id) => ops.get(id) ?? null,
  };

  return <OperationsContext value={value}>{children}</OperationsContext>;
}

export function useOperations(): OperationsContextValue {
  const ctx = use(OperationsContext);
  if (!ctx) throw new Error("useOperations 必须在 OperationProvider 内");
  return ctx;
}

/** operation 终态后 result 里的工作区路径（后端统一写 `workspace_id`）。 */
export function operationResultWorkspaceId(op: OperationState | null): string | null {
  if (!op || op.state !== "succeeded") return null;
  const r = op.result as { workspace_id?: unknown; workspaceId?: unknown } | null;
  if (!r || typeof r !== "object") return null;
  const v = r.workspace_id ?? r.workspaceId;
  return typeof v === "string" && v ? v : null;
}
