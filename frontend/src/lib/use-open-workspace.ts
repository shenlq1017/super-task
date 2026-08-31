import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useWorkspace } from "../providers/workspace-provider";
import { useToast } from "@/components/ui/toast";
import { useUnsavedGuard } from "@/providers/unsaved-guard";
import { IpcFailure } from "@/ipc/protocol";
import { formatIpcFailure } from "@/lib/error-messages";

/**
 * Open a workspace path, auto-scanning + drafting when no `supertask.yaml`
 * exists (backend returns `NO_YAML`). Shared by the welcome page and the
 * AppShell 目录 button so the open-with-autoscan logic lives in one place.
 */
export function useOpenWorkspace() {
  const ws = useWorkspace();
  const { toast } = useToast();
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { confirmLeave } = useUnsavedGuard();

  return async (p: string) => {
    if (!p.trim()) return;
    // 打开新工作区会整体替换 spec，先过未保存守卫（打开动作先于导航发生，路由 blocker 拦不住）
    if (ws.state.workspaceId && !(await confirmLeave())) return;
    try {
      await ws.actions.open(p.trim());
      toast(t("common.workspaceOpened"), "ok");
      navigate("/run");
    } catch (e) {
      if (e instanceof IpcFailure && e.code === "NO_YAML") {
        toast(t("operations.autoScanning"), "warn");
        try {
          const spec = await ws.actions.scanDraft(p.trim());
          await ws.actions.init(p.trim(), spec);
          toast(t("operations.generatedDraftAndOpened"), "ok");
          navigate("/run");
        } catch (e2) {
          toast(formatIpcFailure(e2), "err");
        }
      } else if (e instanceof IpcFailure && e.code === "WORKSPACE_LOCKED") {
        // 1.5 §11：呈现 holder/pid 与重试指引（details 由后端错误信封携带）
        const d = (e.details ?? {}) as { holder?: string; pid?: number };
        toast(
          t("errors.WORKSPACE_LOCKED_detail", { holder: d.holder ?? "?", pid: d.pid ?? "?" }),
          "err",
        );
      } else {
        toast(formatIpcFailure(e), "err");
      }
    }
  };
}
