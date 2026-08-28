import { useCallback, useEffect, useState } from "react";
import {
  Boxes,
  Container,
  Download,
  Hammer,
  Image as ImageIcon,
  Loader2,
  RefreshCw,
  Square,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { cn } from "@/lib/utils";
import { useToast } from "@/components/ui/toast";
import { useWorkspace } from "@/providers/workspace-provider";
import { useOperations } from "@/providers/operation-provider";
import {
  apiDockerBuild,
  apiDockerBuildCancel,
  apiDockerImages,
  apiDockerProbe,
  apiDockerPs,
} from "../ipc/api";
import { IpcFailure } from "../ipc/protocol";
import type {
  ContainerSummary,
  DockerProbe,
  ImageSummary,
  OpState,
} from "../ipc/protocol";
import { opErrorLabel } from "@/lib/status";

/* ---------------- helpers ---------------- */

function fmtSize(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${bytes} B`;
}

function fmtAge(ms: number): string {
  const s = Math.max(0, Math.floor((Date.now() - ms) / 1000));
  if (s < 3600) return `${Math.floor(s / 60)} 分钟前`;
  if (s < 86400) return `${Math.floor(s / 3600)} 小时前`;
  return `${Math.floor(s / 86400)} 天前`;
}

function shortId(id: string): string {
  return id.startsWith("sha256:") ? id.slice(7, 19) : id.slice(0, 12);
}

function containerStateChip(state: string) {
  const running = state === "running";
  const exited = state === "exited" || state === "dead";
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-full px-2 py-0.5 font-mono text-[10px] font-semibold",
        running && "bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]",
        exited && "bg-[var(--st-danger-tint,#fdecec)] text-[var(--st-danger,#dc2626)]",
        !running && !exited && "bg-[var(--surface-2,#f3f4f5)] text-[var(--t2,#62666d)]",
      )}
    >
      <span
        className={cn(
          "size-1.5 rounded-full",
          running ? "bg-[var(--st-ok,#27a644)]" : exited ? "bg-[var(--st-danger,#dc2626)]" : "bg-[var(--t3,#8a8f98)]",
        )}
      />
      {state}
    </span>
  );
}

function healthChip(health: string | null | undefined) {
  if (!health) return <span className="text-[0.75rem] text-[var(--t3,#8a8f98)]">—</span>;
  const cls =
    health === "healthy"
      ? "bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]"
      : health === "unhealthy"
        ? "bg-[var(--st-danger-tint,#fdecec)] text-[var(--st-danger,#dc2626)]"
        : "bg-[var(--st-warn-tint,#fff8e1)] text-[var(--st-warn,#9a6700)]";
  return (
    <span className={cn("inline-flex items-center rounded-full px-2 py-0.5 font-mono text-[10px] font-semibold", cls)}>
      {health}
    </span>
  );
}

const OP_STATE_META: Record<OpState, { label: string; cls: string }> = {
  queued: { label: "排队中", cls: "bg-[var(--surface-2,#f3f4f5)] text-[var(--t2,#62666d)]" },
  running: { label: "构建中", cls: "bg-[var(--st-warn-tint,#fff8e1)] text-[var(--st-warn,#9a6700)]" },
  succeeded: { label: "成功", cls: "bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]" },
  failed: { label: "失败", cls: "bg-[var(--st-danger-tint,#fdecec)] text-[var(--st-danger,#dc2626)]" },
  cancelled: { label: "已取消", cls: "bg-[var(--surface-2,#f3f4f5)] text-[var(--t2,#62666d)]" },
};

function SectionHead({
  icon,
  title,
  count,
  onRefresh,
  refreshing,
  disabled,
  disabledReason,
}: {
  icon: React.ReactNode;
  title: string;
  count?: number;
  onRefresh?: () => void;
  refreshing?: boolean;
  disabled?: boolean;
  disabledReason?: string;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className="flex items-center gap-1.5 text-[0.95rem] font-semibold text-[var(--t1,#222326)]">
        {icon}
        {title}
      </span>
      {count != null ? (
        <span className="font-mono text-[11px] text-[var(--t3,#8a8f98)]">{count}</span>
      ) : null}
      {disabled && disabledReason ? (
        <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">· {disabledReason}</span>
      ) : null}
      {onRefresh ? (
        <Button
          variant="soft"
          size="icon-sm"
          className="ml-auto"
          disabled={disabled || refreshing}
          onClick={onRefresh}
          aria-label={`刷新${title}`}
        >
          {refreshing ? <Loader2 className="size-3.5 animate-spin" /> : <RefreshCw className="size-3.5" />}
        </Button>
      ) : null}
    </div>
  );
}

function EmptyNote({ children }: { children: React.ReactNode }) {
  return (
    <div className="rounded-[var(--r-md,12px)] border border-dashed border-[var(--line-strong,#d0d6e0)] p-5 text-center text-[0.8rem] text-[var(--t3,#8a8f98)]">
      {children}
    </div>
  );
}

/* ---------------- page ---------------- */

/**
 * /docker 页（1.3 live）：引擎三态分态、compose 工作区、容器/镜像只读列表、
 * docker.builds 构建入口（operation 状态与取消）。up/stop 属 runtime 状态机，不在此页。
 */
export function DockerPage() {
  const ws = useWorkspace();
  const ops = useOperations();
  const { toast } = useToast();

  const [probe, setProbe] = useState<DockerProbe | null>(null);
  const [probeLoading, setProbeLoading] = useState(true);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [containers, setContainers] = useState<ContainerSummary[] | null>(null);
  const [images, setImages] = useState<ImageSummary[] | null>(null);
  const [psLoading, setPsLoading] = useState(false);
  const [imagesLoading, setImagesLoading] = useState(false);
  const [listError, setListError] = useState<string | null>(null);
  /** 本会话触发的构建：build name → operation_id（用于从 operation 流对位状态） */
  const [buildOps, setBuildOps] = useState<Record<string, string>>({});

  const spec = ws.state.spec;
  const dockerSpec = spec?.docker ?? null;
  const workspaceId = ws.state.workspaceId;
  const composeServices = Object.entries(spec?.services ?? {})
    .filter(([, s]) => s.kind === "compose")
    .map(([id, s]) => ({
      id,
      service: s.service ?? id,
      port: s.port ?? null,
      build: s.labels?.["supertask.docker.build"] === "true",
    }));

  const engineOnline = probe?.found === true && probe.running === true;
  const composeReady = engineOnline && probe?.compose_version != null;

  const loadProbe = useCallback(async (refresh: boolean) => {
    setProbeLoading(true);
    setProbeError(null);
    try {
      setProbe(await apiDockerProbe(refresh));
    } catch (e) {
      setProbe(null);
      setProbeError(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e));
    } finally {
      setProbeLoading(false);
    }
  }, []);

  const loadContainers = useCallback(async () => {
    if (!workspaceId) return;
    setPsLoading(true);
    setListError(null);
    try {
      setContainers((await apiDockerPs(workspaceId)).containers);
    } catch (e) {
      setContainers(null);
      setListError(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e));
    } finally {
      setPsLoading(false);
    }
  }, [workspaceId]);

  const loadImages = useCallback(async () => {
    setImagesLoading(true);
    setListError(null);
    try {
      setImages((await apiDockerImages()).images);
    } catch (e) {
      setImages(null);
      setListError(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e));
    } finally {
      setImagesLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadProbe(false);
  }, [loadProbe]);

  useEffect(() => {
    if (!engineOnline) return;
    void loadContainers();
    void loadImages();
  }, [engineOnline, loadContainers, loadImages]);

  const composeFile = dockerSpec?.compose_file ?? null;
  const composePath = composeFile && spec ? `${spec.root.replace(/[\\/]+$/, "")}\\${composeFile}` : null;

  const importFromCompose = async () => {
    // 1.3 §7：compose 导入走 1.1 scanPreview/scanApply 同一向导；
    // 现有扫描入口即顶部「扫描」（scanDraft → 重新生成草稿）。
    if (!workspaceId) return;
    try {
      await ws.actions.scanDraft(workspaceId);
      await ws.actions.refreshSpec();
      toast("已重新扫描：请在扫描结果中确认 compose 服务导入", "ok");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  const triggerBuild = async (name: string) => {
    if (!workspaceId) return;
    try {
      const out = await apiDockerBuild(workspaceId, name);
      setBuildOps((prev) => ({ ...prev, [name]: out.operation_id }));
      toast(`已开始构建镜像 ${name}`, "info");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  const cancelBuild = async (name: string) => {
    const opId = buildOps[name];
    if (!workspaceId || !opId) return;
    try {
      await apiDockerBuildCancel(workspaceId, opId);
      toast("已发送取消请求（已提交的层缓存保留）", "info");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  const builds = dockerSpec?.builds ?? [];
  const offlineReason = !probe
    ? probeLoading
      ? "正在探测 Docker 引擎…"
      : "Docker 引擎状态未知"
    : !probe.found
      ? "未检测到 Docker"
      : !probe.running
        ? "Docker 引擎未运行"
        : probe.compose_version == null
          ? "docker compose 插件不可用"
          : null;

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto flex max-w-[62rem] flex-col gap-4 p-5">
        {/* ================= 引擎状态卡 ================= */}
        <Card className="p-4">
          <div className="flex items-start gap-3">
            <span
              className={cn(
                "grid size-9 shrink-0 place-items-center rounded-[var(--r-md,12px)]",
                engineOnline
                  ? "bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]"
                  : "bg-[var(--st-warn-tint,#fff8e1)] text-[var(--st-warn,#9a6700)]",
              )}
            >
              <Container className="size-4.5" />
            </span>
            <div className="min-w-0 flex-1">
              {probeLoading && !probe ? (
                <div className="flex items-center gap-2 text-[0.85rem] text-[var(--t2,#62666d)]">
                  <Loader2 className="size-3.5 animate-spin" /> 正在探测 Docker 引擎…
                </div>
              ) : probeError ? (
                <>
                  <div className="text-[0.95rem] font-semibold text-[var(--t1,#222326)]">Docker 引擎状态未知</div>
                  <p className="mt-0.5 text-[0.78rem] text-[var(--st-danger,#dc2626)]">{probeError}</p>
                </>
              ) : probe && !probe.found ? (
                <>
                  <div className="text-[0.95rem] font-semibold text-[var(--t1,#222326)]">未检测到 Docker</div>
                  <p className="mt-0.5 max-w-lg text-[0.78rem] leading-relaxed text-[var(--t2,#62666d)]">
                    PATH 中没有 docker 可执行文件。请安装 Docker Desktop 后重试；SuperTask 不提供代装（工具链安装仅覆盖 JDK / Maven / Node）。
                  </p>
                  <a
                    href="https://www.docker.com/products/docker-desktop/"
                    target="_blank"
                    rel="noreferrer"
                    className="mt-1 inline-flex items-center gap-1 text-[0.78rem] font-medium text-[var(--st-accent,#5e6ad2)] underline-offset-4 hover:underline"
                  >
                    打开 Docker Desktop 下载页 <Download className="size-3.5" />
                  </a>
                </>
              ) : probe && !probe.running ? (
                <>
                  <div className="text-[0.95rem] font-semibold text-[var(--t1,#222326)]">Docker 引擎未运行</div>
                  <p className="mt-0.5 text-[0.78rem] text-[var(--t2,#62666d)]">
                    已检测到 docker CLI{probe.version ? `（${probe.version}）` : ""}，但 daemon 不可达。请启动 Docker Desktop 后重试。
                  </p>
                </>
              ) : probe ? (
                <>
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-[0.95rem] font-semibold text-[var(--t1,#222326)]">Docker 引擎在线</span>
                    <span className="inline-flex items-center gap-1 rounded-full bg-[var(--st-ok-tint,#e9f7ed)] px-2 py-0.5 text-[11px] font-medium text-[var(--st-ok-deep,#1e7e35)]">
                      <span className="size-1.5 rounded-full bg-[var(--st-ok,#27a644)]" /> 运行中
                    </span>
                    {probe.version ? <Badge variant="outline" className="font-mono text-[10px]">docker {probe.version}</Badge> : null}
                    {probe.compose_version ? (
                      <Badge variant="outline" className="font-mono text-[10px]">compose {probe.compose_version}</Badge>
                    ) : (
                      <span className="rounded-full bg-[var(--st-warn-tint,#fff8e1)] px-2 py-0.5 text-[11px] font-medium text-[var(--st-warn,#9a6700)]">
                        compose 插件不可用
                      </span>
                    )}
                  </div>
                </>
              ) : null}
            </div>
            <Button
              variant="soft"
              size="sm"
              className="shrink-0 gap-1"
              disabled={probeLoading}
              onClick={() => void loadProbe(true)}
            >
              {probeLoading ? <Loader2 className="size-3.5 animate-spin" /> : <RefreshCw className="size-3.5" />}
              重试探测
            </Button>
          </div>
        </Card>

        {/* ================= 工作区卡（compose 服务） ================= */}
        <Card className="p-4">
          <SectionHead
            icon={<Boxes className="size-4 text-[var(--st-accent,#5e6ad2)]" />}
            title="工作区 compose"
            count={composeServices.length}
          />
          {!workspaceId ? (
            <div className="mt-3">
              <EmptyNote>未打开工作区：先在顶部选择或扫描一个工作区。</EmptyNote>
            </div>
          ) : !composeFile ? (
            <div className="mt-3">
              <EmptyNote>未配置 docker.compose_file；可将存量 compose 项目走扫描向导导入。</EmptyNote>
            </div>
          ) : (
            <>
              <div className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-1 text-[0.78rem]">
                <span className="text-[var(--t3,#8a8f98)]">compose 文件</span>
                <span className="font-mono text-[var(--t1,#222326)]" title={composePath ?? composeFile}>
                  {composeFile}
                </span>
                {dockerSpec?.project_name ? (
                  <>
                    <span className="text-[var(--t3,#8a8f98)]">·</span>
                    <span className="text-[var(--t3,#8a8f98)]">project</span>
                    <span className="font-mono text-[var(--t1,#222326)]">{dockerSpec.project_name}</span>
                  </>
                ) : null}
                <Button
                  variant="outline"
                  size="sm"
                  className="ml-auto gap-1"
                  disabled={!composeReady}
                  title={composeReady ? "扫描工作区并生成 compose 服务草稿（与顶部「扫描」同一入口）" : offlineReason ?? undefined}
                  onClick={() => void importFromCompose()}
                >
                  <Download className="size-3.5" /> 从 compose 导入
                </Button>
              </div>
              {composeServices.length === 0 ? (
                <div className="mt-3">
                  <EmptyNote>compose 文件已配置，但 supertask.yaml 还没有 kind: compose 服务。</EmptyNote>
                </div>
              ) : (
                <div className="mt-3 overflow-x-auto">
                  <div className="min-w-[26rem]">
                    <div className="flex items-center gap-3 border-b border-[var(--line,#e6e6e6)] pb-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
                      <span className="w-[10rem]">服务（YAML id）</span>
                      <span className="w-[8rem]">compose 服务</span>
                      <span className="w-[6rem]">主机端口</span>
                      <span>build</span>
                    </div>
                    {composeServices.map((s) => (
                      <div
                        key={s.id}
                        className="flex items-center gap-3 border-b border-[var(--line,#e6e6e6)] py-1.5 text-[0.8rem] last:border-b-0"
                      >
                        <span className="w-[10rem] truncate font-medium text-[var(--t1,#222326)]" title={s.id}>
                          {s.id}
                        </span>
                        <span className="w-[8rem] truncate font-mono text-[var(--t2,#62666d)]" title={s.service}>
                          {s.service}
                        </span>
                        <span className="w-[6rem] font-mono text-[var(--t2,#62666d)]">{s.port ?? "—"}</span>
                        {s.build ? (
                          <Badge variant="outline" className="font-mono text-[10px] uppercase">build</Badge>
                        ) : (
                          <span className="text-[var(--t3,#8a8f98)]">—</span>
                        )}
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </>
          )}
        </Card>

        {/* ================= 容器列表（docker.ps，只读） ================= */}
        <Card className="p-4">
          <SectionHead
            icon={<Container className="size-4 text-[var(--st-accent,#5e6ad2)]" />}
            title="容器"
            count={containers?.length}
            onRefresh={() => void loadContainers()}
            refreshing={psLoading}
            disabled={!composeReady}
            disabledReason={offlineReason ?? undefined}
          />
          <div className="mt-3">
            {!composeReady ? (
              <EmptyNote>{offlineReason}</EmptyNote>
            ) : psLoading && containers == null ? (
              <div className="flex items-center gap-2 p-3 text-[0.8rem] text-[var(--t2,#62666d)]">
                <Loader2 className="size-3.5 animate-spin" /> 正在查询 docker compose ps…
              </div>
            ) : containers && containers.length === 0 ? (
              <EmptyNote>当前 compose project 没有容器（尚未启动或已清理）。</EmptyNote>
            ) : containers ? (
              <div className="overflow-x-auto">
                <div className="min-w-[40rem]">
                  <div className="flex items-center gap-3 border-b border-[var(--line,#e6e6e6)] pb-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
                    <span className="w-[7rem]">服务</span>
                    <span className="w-[8rem]">容器 ID</span>
                    <span className="min-w-[10rem] flex-1">镜像</span>
                    <span className="w-[5.5rem]">状态</span>
                    <span className="w-[5.5rem]">健康</span>
                    <span className="w-[7rem]">端口</span>
                  </div>
                  {containers.map((c) => (
                    <div
                      key={`${c.service}-${c.container_id}`}
                      className="flex items-center gap-3 border-b border-[var(--line,#e6e6e6)] py-1.5 text-[0.8rem] last:border-b-0"
                    >
                      <span className="w-[7rem] truncate font-medium text-[var(--t1,#222326)]" title={c.service}>
                        {c.service}
                      </span>
                      <span className="w-[8rem] truncate font-mono text-[var(--t2,#62666d)]" title={c.container_id}>
                        {shortId(c.container_id)}
                      </span>
                      <span className="min-w-0 flex-1 truncate font-mono text-[var(--t2,#62666d)]" title={c.image}>
                        {c.image}
                      </span>
                      <span className="w-[5.5rem]">{containerStateChip(c.state)}</span>
                      <span className="w-[5.5rem]">{healthChip(c.health)}</span>
                      <span className="w-[7rem] truncate font-mono text-[var(--t2,#62666d)]" title={c.ports.join(", ")}>
                        {c.ports.length ? c.ports.join(", ") : "—"}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            ) : listError ? (
              <p className="text-[0.78rem] text-[var(--st-danger,#dc2626)]">{listError}</p>
            ) : null}
          </div>
        </Card>

        {/* ================= 镜像列表（docker.images，只读，不提供删除） ================= */}
        <Card className="p-4">
          <SectionHead
            icon={<ImageIcon className="size-4 text-[var(--st-accent,#5e6ad2)]" />}
            title="镜像"
            count={images?.length}
            onRefresh={() => void loadImages()}
            refreshing={imagesLoading}
            disabled={!engineOnline}
            disabledReason={offlineReason ?? undefined}
          />
          <div className="mt-3">
            {!engineOnline ? (
              <EmptyNote>{offlineReason}</EmptyNote>
            ) : imagesLoading && images == null ? (
              <div className="flex items-center gap-2 p-3 text-[0.8rem] text-[var(--t2,#62666d)]">
                <Loader2 className="size-3.5 animate-spin" /> 正在查询本机镜像…
              </div>
            ) : images && images.length === 0 ? (
              <EmptyNote>本机没有镜像。可在下方触发构建，或在 compose 首次 up 时自动拉取。</EmptyNote>
            ) : images ? (
              <div className="overflow-x-auto">
                <div className="min-w-[34rem]">
                  <div className="flex items-center gap-3 border-b border-[var(--line,#e6e6e6)] pb-1.5 text-[10px] font-semibold tracking-wider text-[var(--t3,#8a8f98)]">
                    <span className="min-w-[12rem] flex-1">仓库:标签</span>
                    <span className="w-[10rem]">镜像 ID</span>
                    <span className="w-[6rem]">大小</span>
                    <span className="w-[7rem]">创建时间</span>
                  </div>
                  {images.map((img) => (
                    <div
                      key={`${img.repository}:${img.tag}-${img.id}`}
                      className="flex items-center gap-3 border-b border-[var(--line,#e6e6e6)] py-1.5 text-[0.8rem] last:border-b-0"
                    >
                      <span className="min-w-0 flex-1 truncate font-mono font-medium text-[var(--t1,#222326)]" title={`${img.repository}:${img.tag}`}>
                        {img.repository}:{img.tag}
                      </span>
                      <span className="w-[10rem] truncate font-mono text-[var(--t2,#62666d)]" title={img.id}>
                        {shortId(img.id)}
                      </span>
                      <span className="w-[6rem] font-mono text-[var(--t2,#62666d)]">
                        {img.size_bytes != null ? fmtSize(img.size_bytes) : "—"}
                      </span>
                      <span className="w-[7rem] text-[var(--t3,#8a8f98)]">
                        {img.created_ms != null ? fmtAge(img.created_ms) : "—"}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            ) : listError ? (
              <p className="text-[0.78rem] text-[var(--st-danger,#dc2626)]">{listError}</p>
            ) : null}
          </div>
        </Card>

        {/* ================= 构建入口（docker.builds 条目，走 operation） ================= */}
        <Card className="p-4">
          <SectionHead
            icon={<Hammer className="size-4 text-[var(--st-accent,#5e6ad2)]" />}
            title="镜像构建"
            count={builds.length}
            disabled={!composeReady}
            disabledReason={offlineReason ?? undefined}
          />
          <div className="mt-3">
            {!composeReady ? (
              <EmptyNote>{offlineReason}</EmptyNote>
            ) : builds.length === 0 ? (
              <EmptyNote>
                supertask.yaml 未配置 docker.builds。compose 服务内有 build: 段的，可在运行页该服务处触发构建。
              </EmptyNote>
            ) : (
              <div className="flex flex-col">
                {builds.map((b) => {
                  const opId = buildOps[b.name];
                  const op = opId ? ops.get(opId) : null;
                  const opActive = op?.state === "queued" || op?.state === "running";
                  return (
                    <div
                      key={b.name}
                      className="flex flex-col gap-1.5 border-b border-[var(--line,#e6e6e6)] py-2.5 last:border-b-0"
                    >
                      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                        <span className="font-mono text-[0.85rem] font-semibold text-[var(--t1,#222326)]">{b.name}</span>
                        <span className="font-mono text-[0.72rem] text-[var(--t2,#62666d)]" title={b.tags.join(", ")}>
                          {b.tags.join(", ")}
                        </span>
                        <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]" title={`${b.context}${b.dockerfile ? ` · ${b.dockerfile}` : ""}`}>
                          {b.context}
                          {b.dockerfile ? ` · ${b.dockerfile}` : ""}
                        </span>
                        <div className="ml-auto flex items-center gap-1.5">
                          {op ? (
                            <span
                              className={cn(
                                "inline-flex items-center rounded-full px-2 py-0.5 text-[11px] font-semibold",
                                OP_STATE_META[op.state].cls,
                              )}
                              title={op.message ?? undefined}
                            >
                              {opActive ? <Loader2 className="mr-1 size-3 animate-spin" /> : null}
                              {OP_STATE_META[op.state].label}
                            </span>
                          ) : null}
                          {opActive ? (
                            <Button variant="destructive" size="sm" className="gap-1" onClick={() => void cancelBuild(b.name)}>
                              <Square className="size-3.5" /> 取消构建
                            </Button>
                          ) : (
                            <Button
                              variant="secondary"
                              size="sm"
                              className="gap-1"
                              disabled={!workspaceId}
                              onClick={() => void triggerBuild(b.name)}
                            >
                              <Hammer className="size-3.5" /> 构建镜像
                            </Button>
                          )}
                        </div>
                      </div>
                      {op?.state === "failed" ? (
                        <p className="break-all text-[0.72rem] text-[var(--st-danger,#dc2626)]" title={op.message ?? undefined}>
                          {op.error_code ? `${op.error_code}：` : ""}
                          {op.message ?? "构建失败"}
                        </p>
                      ) : op?.state === "succeeded" && op.message ? (
                        <p className="break-all text-[0.72rem] text-[var(--t3,#8a8f98)]">{op.message}</p>
                      ) : null}
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </Card>
      </div>
    </div>
  );
}
