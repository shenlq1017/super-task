import { useCallback, useEffect, useRef, useState } from "react";
import { RefreshCw, Copy, Radar, FolderOpen, Square } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { toast as toastGlobal, useToast } from "@/components/ui/toast";
import { useOpenWorkspace } from "../lib/use-open-workspace";
import { useWorkspace } from "../providers/workspace-provider";
import { apiSystemDiscover, apiSystemKillProcess } from "../ipc/api";
import { IpcFailure } from "../ipc/protocol";
import type { ForeignService } from "../ipc/protocol";

const REFRESH_MS = 30_000;

function runtimeColor(kind: string): string {
  if (kind === "java") return "#2E90FA";
  if (kind === "node") return "#27A644";
  if (kind === "python") return "#F79009";
  return "var(--t3,#8a8f98)";
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${bytes} B`;
}

/** CPU / 内存读数：首次差分采样或读取失败显示占位。 */
function MetricCell({ value, format, placeholder }: { value: number | null; format: (v: number) => string; placeholder: string }) {
  if (value == null) {
    return (
      <span title="下个刷新周期出读数，或该进程拒绝读取" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
        {placeholder}
      </span>
    );
  }
  return <span className="font-mono text-[0.75rem] text-[var(--t1,#222326)]">{format(value)}</span>;
}

function DetailField({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex gap-3 text-[0.78rem] leading-relaxed">
      <span className="w-14 shrink-0 pt-0.5 text-[var(--t3,#8a8f98)]">{label}</span>
      <span className="min-w-0 flex-1 break-all text-[var(--t1,#222326)]">{children}</span>
    </div>
  );
}

/**
 * 系统服务发现：列出本机正在监听端口的 java/node/python 等进程，
 * 展示 CPU / 内存占用，并与当前工作区 supertask.yaml 的服务端口做关联标注；
 * 支持把进程工作目录一键打开为工作区（快速切换）。
 */
export function DiscoverPage() {
  const ws = useWorkspace();
  const { toast } = useToast();
  const openWorkspace = useOpenWorkspace();
  const [items, setItems] = useState<ForeignService[]>([]);
  const [loading, setLoading] = useState(false);
  const [showOther, setShowOther] = useState(false);
  const [detail, setDetail] = useState<ForeignService | null>(null);
  const [killTarget, setKillTarget] = useState<ForeignService | null>(null);
  const timer = useRef<ReturnType<typeof setInterval> | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setItems(await apiSystemDiscover());
    } catch (e) {
      // 不再静默：读端口表失败必须让用户知道这不是「没有服务」
      const msg = e instanceof Error && e.message ? e.message : "";
      toastGlobal(`发现查询失败${msg ? `：${msg}` : ""}，稍后自动重试`, "err");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    timer.current = setInterval(() => void refresh(), REFRESH_MS);
    return () => {
      if (timer.current) clearInterval(timer.current);
    };
  }, [refresh]);

  // 当前工作区占用的端口 → 服务 id 映射（用于行级关联标注）
  const portOwner = new Map<number, string>();
  for (const [id, svc] of Object.entries(ws.state.spec?.services ?? {})) {
    if (svc.port != null) portOwner.set(svc.port, id);
  }

  const known = items.filter((s) => s.kind !== "other");
  const others = items.filter((s) => s.kind === "other");

  // 弹框里的对象要跟随刷新（进程可能已退出 / 读数更新），按 pid 重新对位
  const detailLive = detail ? (items.find((s) => s.pid === detail.pid) ?? detail) : null;
  const detailMatched = detailLive
    ? detailLive.ports.map((p) => ({ p, id: portOwner.get(p) })).filter((x) => x.id)
    : [];

  const copy = async (text: string, label: string) => {
    await navigator.clipboard?.writeText(text);
    toast(`已复制${label}`, "ok");
  };

  const openAsWorkspace = async (s: ForeignService) => {
    if (!s.cwd) return;
    setDetail(null);
    await openWorkspace(s.cwd);
  };

  /** 杀整棵进程树：core 护栏（pid≤4 / 自身 / 非监听 pid 拒绝）+ 二次确认。 */
  const confirmKill = async () => {
    const target = killTarget;
    setKillTarget(null);
    if (!target) return;
    try {
      await apiSystemKillProcess(target.pid);
      toast(`已终止 ${target.name}（PID ${target.pid}）`, "ok");
      if (detail?.pid === target.pid) setDetail(null);
    } catch (e) {
      toast(e instanceof IpcFailure ? e.message : String(e), "err");
    }
    void refresh();
  };

  const renderRow = (s: ForeignService) => {
    const matched = s.ports
      .map((p) => ({ p, id: portOwner.get(p) }))
      .filter((x) => x.id);
    return (
      <tr
        key={`${s.pid}-${s.name}`}
        onClick={() => setDetail(s)}
        title="点击查看详情"
        className={cn(
          "cursor-pointer border-b border-[var(--line,#e6e6e6)] transition-colors duration-100 last:border-0 hover:bg-[var(--surface-2,#f3f4f5)]",
          matched.length > 0 && "bg-[rgb(94_106_210_/_0.04)]",
        )}
      >
        <td className="px-4 py-2.5">
          <span className="inline-flex items-center gap-2">
            <span className="size-1.5 rounded-full" style={{ background: runtimeColor(s.kind) }} />
            <span className="font-mono text-[0.78rem] font-medium text-[var(--t1,#222326)]">{s.name}</span>
          </span>
        </td>
        <td className="px-4 py-2.5 font-mono text-[0.78rem] text-[var(--t2,#62666d)]">{s.pid}</td>
        <td className="px-4 py-2.5">
          <MetricCell value={s.cpu_percent} format={(v) => `${v.toFixed(1)}%`} placeholder="—" />
        </td>
        <td className="px-4 py-2.5">
          <MetricCell value={s.memory_bytes} format={formatBytes} placeholder="—" />
        </td>
        <td className="px-4 py-2.5">
          <span className="flex flex-wrap gap-1">
            {s.ports.slice(0, 8).map((p) => (
              <span key={p} className="rounded-full bg-[var(--surface-2,#f3f4f5)] px-1.5 py-0.5 font-mono text-[0.68rem] text-[var(--primary,#5E6AD2)]">
                {p}
              </span>
            ))}
            {s.ports.length > 8 ? (
              <span className="font-mono text-[0.68rem] text-[var(--t3,#8a8f98)]">+{s.ports.length - 8}</span>
            ) : null}
          </span>
        </td>
        <td className="max-w-[220px] px-4 py-2.5">
          {s.cwd ? (
            <span
              title={s.cmd_line ?? undefined}
              className="block truncate font-mono text-[0.72rem] text-[var(--t2,#62666d)]"
            >
              {s.cwd}
            </span>
          ) : (
            <span className="text-[0.75rem] text-[var(--t3,#8a8f98)]">—</span>
          )}
        </td>
        <td className="px-4 py-2.5">
          {matched.length > 0 ? (
            <span className="inline-flex items-center gap-1.5">
              <Badge variant="soon" className="shrink-0">↔ 工作区</Badge>
              <span className="truncate text-[0.75rem] text-[var(--st-accent-hover,#4f5ac8)]">
                {matched.map((m) => m.id).join(", ")}
              </span>
            </span>
          ) : (
            <span className="text-[0.75rem] text-[var(--t3,#8a8f98)]">—</span>
          )}
        </td>
        <td className="px-2 py-2.5 text-right">
          <span className="inline-flex items-center gap-0.5" onClick={(e) => e.stopPropagation()}>
            {s.cwd ? (
              <button
                type="button"
                title={`把 ${s.cwd} 打开为工作区`}
                onClick={() => void openAsWorkspace(s)}
                className="grid size-6 place-items-center rounded-[var(--r-sm,8px)] text-[var(--t3,#8a8f98)] transition-colors hover:bg-[rgb(0_0_0_/_0.06)] hover:text-[var(--st-accent,#5e6ad2)]"
              >
                <FolderOpen className="size-3" />
              </button>
            ) : null}
            <button
              type="button"
              title={`终止 ${s.name}（PID ${s.pid}）整棵进程树`}
              onClick={() => setKillTarget(s)}
              className="grid size-6 place-items-center rounded-[var(--r-sm,8px)] text-[var(--t3,#8a8f98)] transition-colors hover:bg-[var(--st-danger-tint,#fdecec)] hover:text-[var(--st-danger,#dc2626)]"
            >
              <Square className="size-3" />
            </button>
            <button
              type="button"
              title={`复制 PID ${s.pid}`}
              onClick={() => void copy(String(s.pid), ` PID ${s.pid}`)}
              className="grid size-6 place-items-center rounded-[var(--r-sm,8px)] text-[var(--t3,#8a8f98)] transition-colors hover:bg-[rgb(0_0_0_/_0.06)] hover:text-[var(--t1,#222326)]"
            >
              <Copy className="size-3" />
            </button>
          </span>
        </td>
      </tr>
    );
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto flex max-w-5xl flex-col gap-4">
          <div className="flex items-center justify-between gap-3">
            <div>
              <h2 className="text-[1.05rem] font-bold tracking-tight text-[var(--t1,#222326)]">发现</h2>
              <p className="mt-0.5 text-[0.78rem] text-[var(--t3,#8a8f98)]">
                本机所有 java / node / python 等开发服务的监听进程。每 {REFRESH_MS / 1000}s 自动刷新，点击行查看详情。
              </p>
            </div>
            <Button variant="outline" size="sm" onClick={() => void refresh()} disabled={loading} className="gap-1">
              <RefreshCw className={cn(loading && "animate-spin")} /> 刷新
            </Button>
          </div>

          {items.length === 0 && !loading ? (
            <div className="flex flex-col items-center gap-3 rounded-[var(--r-lg,16px)] border border-dashed border-[var(--line-strong,#d0d6e0)] p-10">
              <Radar className="size-9 text-[var(--line-strong,#d0d6e0)]" />
              <div className="text-[0.88rem] font-semibold text-[var(--t1,#222326)]">没有发现运行中的开发服务</div>
              <div className="text-[0.78rem] text-[var(--t3,#8a8f98)]">启动 Spring Boot / Node / Python 服务后会出现在这里。</div>
            </div>
          ) : (
            <>
              <Card className="overflow-hidden p-0">
                <table className="w-full border-collapse text-left">
                  <thead>
                    <tr className="border-b border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] text-[11px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
                      <th className="px-4 py-2.5 font-semibold">进程</th>
                      <th className="px-4 py-2.5 font-semibold">PID</th>
                      <th className="px-4 py-2.5 font-semibold">CPU</th>
                      <th className="px-4 py-2.5 font-semibold">内存</th>
                      <th className="px-4 py-2.5 font-semibold">监听端口</th>
                      <th className="px-4 py-2.5 font-semibold">工作目录</th>
                      <th className="px-4 py-2.5 font-semibold">工作区匹配</th>
                      <th className="px-2 py-2.5" />
                    </tr>
                  </thead>
                  <tbody>{known.map(renderRow)}</tbody>
                </table>
              </Card>

              {others.length > 0 ? (
                <Card className="p-0">
                  <button
                    type="button"
                    onClick={() => setShowOther((v) => !v)}
                    aria-expanded={showOther}
                    className="flex w-full items-center justify-between gap-3 px-4 py-2.5 text-left transition-colors hover:bg-[var(--surface-2,#f3f4f5)]"
                  >
                    <span className="text-[0.78rem] font-medium text-[var(--t1,#222326)]">
                      其他监听进程（{others.length}）
                    </span>
                    <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">
                      {showOther ? "收起" : "展开"} · 非 java/node/python/deno/bun 运行时
                    </span>
                  </button>
                  {showOther ? (
                    <table className="w-full border-collapse border-t border-[var(--line,#e6e6e6)] text-left">
                      <tbody>{others.map(renderRow)}</tbody>
                    </table>
                  ) : null}
                </Card>
              ) : null}
            </>
          )}

          <p className="text-[0.72rem] leading-relaxed text-[var(--t3,#8a8f98)]">
            说明：通过系统 TCP 表（IPv4 + IPv6）+ 进程名识别。CPU / 内存为系统采样读数
            （首次刷新只有内存，CPU 从第二个周期起显示）；点击行可查看完整命令行等详情。
            若端口与当前工作区的 supertask.yaml 匹配，打开工作区时该服务会直接显示为「外部 · 仅监控」状态。
            行内 ⏹ 可强制终止整棵进程树（taskkill /T /F，不可恢复），系统进程与 SuperTask 自身不可终止。
          </p>
        </div>
      </div>

      <ConfirmDialog
        open={killTarget != null}
        destructive
        title={`终止 ${killTarget?.name ?? ""}（PID ${killTarget?.pid ?? ""}）`}
        description={
          <>
            将 taskkill /T /F 强制终止该进程及全部子进程，未保存的数据会丢失。
            {killTarget?.cwd ? (
              <span className="mt-1 block font-mono text-[0.72rem] text-[var(--t2,#62666d)]">{killTarget.cwd}</span>
            ) : null}
          </>
        }
        confirmText="强制终止"
        onConfirm={() => void confirmKill()}
        onCancel={() => setKillTarget(null)}
      />

      <Dialog open={detailLive != null} onOpenChange={(o) => !o && setDetail(null)}>
        <DialogContent className="sm:max-w-xl">
          {detailLive ? (
            <>
              <DialogHeader>
                <DialogTitle className="flex items-center gap-2">
                  <span className="size-2 rounded-full" style={{ background: runtimeColor(detailLive.kind) }} />
                  <span className="font-mono">{detailLive.name}</span>
                  <span className="font-mono text-[0.75rem] font-normal text-[var(--t3,#8a8f98)]">
                    PID {detailLive.pid}
                  </span>
                </DialogTitle>
                <DialogDescription>
                  进程详情 · 每 {REFRESH_MS / 1000}s 随列表自动刷新读数
                </DialogDescription>
              </DialogHeader>

              <div className="flex flex-col gap-2">
                <DetailField label="运行时">{detailLive.kind}</DetailField>
                <DetailField label="CPU">
                  <MetricCell value={detailLive.cpu_percent} format={(v) => `${v.toFixed(1)}%`} placeholder="首次采样中，下个周期显示" />
                </DetailField>
                <DetailField label="内存">
                  <MetricCell value={detailLive.memory_bytes} format={formatBytes} placeholder="读取失败（可能为受保护进程）" />
                </DetailField>
                <DetailField label="监听端口">
                  <span className="flex flex-wrap gap-1">
                    {detailLive.ports.map((p) => (
                      <span key={p} className="rounded-full bg-[var(--surface-2,#f3f4f5)] px-1.5 py-0.5 font-mono text-[0.7rem] text-[var(--primary,#5E6AD2)]">
                        {p}
                      </span>
                    ))}
                  </span>
                </DetailField>
                <DetailField label="工作目录">
                  {detailLive.cwd ? (
                    <span className="inline-flex items-start gap-1.5">
                      <span className="font-mono text-[0.72rem]">{detailLive.cwd}</span>
                      <button
                        type="button"
                        title="复制路径"
                        onClick={() => void copy(detailLive.cwd!, "路径")}
                        className="shrink-0 text-[var(--t3,#8a8f98)] transition-colors hover:text-[var(--t1,#222326)]"
                      >
                        <Copy className="size-3" />
                      </button>
                    </span>
                  ) : (
                    <span className="text-[var(--t3,#8a8f98)]">—</span>
                  )}
                </DetailField>
                <DetailField label="命令行">
                  {detailLive.cmd_line ? (
                    <span className="inline-flex items-start gap-1.5">
                      <span className="max-h-24 overflow-y-auto font-mono text-[0.72rem]">{detailLive.cmd_line}</span>
                      <button
                        type="button"
                        title="复制命令行"
                        onClick={() => void copy(detailLive.cmd_line!, "命令行")}
                        className="shrink-0 text-[var(--t3,#8a8f98)] transition-colors hover:text-[var(--t1,#222326)]"
                      >
                        <Copy className="size-3" />
                      </button>
                    </span>
                  ) : (
                    <span className="text-[var(--t3,#8a8f98)]">—</span>
                  )}
                </DetailField>
                <DetailField label="工作区">
                  {detailMatched.length > 0 ? (
                    <span className="inline-flex flex-wrap items-center gap-1.5">
                      <Badge variant="soon" className="shrink-0">↔ 当前工作区</Badge>
                      <span className="text-[0.75rem] text-[var(--st-accent-hover,#4f5ac8)]">
                        {detailMatched.map((m) => `${m.id} (${m.p})`).join("、")}
                      </span>
                    </span>
                  ) : detailLive.cwd ? (
                    <span className="text-[var(--t3,#8a8f98)]">与当前工作区无端口交集</span>
                  ) : (
                    <span className="text-[var(--t3,#8a8f98)]">—</span>
                  )}
                </DetailField>
              </div>

              <DialogFooter>
                <Button variant="outline" size="sm" onClick={() => void copy(String(detailLive.pid), ` PID ${detailLive.pid}`)}>
                  复制 PID
                </Button>
                <Button
                  variant="destructive"
                  size="sm"
                  onClick={() => {
                    setDetail(null);
                    setKillTarget(detailLive);
                  }}
                >
                  终止进程树
                </Button>
                {detailLive.cwd ? (
                  <Button size="sm" className="gap-1" onClick={() => void openAsWorkspace(detailLive)}>
                    <FolderOpen /> 打开为工作区
                  </Button>
                ) : null}
              </DialogFooter>
            </>
          ) : null}
        </DialogContent>
      </Dialog>
    </div>
  );
}
