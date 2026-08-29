import { useState } from "react";
import { useTranslation } from "react-i18next";
import { PackageOpen, PackagePlus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { useToast } from "@/components/ui/toast";
import { useWorkspace } from "../providers/workspace-provider";
import { useOpenWorkspace } from "../lib/use-open-workspace";
import { errorDisplayText } from "@/lib/error-messages";
import { isTauri } from "../ipc/invoke";
import { apiWorkspaceExportPackage, apiWorkspaceImportPackage } from "../ipc/api";
import { IpcFailure } from "../ipc/protocol";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";

/**
 * 1.7 §9.1：工作区包导出/导入卡（自设置页迁移至此；功能本体 core pkg 不变）。
 * 导出：with-secrets 需显式确认（§9.2）；导入：选包 → 选目标目录 → 只落盘 → 打开。
 * welcome 首启导入保留独立实现（onboarding 路径不动）。
 */
export function WorkspacePkgCard() {
  return (
    <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
      <ExportPkgCard />
      <ImportPkgCard />
    </div>
  );
}

/** 导出卡（原 settings ExportPkgCard 原样迁移）。 */
function ExportPkgCard() {
  const { state } = useWorkspace();
  const workspaceId = state.workspaceId;
  const { t } = useTranslation();
  const { toast } = useToast();
  const [withSecrets, setWithSecrets] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  const doExport = async (destPath: string) => {
    setBusy(true);
    try {
      const out = await apiWorkspaceExportPackage({
        workspaceId: workspaceId ?? "",
        destPath,
        withSecrets,
      });
      toast(t("pages.settings.exportDoneToast", { n: out.entries.length }), "ok");
    } catch (e) {
      toast(e instanceof IpcFailure ? errorDisplayText(e.code, e.message) : String(e), "err");
    } finally {
      setBusy(false);
    }
  };

  const onExport = async () => {
    if (!workspaceId) {
      toast(t("pages.settings.exportNoWs"), "err");
      return;
    }
    if (withSecrets) {
      setConfirmOpen(true);
      return;
    }
    if (!isTauri()) {
      const p = window.prompt(t("pages.settings.exportPathPrompt"));
      if (p) await doExport(p);
      return;
    }
    const sel = await saveDialog({
      title: t("pages.settings.exportPkg"),
      defaultPath: "supertask-export.zip",
      filters: [{ name: "SuperTask 导出包", extensions: ["zip"] }],
    });
    if (typeof sel === "string" && sel) await doExport(sel);
  };

  return (
    <Card className="p-4">
      <h3 className="mb-3 flex items-center gap-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">
        <PackageOpen className="size-4" /> {t("pages.settings.exportPkg")}
      </h3>
      <p className="mb-2 text-[0.75rem] leading-relaxed text-[var(--t3,#8a8f98)]">
        {t("pages.settings.exportPkgHint")}
      </p>
      <label className="mt-1 flex cursor-pointer items-start gap-2 text-[0.78rem] text-[var(--t2,#62666d)]">
        <input
          type="checkbox"
          className="mt-0.5 cursor-pointer"
          checked={withSecrets}
          onChange={(e) => setWithSecrets(e.target.checked)}
        />
        <span className="min-w-0">
          <span className="block">{t("pages.settings.exportWithSecrets")}</span>
          <span className="block text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.settings.exportWithSecretsDesc")}</span>
        </span>
      </label>
      <div className="mt-2">
        <Button variant="soft" size="sm" onClick={onExport} disabled={busy} className="gap-1">
          <PackageOpen /> {t("pages.settings.exportBtn")}
        </Button>
      </div>
      <ConfirmDialog
        open={confirmOpen}
        title={t("pages.settings.exportConfirmTitle")}
        description={t("pages.settings.exportConfirmDesc")}
        destructive
        onConfirm={() => {
          setConfirmOpen(false);
          if (!isTauri()) {
            const p = window.prompt(t("pages.settings.exportPathPrompt"));
            if (p) void doExport(p);
            return;
          }
          void (async () => {
            const sel = await saveDialog({
              title: t("pages.settings.exportPkg"),
              defaultPath: "supertask-export.zip",
              filters: [{ name: "SuperTask 导出包", extensions: ["zip"] }],
            });
            if (typeof sel === "string" && sel) await doExport(sel);
          })();
        }}
        onCancel={() => setConfirmOpen(false)}
      />
    </Card>
  );
}

/** 导入卡（复用 welcome 导入交互：选包 → 选目录 → 只落盘 → 打开返回 root）。 */
function ImportPkgCard() {
  const { t } = useTranslation();
  const { toast } = useToast();
  const openWs = useOpenWorkspace();
  const [busy, setBusy] = useState(false);

  const importPkg = async () => {
    setBusy(true);
    try {
      let pkgPath: string | null = null;
      if (!isTauri()) {
        pkgPath = window.prompt(t("pages.welcome.importPkgPrompt"));
      } else {
        const sel = await openDialog({
          multiple: false,
          filters: [{ name: "SuperTask 导出包", extensions: ["zip"] }],
        });
        pkgPath = typeof sel === "string" ? sel : null;
      }
      if (!pkgPath) return;
      let destDir: string | null = null;
      if (!isTauri()) {
        destDir = window.prompt(t("pages.welcome.importDestPrompt"));
      } else {
        const sel = await openDialog({ directory: true, multiple: false });
        destDir = typeof sel === "string" ? sel : null;
      }
      if (!destDir) return;
      const out = await apiWorkspaceImportPackage({ pkgPath, destDir });
      toast(t("pages.welcome.importedToast"), "ok");
      await openWs(out.root);
    } catch (e) {
      toast(e instanceof IpcFailure ? errorDisplayText(e.code, e.message) : String(e), "err");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card className="p-4">
      <h3 className="mb-3 flex items-center gap-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">
        <PackagePlus className="size-4" /> {t("pages.welcome.importPkg")}
      </h3>
      <p className="mb-2 text-[0.75rem] leading-relaxed text-[var(--t3,#8a8f98)]">
        {t("pages.welcome.importPkgHint")}
      </p>
      <div className="mt-2">
        <Button variant="soft" size="sm" onClick={() => void importPkg()} disabled={busy} className="gap-1">
          <PackagePlus /> {t("pages.welcome.importPkgBtn")}
        </Button>
      </div>
    </Card>
  );
}
