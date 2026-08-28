import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useWorkspace } from "../providers/workspace-provider";
import { useToast } from "@/components/ui/toast";
import { IpcFailure } from "../ipc/protocol";
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

  return async (p: string) => {
    if (!p.trim()) return;
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
      } else {
        toast(formatIpcFailure(e), "err");
      }
    }
  };
}
