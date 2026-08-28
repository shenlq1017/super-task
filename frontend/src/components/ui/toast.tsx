import { createContext, use, useEffect, useState, type ReactNode } from "react";
import { cn } from "@/lib/utils";

type ToastKind = "ok" | "warn" | "err" | "info";
type ToastItem = { id: number; message: string; kind: ToastKind };

type ToastCtx = { toast: (message: string, kind?: ToastKind) => void };

const ToastContext = createContext<ToastCtx | null>(null);

let seq = 0;
let globalThisToast: ((m: string, k: ToastKind) => void) | null = null;

export function registerGlobalToast(fn: (m: string, k: ToastKind) => void) {
  globalThisToast = fn;
}

/** Non-hook escape hatch for event handlers / modules. */
export function toast(message: string, kind?: ToastKind) {
  if (globalThisToast) globalThisToast(message, kind ?? "info");
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);

  const toast = (message: string, kind: ToastKind = "info") => {
    const id = ++seq;
    setItems((prev) => [...prev, { id, message, kind }]);
    setTimeout(() => {
      setItems((prev) => prev.filter((t) => t.id !== id));
    }, 3200);
  };

  useEffect(() => {
    registerGlobalToast(toast);
    return () => registerGlobalToast(() => {});
  }, []);

  return (
    <ToastContext value={{ toast }}>
      {children}
      <div className="pointer-events-none fixed bottom-12 left-1/2 z-[200] flex -translate-x-1/2 flex-col items-center gap-2">
        {items.map((t) => (
          <div
            key={t.id}
            className={cn(
              "pointer-events-auto inline-flex max-w-[30rem] items-center gap-2 rounded-full px-3.5 py-2 text-[0.76rem] font-medium text-white shadow-[var(--shadow-2,0_6px_20px_rgb(16_24_40_/_0.09))] animate-in fade-in slide-in-from-bottom-2 duration-200",
              t.kind === "ok" && "bg-[var(--st-ok-deep,#1e7e35)]",
              t.kind === "warn" && "bg-[#B7791F]",
              t.kind === "err" && "bg-[var(--st-danger,#dc2626)]",
              t.kind === "info" && "bg-[var(--t1,#222326)]",
            )}
            role="status"
          >
            {t.message}
          </div>
        ))}
      </div>
    </ToastContext>
  );
}

export function useToast(): ToastCtx {
  const ctx = use(ToastContext);
  if (!ctx) throw new Error("useToast 必须在 ToastProvider 内");
  return ctx;
}
