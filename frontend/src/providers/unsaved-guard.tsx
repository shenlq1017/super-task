/**
 * 统一未保存守卫（providers 约定：state/actions + 模块级 registry）。
 *
 * - 页面通过 useUnsavedEntry(id, isDirty, save) 注册当前页的脏检查与保存动作；
 * - 路由切换由 NavBlocker（useBlocker）拦截；窗口/标签页关闭由 beforeunload 兜底；
 * - confirmLeave() 供页内 Tab 切换等场景复用，返回 Promise<boolean>（true = 允许离开）；
 * - 弹窗三选：保存并离开 / 放弃保存并离开 / 取消；保存串行执行，失败留在弹窗可重试。
 *
 * Registry 用模块级 Map：beforeunload 触发时同步读取，不依赖渲染周期。
 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useBlocker } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Loader2, TriangleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export type UnsavedEntry = {
  /** 唯一 id，如 "config.raw" */
  id: string;
  isDirty: () => boolean;
  /** 保存动作；resolve true = 保存成功（内部须捕获异常并返回 false） */
  save: () => Promise<boolean>;
};

const registry = new Map<string, UnsavedEntry>();

/** 任意已注册条目存在未保存变更 */
export function hasUnsavedChanges(): boolean {
  for (const entry of registry.values()) {
    try {
      if (entry.isDirty()) return true;
    } catch {
      // isDirty 抛错按未修改处理，不阻塞离开
    }
  }
  return false;
}

type UnsavedGuardCtx = {
  register: (entry: UnsavedEntry) => () => void;
  /** 有未保存内容时弹三选确认；resolve true = 允许离开 */
  confirmLeave: () => Promise<boolean>;
};

const UnsavedGuardContext = createContext<UnsavedGuardCtx | null>(null);

export function useUnsavedGuard(): UnsavedGuardCtx {
  const ctx = useContext(UnsavedGuardContext);
  if (!ctx) throw new Error("useUnsavedGuard must be used within UnsavedGuardProvider");
  return ctx;
}

/** 页面表单/设置项接入点：挂载即注册，卸载自动注销（闭包经 ref 保持最新） */
export function useUnsavedEntry(id: string, isDirty: () => boolean, save: () => Promise<boolean>): void {
  const { register } = useUnsavedGuard();
  const ref = useRef({ isDirty, save });
  ref.current = { isDirty, save };
  useEffect(
    () =>
      register({
        id,
        isDirty: () => ref.current.isDirty(),
        save: () => ref.current.save(),
      }),
    [id, register],
  );
}

