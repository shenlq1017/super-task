import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card } from "@/components/ui/card";
import { useWorkspace } from "../providers/workspace-provider";
import { useOpenWorkspace } from "../lib/use-open-workspace";
import { isTauri } from "../ipc/invoke";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FolderOpen, ScanLine, Plus, FolderSearch } from "lucide-react";

export function WelcomePage() {
  const ws = useWorkspace();
  const openWs = useOpenWorkspace();
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);

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
        const p = window.prompt("输入工作区目录路径");
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
        <h1 className="text-2xl font-bold tracking-tight text-[var(--t1,#222326)]">欢迎使用 SuperTask</h1>
        <p className="mt-1 text-[0.875rem] text-[var(--t2,#62666d)]">
          一份 <code className="rounded bg-[var(--surface-2,#f3f4f5)] px-1 font-mono text-[0.8rem]">supertask.yaml</code>
          ，可视化启停 Spring Boot 多模块 + Node，带日志与健康检查。
        </p>

        <Card className="mt-6 p-4">
          <div className="mb-2 flex items-center gap-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">
            <FolderOpen className="size-4" /> 打开已有工作区
          </div>
          <div className="flex gap-2">
            <Input
              value={path}
              onChange={(e) => setPath(e.target.value)}
              placeholder="<knife4j-root>/knife4j/knife4j-demo-openapi3"
              onKeyDown={(e) => e.key === "Enter" && openWs(path)}
            />
            <Button variant="outline" onClick={pickDirectory} disabled={busy} className="gap-1">
              <FolderSearch /> 选择目录
            </Button>
            <Button onClick={() => openWs(path)} disabled={busy}>
              打开
            </Button>
          </div>
          <div className="mt-3 flex items-center gap-2">
            <Button variant="outline" size="sm" className="gap-1" onClick={scanCreate} disabled={busy}>
              <ScanLine /> 扫描目录并生成草稿
            </Button>
            <span className="text-[0.65rem] text-[var(--t3,#8a8f98)]">无 yaml 也能扫描 Maven/Node 工程并生成可启停草稿</span>
          </div>
        </Card>

        {ws.state.recents.length > 0 ? (
          <Card className="mt-4 p-4">
            <div className="mb-2 flex items-center gap-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">
              <Plus className="size-4" /> 最近
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
