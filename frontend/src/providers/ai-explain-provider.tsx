import {
  createContext,
  use,
  useCallback,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { apiAiComplete, subscribeAiStream } from "@/ipc/api";
import { IpcFailure, type AiExplainPayload } from "@/ipc/protocol";
import { errorDisplayText } from "@/lib/error-messages";
import { useToast } from "@/components/ui/toast";
import { AiExplainDialog } from "@/components/ai-explain-dialog";

type AiExplainContextValue = {
  /** 发起 explain_logs；同一时刻仅允许一个在途请求。 */
  startExplain: (payload: AiExplainPayload) => Promise<void>;
  busy: boolean;
};

const AiExplainContext = createContext<AiExplainContextValue | null>(null);

export function useAiExplain(): AiExplainContextValue {
  const ctx = use(AiExplainContext);
  if (!ctx) {
    throw new Error("useAiExplain must be used within AiExplainProvider");
  }
  return ctx;
}

/** 全局 AI 解释会话：对话框挂在此 Provider，避免 LogView 重渲染时卸载丢状态。 */
export function AiExplainProvider({ children }: { children: ReactNode }) {
  const { toast } = useToast();
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [text, setText] = useState("");
  const inflight = useRef(false);

  const startExplain = useCallback(
    async (payload: AiExplainPayload) => {
      if (inflight.current) return;
      inflight.current = true;
      const requestId = crypto.randomUUID();
      setOpen(true);
      setLoading(true);
      setText("");

      let unlisten: (() => void) | null = null;
      try {
        unlisten = await subscribeAiStream(requestId, (delta) => {
          setText((prev) => prev + delta);
          setLoading(false);
        });
        const out = await apiAiComplete("explain_logs", payload, undefined, requestId);
        setText(out.text);
        setOpen(true);
      } catch (error) {
        setOpen(false);
        const message =
          error instanceof IpcFailure ? errorDisplayText(error.code, error.message) : String(error);
        toast(message, "err");
      } finally {
        unlisten?.();
        setLoading(false);
        inflight.current = false;
      }
    },
    [toast],
  );

  return (
    <AiExplainContext value={{ startExplain, busy: loading }}>
      {children}
      <AiExplainDialog open={open} onOpenChange={setOpen} loading={loading} text={text} />
    </AiExplainContext>
  );
}