export function UnsavedGuardProvider({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);
  const pendingRef = useRef<{ resolve: (leave: boolean) => void } | null>(null);

  const register = useCallback((entry: UnsavedEntry) => {
    registry.set(entry.id, entry);
    return () => {
      if (registry.get(entry.id) === entry) registry.delete(entry.id);
    };
  }, []);

  const finish = useCallback((leave: boolean) => {
    const pending = pendingRef.current;
    pendingRef.current = null;
    setOpen(false);
    setSaving(false);
    setSaveFailed(false);
    pending?.resolve(leave);
  }, []);

  const confirmLeave = useCallback((): Promise<boolean> => {
    if (!hasUnsavedChanges()) return Promise.resolve(true);
    // 已有弹窗在等待决定：复用同一 Promise，防止重复/冲突提醒
    if (pendingRef.current) {
      return new Promise<boolean>((resolve) => {
        const prev = pendingRef.current!.resolve;
        pendingRef.current!.resolve = (v) => {
          prev(v);
          resolve(v);
        };
      });
    }
    return new Promise<boolean>((resolve) => {
      pendingRef.current = { resolve };
      setSaveFailed(false);
      setOpen(true);
    });
  }, []);

  const onSaveAndLeave = useCallback(async () => {
    if (saving) return;
    setSaving(true);
    setSaveFailed(false);
    try {
      for (const entry of [...registry.values()]) {
        if (!entry.isDirty()) continue;
        const ok = await entry.save();
        if (!ok) {
          // 保存失败（含网络/IPC 异常）：留在弹窗，可重试 / 放弃 / 取消
          setSaving(false);
          setSaveFailed(true);
          return;
        }
      }
      finish(true);
    } catch {
      setSaving(false);
      setSaveFailed(true);
    }
  }, [saving, finish]);

  // 浏览器窗口/标签页关闭与刷新兜底（Tauri 关窗由壳层 close-to-tray/退出流程接管）
  useEffect(() => {
    const onBeforeUnload = (e: BeforeUnloadEvent) => {
      if (hasUnsavedChanges()) {
        e.preventDefault();
        e.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => window.removeEventListener("beforeunload", onBeforeUnload);
  }, []);

  // Esc = 取消（留在当前页）
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        finish(false);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, finish]);

  return (
    <UnsavedGuardContext.Provider value={{ register, confirmLeave }}>
      <NavBlocker confirmLeave={confirmLeave} />
      {children}
      {open ? (
        <div
          className="fixed inset-0 z-[200] grid place-items-center bg-black/40 backdrop-blur-[1px]"
          role="dialog"
          aria-modal="true"
          aria-label={t("unsavedGuard.title")}
        >
          <div className="mx-4 w-[24rem] rounded-xl border border-[#f0d58a] bg-[var(--surface,#fff)] p-4 shadow-2xl">
            <div className="mb-1.5 flex items-center gap-2">
              <span className="grid size-7 shrink-0 place-items-center rounded-full bg-[#FDF6E3]">
                <TriangleAlert className="size-4 text-[#B7791F]" />
              </span>
              <span className="text-[0.92rem] font-semibold text-[var(--t1,#222326)]">
                {t("unsavedGuard.title")}
              </span>
            </div>
            <div className="whitespace-pre-wrap break-words pl-9 text-[0.8rem] leading-relaxed text-[var(--t2,#62666d)]">
              {t("unsavedGuard.message")}
            </div>
            {saving || saveFailed ? (
              <div
                className={cn(
                  "mt-2 pl-9 text-[0.75rem]",
                  saveFailed ? "text-[#DC2626]" : "text-[var(--t3,#8a8f98)]",
                )}
                role="status"
              >
                {saving ? t("unsavedGuard.saving") : t("unsavedGuard.saveFailed")}
              </div>
            ) : null}
            <div className="mt-4 flex justify-end gap-2">
              <Button
                variant="outline"
                size="sm"
                autoFocus
                disabled={saving}
                onClick={() => finish(false)}
              >
                {t("unsavedGuard.cancel")}
              </Button>
              <Button variant="destructive" size="sm" disabled={saving} onClick={() => finish(true)}>
                {t("unsavedGuard.discard")}
              </Button>
              <Button variant="success" size="sm" disabled={saving} onClick={() => void onSaveAndLeave()}>
                {saving ? <Loader2 className="size-3.5 animate-spin" /> : null}
                {t("unsavedGuard.saveAndLeave")}
              </Button>
            </div>
          </div>
        </div>
      ) : null}
    </UnsavedGuardContext.Provider>
  );
}

/** 路由切换拦截：pathname 变化且有未保存内容时挂起导航，等 confirmLeave 决定 */
function NavBlocker({ confirmLeave }: { confirmLeave: () => Promise<boolean> }) {
  const blocker = useBlocker(({ currentLocation, nextLocation }) => {
    return currentLocation.pathname !== nextLocation.pathname && hasUnsavedChanges();
  });
  const blockerRef = useRef(blocker);
  blockerRef.current = blocker;

  const state = blocker.state;
  useEffect(() => {
    if (state !== "blocked") return;
    let alive = true;
    void confirmLeave().then((leave) => {
      if (!alive) return;
      const b = blockerRef.current;
      if (leave) b.proceed?.();
      else b.reset?.();
    });
    return () => {
      alive = false;
    };
  }, [state, confirmLeave]);

  return null;
}
