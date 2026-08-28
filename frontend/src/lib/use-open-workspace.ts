import { useNavigate } from "react-router-dom";
import { useWorkspace } from "../providers/workspace-provider";
import { useToast } from "@/components/ui/toast";
import { IpcFailure } from "../ipc/protocol";

/**
 * Open a workspace path, auto-scanning + drafting when no `supertask.yaml`
 * exists (backend returns `NO_YAML`). Shared by the welcome page and the
 * AppShell 目录 button so the open-with-autoscan logic lives in one place.
 */
export function useOpenWorkspace() {
  const ws = useWorkspace();
  const { toast } = useToast();
  const navigate = useNavigate();

  return async (p: string) => {
    if (!p.trim()) return;
    try {
      await ws.actions.open(p.trim());
      toast("工作区已打开", "ok");
      navigate("/run");
    } catch (e) {
      if (e instanceof IpcFailure && e.code === "NO_YAML") {
        toast("未检测到 supertask.yaml，正在自动扫描目录…", "warn");
        try {
          const spec = await ws.actions.scanDraft(p.trim());
          await ws.actions.init(p.trim(), spec);
          toast("已生成草稿并打开工作区", "ok");
          navigate("/run");
        } catch (e2) {
          toast(e2 instanceof IpcFailure ? e2.message : String(e2), "err");
        }
      } else {
        toast(e instanceof IpcFailure ? e.message : String(e), "err");
      }
    }
  };
}
