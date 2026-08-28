import { CheckCircle2, XCircle, Wrench } from "lucide-react";
import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { useSession } from "../providers/session-provider";
import { cn } from "@/lib/utils";

export function EnvPage() {
  const { state } = useSession();
  const probe = state.app?.probe;

  const tools = probe
    ? [
        { name: "JDK", found: probe.java, rec: "17 LTS" },
        { name: "Maven", found: probe.maven, rec: "3.9+" },
        { name: "Node", found: probe.node, rec: "20 LTS" },
        { name: "npm", found: probe.npm, rec: "10+" },
        { name: "pnpm", found: probe.pnpm, rec: "可选" },
        { name: "Yarn", found: probe.yarn, rec: "可选" },
      ]
    : [];

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto p-4">
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {tools.map((t) => (
            <Card key={t.name} className="flex items-center gap-3 p-3 transition-colors duration-150 hover:border-[var(--line-strong,#d0d6e0)]">
              <div
                className={cn(
                  "flex size-9 items-center justify-center rounded-[var(--r-sm,8px)]",
                  t.found.found ? "bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]" : "bg-[var(--st-warn-tint,#fff8e1)] text-[var(--st-warn,#9a6700)]",
                )}
              >
                {t.found.found ? <CheckCircle2 className="size-5" /> : <XCircle className="size-5" />}
              </div>
              <div className="min-w-0">
                <div className="font-semibold text-[var(--t1,#222326)]">{t.name}</div>
                <div className="truncate font-mono text-[0.66rem] text-[var(--t3,#8a8f98)]">
                  {t.found.found ? `${t.found.version ?? "已安装"} · ${t.found.path ?? ""}` : `缺失 · 建议 ${t.rec}`}
                </div>
              </div>
            </Card>
          ))}
        </div>

        <div className="mt-6 rounded-[var(--r-lg,16px)] border border-dashed border-[var(--line-strong,#d0d6e0)] p-6">
          <div className="flex items-center gap-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">
            <Wrench className="size-4 text-[var(--t3,#8a8f98)]" /> 一键安装区
          </div>
          <p className="mt-2 text-[0.875rem] text-[var(--t2,#62666d)]">
            按路线版本，<span className="font-semibold">1.2</span> 接入 mise / winget 实现缺失工具链的安装与管理。当前版本仅探测、不安装、不展示假数据。
          </p>
          <Badge variant="soon" className="mt-3">即将 1.2</Badge>
        </div>
      </div>
    </div>
  );
}
