import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card } from "@/components/ui/card";
import { useToast } from "@/components/ui/toast";
import { useWorkspace } from "../providers/workspace-provider";
import { useOpenWorkspace } from "../lib/use-open-workspace";
import { isTauri } from "../ipc/invoke";
import { apiWorkspaceImportPackage } from "../ipc/api";
import { IpcFailure } from "../ipc/protocol";
import { errorDisplayText } from "@/lib/error-messages";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Cloud, FolderOpen, PackageOpen, ScanLine, Plus, FolderSearch } from "lucide-react";

export function WelcomePage() {
  const ws = useWorkspace();
  const openWs = useOpenWorkspace();
  const navigate = useNavigate();
  const { t } = useTranslation();
  const { toast } = useToast();
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);

  // 1.5 §11：从导出包导入（选包 → 选目标目录 → 只落盘 → 打开返回的 root）
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

  const scanCreate = async () => {
    if (!path.trim()) return;
    setBusy(true);
    try {
      await openWs(path.trim());
    } finally {
      setBusy(false);
    }
  };

  const pickDirectory = async () => {
    setBusy(true);
    try {
      if (!isTauri()) {
        const p = window.prompt(t("common.inputWorkspacePath"));
        if (p) await openWs(p);
        return;
      }
      const selected = await openDialog({ directory: true, multiple: false });
      if (typeof selected === "string") await openWs(selected);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col items-center overflow-auto p-8">
      <div className="w-full max-w-xl">
        <h1 className="text-2xl font-bold tracking-tight text-[var(--t1,#222326)]">{t("pages.welcome.title")}</h1>
        <p className="mt-1 text-[0.875rem] text-[var(--t2,#62666d)]">
          {t("pages.welcome.taglinePrefix")}
          <code className="rounded bg-[var(--surface-2,#f3f4f5)] px-1 font-mono text-[0.8rem]">supertask.yaml</code>
          {t("pages.welcome.taglineSuffix")}
        </p>

        <Card className="mt-6 p-4">
          <div className="mb-2 flex items-center gap-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">
            <FolderOpen className="size-4" /> {t("pages.welcome.openExisting")}
          </div>
          <div className="flex gap-2">
            <Input
              value={path}
              onChange={(e) => setPath(e.target.value)}
              placeholder="<knife4j-root>/knife4j/knife4j-demo-openapi3"
              onKeyDown={(e) => e.key === "Enter" && openWs(path)}
            />
            <Button variant="outline" onClick={pickDirectory} disabled={busy} className="gap-1">
              <FolderSearch /> {t("pages.welcome.pickDir")}
            </Button>
            <Button onClick={() => openWs(path)} disabled={busy}>
              {t("common.open")}
            </Button>
          </div>
          <div className="mt-3 flex items-center gap-2">
            <Button variant="outline" size="sm" className="gap-1" onClick={scanCreate} disabled={busy}>
              <ScanLine /> {t("pages.welcome.scanDraft")}
            </Button>
            <span className="text-[0.65rem] text-[var(--t3,#8a8f98)]">{t("pages.welcome.scanHint")}</span>
          </div>
        </Card>

        <Card className="mt-4 p-4">
          <div className="mb-1 flex items-center gap-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">
            <PackageOpen className="size-4" /> {t("pages.welcome.importPkg")}
          </div>
          <p className="mb-3 text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.welcome.importPkgHint")}</p>
          <Button variant="soft" size="sm" className="gap-1" onClick={importPkg} disabled={busy}>
            <PackageOpen /> {t("pages.welcome.importPkgBtn")}
          </Button>
        </Card>

        <Card className="mt-4 border-[rgb(94_106_210_/_0.28)] bg-[rgb(94_106_210_/_0.035)] p-4">
          <div className="mb-1 flex items-center gap-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">
            <Cloud className="size-4 text-[var(--st-accent,#5e6ad2)]" /> {t("pages.welcome.cloudRestore")}
          </div>
          <p className="mb-3 text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.welcome.cloudRestoreHint")}</p>
          <Button variant="default" size="sm" className="gap-1" onClick={() => navigate("/cloud")} disabled={busy}>
            <Cloud className="size-3.5" /> {t("pages.welcome.cloudRestoreBtn")}
          </Button>
        </Card>

        {ws.state.recents.length > 0 ? (
          <Card className="mt-4 p-4">
            <div className="mb-2 flex items-center gap-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">
              <Plus className="size-4" /> {t("workspace.recent")}
            </div>
            <div className="flex flex-col gap-1">
              {ws.state.recents.map((r) => (
                <button
                  key={r}
                  onClick={() => openWs(r)}
                  className="truncate rounded-[var(--r-sm,8px)] px-2.5 py-1.5 text-left font-mono text-[0.72rem] text-[var(--t2,#62666d)] transition-colors duration-150 hover:bg-[rgb(0_0_0_/_0.045)] hover:text-[var(--t1,#222326)]"
                  title={r}
                >
                  {r}
                </button>
              ))}
            </div>
          </Card>
        ) : null}
      </div>
    </div>
  );
}
