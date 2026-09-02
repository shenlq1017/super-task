import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
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
import i18n from "@/i18n";

/* ---------------- helpers ---------------- */

function fmtSize(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${bytes} B`;
}

function fmtAge(ms: number): string {
  const s = Math.max(0, Math.floor((Date.now() - ms) / 1000));
  if (s < 3600) return i18n.t("pages.docker.ageMinutes", { n: Math.floor(s / 60) });
  if (s < 86400) return i18n.t("pages.docker.ageHours", { n: Math.floor(s / 3600) });
  return i18n.t("pages.docker.ageDays", { n: Math.floor(s / 86400) });
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
        "inline-flex h-5 items-center gap-1 rounded-full px-2 font-mono text-[10px] font-semibold leading-none",
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
    <span className={cn("inline-flex h-5 items-center rounded-full px-2 font-mono text-[10px] font-semibold leading-none", cls)}>
      {health}
    </span>
  );
}

/** operation 状态 chip 文案走 `opStates.*`；配色本地保留。 */
const OP_STATE_META: Record<OpState, { cls: string }> = {
  queued: { cls: "bg-[var(--surface-2,#f3f4f5)] text-[var(--t2,#62666d)]" },
  running: { cls: "bg-[var(--st-warn-tint,#fff8e1)] text-[var(--st-warn,#9a6700)]" },
  succeeded: { cls: "bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]" },
  failed: { cls: "bg-[var(--st-danger-tint,#fdecec)] text-[var(--st-danger,#dc2626)]" },
  cancelled: { cls: "bg-[var(--surface-2,#f3f4f5)] text-[var(--t2,#62666d)]" },
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
  const { t } = useTranslation();
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
          aria-label={t("pages.docker.refreshAria", { title })}
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
  const { t } = useTranslation();

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
      toast(t("pages.docker.rescanImported"), "ok");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  const triggerBuild = async (name: string) => {
    if (!workspaceId) return;
    try {
      const out = await apiDockerBuild(workspaceId, name);
      setBuildOps((prev) => ({ ...prev, [name]: out.operation_id }));
      toast(t("pages.docker.buildStarted", { name }), "info");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  const cancelBuild = async (name: string) => {
    const opId = buildOps[name];
    if (!workspaceId || !opId) return;
    try {
      await apiDockerBuildCancel(workspaceId, opId);
      toast(t("pages.docker.cancelSent"), "info");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  const builds = dockerSpec?.builds ?? [];
  const offlineReason = !probe
    ? probeLoading
      ? t("pages.docker.probingEngine")
      : t("pages.docker.engineUnknown")
    : !probe.found
      ? t("pages.docker.notFound")
      : !probe.running
        ? t("pages.docker.engineDown")
        : probe.compose_version == null
          ? t("pages.docker.composeMissing")
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
                  <Loader2 className="size-3.5 animate-spin" /> {t("pages.docker.probingEngine")}
                </div>
              ) : probeError ? (
                <>
                  <div className="text-[0.95rem] font-semibold text-[var(--t1,#222326)]">{t("pages.docker.engineUnknown")}</div>
                  <p className="mt-0.5 text-[0.78rem] text-[var(--st-danger,#dc2626)]">{probeError}</p>
                </>
              ) : probe && !probe.found ? (
                <>
                  <div className="text-[0.95rem] font-semibold text-[var(--t1,#222326)]">{t("pages.docker.notFound")}</div>
                  <p className="mt-0.5 max-w-lg text-[0.78rem] leading-relaxed text-[var(--t2,#62666d)]">
                    {t("pages.docker.notFoundDesc")}
                  </p>
                  <a
                    href="https://www.docker.com/products/docker-desktop/"
                    target="_blank"
                    rel="noreferrer"
                    className="mt-1 inline-flex items-center gap-1 text-[0.78rem] font-medium text-[var(--st-accent,#5e6ad2)] underline-offset-4 hover:underline"
                  >
                    {t("pages.docker.downloadLink")} <Download className="size-3.5" />
                  </a>
                </>
              ) : probe && !probe.running ? (
                <>
                  <div className="text-[0.95rem] font-semibold text-[var(--t1,#222326)]">{t("pages.docker.engineDown")}</div>
                  <p className="mt-0.5 text-[0.78rem] text-[var(--t2,#62666d)]">
                    {t("pages.docker.engineDownDesc", { version: probe.version ?? "" })}
                  </p>
                </>
              ) : probe ? (
                <>
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-[0.95rem] font-semibold text-[var(--t1,#222326)]">{t("pages.docker.engineOnline")}</span>
                    <span className="inline-flex h-5 items-center gap-1 rounded-full bg-[var(--st-ok-tint,#e9f7ed)] px-2 text-[11px] font-medium leading-none text-[var(--st-ok-deep,#1e7e35)]">
                      <span className="size-1.5 rounded-full bg-[var(--st-ok,#27a644)]" /> {t("states.running")}
                    </span>
                    {probe.version ? <Badge variant="outline" className="font-mono text-[10px]">docker {probe.version}</Badge> : null}
                    {probe.compose_version ? (
                      <Badge variant="outline" className="font-mono text-[10px]">compose {probe.compose_version}</Badge>
                    ) : (
                      <span className="inline-flex h-5 items-center rounded-full bg-[var(--st-warn-tint,#fff8e1)] px-2 text-[11px] font-medium leading-none text-[var(--st-warn,#9a6700)]">
                        {t("pages.docker.composeMissing")}
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
              {t("pages.docker.retryProbe")}
            </Button>
          </div>
        </Card>

        {/* ================= 工作区卡（compose 服务） ================= */}
        <Card className="p-4">
          <SectionHead
            icon={<Boxes className="size-4 text-[var(--st-accent,#5e6ad2)]" />}
            title={t("pages.docker.wsCompose")}
            count={composeServices.length}
          />
          {!workspaceId ? (
            <div className="mt-3">
              <EmptyNote>{t("pages.docker.noWorkspace")}</EmptyNote>
            </div>
          ) : !composeFile ? (
            <div className="mt-3">
              <EmptyNote>{t("pages.docker.noComposeFile")}</EmptyNote>
            </div>
          ) : (
            <>
              <div className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-1 text-[0.78rem]">
                <span className="text-[var(--t3,#8a8f98)]">{t("pages.docker.composeFile")}</span>
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
                  title={composeReady ? t("pages.docker.importTitle") : offlineReason ?? undefined}
                  onClick={() => void importFromCompose()}
                >
                  <Download className="size-3.5" /> {t("pages.docker.importFromCompose")}
                </Button>
              </div>
              {composeServices.length === 0 ? (
                <div className="mt-3">
                  <EmptyNote>{t("pages.docker.noComposeServices")}</EmptyNote>
                </div>
              ) : (
                <div className="mt-3 overflow-x-auto">
                  <div className="min-w-[26rem]">
                    <div className="flex items-center gap-3 border-b border-[var(--line,#e6e6e6)] pb-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
                      <span className="w-[10rem]">{t("pages.docker.colServiceYaml")}</span>
                      <span className="w-[8rem]">{t("pages.docker.colComposeService")}</span>
                      <span className="w-[6rem]">{t("pages.docker.colHostPort")}</span>
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
            title={t("pages.docker.containers")}
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
                <Loader2 className="size-3.5 animate-spin" /> {t("pages.docker.queryingPs")}
              </div>
            ) : containers && containers.length === 0 ? (
              <EmptyNote>{t("pages.docker.noContainers")}</EmptyNote>
            ) : containers ? (
              <div className="overflow-x-auto">
                <div className="min-w-[40rem]">
                  <div className="flex items-center gap-3 border-b border-[var(--line,#e6e6e6)] pb-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
                    <span className="w-[7rem]">{t("pages.docker.colService")}</span>
                    <span className="w-[8rem]">{t("pages.docker.colContainerId")}</span>
                    <span className="min-w-[10rem] flex-1">{t("pages.docker.colImage")}</span>
                    <span className="w-[5.5rem]">{t("pages.docker.colState")}</span>
                    <span className="w-[5.5rem]">{t("pages.docker.colHealth")}</span>
                    <span className="w-[7rem]">{t("pages.docker.colPorts")}</span>
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
            title={t("pages.docker.images")}
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
                <Loader2 className="size-3.5 animate-spin" /> {t("pages.docker.queryingImages")}
              </div>
            ) : images && images.length === 0 ? (
              <EmptyNote>{t("pages.docker.noImages")}</EmptyNote>
            ) : images ? (
              <div className="overflow-x-auto">
                <div className="min-w-[34rem]">
                  <div className="flex items-center gap-3 border-b border-[var(--line,#e6e6e6)] pb-1.5 text-[10px] font-semibold tracking-wider text-[var(--t3,#8a8f98)]">
                    <span className="min-w-[12rem] flex-1">{t("pages.docker.colRepoTag")}</span>
                    <span className="w-[10rem]">{t("pages.docker.colImageId")}</span>
                    <span className="w-[6rem]">{t("pages.docker.colSize")}</span>
                    <span className="w-[7rem]">{t("pages.docker.colCreated")}</span>
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
            title={t("pages.docker.imageBuild")}
            count={builds.length}
            disabled={!composeReady}
            disabledReason={offlineReason ?? undefined}
          />
          <div className="mt-3">
            {!composeReady ? (
              <EmptyNote>{offlineReason}</EmptyNote>
            ) : builds.length === 0 ? (
              <EmptyNote>
                {t("pages.docker.noBuilds")}
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
                                "inline-flex h-5 items-center rounded-full px-2 text-[11px] font-semibold leading-none",
                                OP_STATE_META[op.state].cls,
                              )}
                              title={op.message ?? undefined}
                            >
                              {opActive ? <Loader2 className="mr-1 size-3 animate-spin" /> : null}
                              {t(`opStates.${op.state}`)}
                            </span>
                          ) : null}
                          {opActive ? (
                            <Button variant="destructive" size="sm" className="gap-1" onClick={() => void cancelBuild(b.name)}>
                              <Square className="size-3.5" /> {t("pages.docker.cancelBuild")}
                            </Button>
                          ) : (
                            <Button
                              variant="secondary"
                              size="sm"
                              className="gap-1"
                              disabled={!workspaceId}
                              onClick={() => void triggerBuild(b.name)}
                            >
                              <Hammer className="size-3.5" /> {t("pages.docker.buildImage")}
                            </Button>
                          )}
                        </div>
                      </div>
                      {op?.state === "failed" ? (
                        <p className="break-all text-[0.72rem] text-[var(--st-danger,#dc2626)]" title={op.message ?? undefined}>
                          {op.error_code ? `${op.error_code}：` : ""}
                          {op.message ?? t("pages.docker.buildFailed")}
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
