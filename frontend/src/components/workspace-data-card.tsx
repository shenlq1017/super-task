import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { DatabaseBackup, History, RotateCcw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { useToast } from "@/components/ui/toast";
import { useWorkspace } from "../providers/workspace-provider";
import {
  apiWorkspaceDataList,
  apiWorkspaceDataRestore,
  apiWorkspaceDataRestorePreview,
  apiWorkspaceDataSnapshotCreate,
  apiWorkspaceDataSnapshotDelete,
} from "../ipc/api";
import type {
  DataVolumeView,
  WorkspaceDataRestorePreviewOut,
} from "../ipc/protocol";
import { IpcFailure } from "../ipc/protocol";
import { errorDisplayText } from "@/lib/error-messages";

/**
 * 方向六（ipc.md §10.18）：数据快照卡。
 * 为 supertask.yaml `data.volumes` 声明的数据目录提供离线快照闭环：
 * create → list → restorePreview → restore → delete。恢复 = 目录内容替换。
 */
export function WorkspaceDataCard() {
  const { state } = useWorkspace();
  const workspaceId = state.workspaceId;
  const { t } = useTranslation();
  const { toast } = useToast();
  const [volumes, setVolumes] = useState<DataVolumeView[]>([]);
  const [busy, setBusy] = useState(false);
  const [snapshotTarget, setSnapshotTarget] = useState<DataVolumeView | null>(null);
  const [preview, setPreview] = useState<WorkspaceDataRestorePreviewOut | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{ volumeId: string; snapshotId: string } | null>(null);

  const onErr = (e: unknown) =>
    toast(e instanceof IpcFailure ? errorDisplayText(e.code, e.message) : String(e), "err");

  const reload = useCallback(async () => {
    if (!workspaceId) {
      setVolumes([]);
      return;
    }
    try {
      const out = await apiWorkspaceDataList({ workspaceId });
      setVolumes(out.volumes);
      out.warnings.forEach((w) => toast(w, "err"));
    } catch (e) {
      onErr(e);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspaceId]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const doCreate = async (volume: DataVolumeView) => {
    if (!workspaceId) return;
    setBusy(true);
    try {
      await apiWorkspaceDataSnapshotCreate({ workspaceId, volumeId: volume.id });
      toast(t("pages.workspaces.dataCreatedToast"), "ok");
    } catch (e) {
      onErr(e);
    } finally {
      setBusy(false);
      setSnapshotTarget(null);
      void reload();
    }
  };

  const openPreview = async (volume: DataVolumeView, snapshotId: string) => {
    if (!workspaceId) return;
    setBusy(true);
    try {
      const out = await apiWorkspaceDataRestorePreview({
        workspaceId,
        volumeId: volume.id,
        snapshotId,
      });
      setPreview(out);
    } catch (e) {
      onErr(e);
    } finally {
      setBusy(false);
    }
  };

  const doRestore = async () => {
    if (!workspaceId || !preview) return;
    setBusy(true);
    try {
      const out = await apiWorkspaceDataRestore({
        workspaceId,
        volumeId: preview.volume_id,
        snapshotId: preview.snapshot_id,
      });
      toast(
        t("pages.workspaces.dataRestoredToast", {
          restored: out.restored_files,
          removed: out.removed_files,
        }),
        "ok",
      );
    } catch (e) {
      onErr(e);
    } finally {
      setBusy(false);
      setPreview(null);
      void reload();
    }
  };

  const doDelete = async () => {
    if (!workspaceId || !deleteTarget) return;
    setBusy(true);
    try {
      await apiWorkspaceDataSnapshotDelete({
        workspaceId,
        volumeId: deleteTarget.volumeId,
        snapshotId: deleteTarget.snapshotId,
      });
      toast(t("pages.workspaces.dataDeletedToast"), "ok");
    } catch (e) {
      onErr(e);
    } finally {
      setBusy(false);
      setDeleteTarget(null);
      void reload();
    }
  };

  return (
    <Card className="p-4">
      <h3 className="mb-3 flex items-center gap-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">
        <DatabaseBackup className="size-4" /> {t("pages.workspaces.dataTitle")}
      </h3>
      <p className="mb-2 text-[0.75rem] leading-relaxed text-[var(--t3,#8a8f98)]">
        {t("pages.workspaces.dataHint")}
      </p>

      {!workspaceId ? (
        <p className="text-[0.78rem] text-[var(--t3,#8a8f98)]">{t("pages.workspaces.dataNoWs")}</p>
      ) : volumes.length === 0 ? (
        <p className="text-[0.78rem] text-[var(--t3,#8a8f98)]">{t("pages.workspaces.dataEmpty")}</p>
      ) : (
        <div className="mt-1 flex flex-col gap-3">
          {volumes.map((v) => (
            <div key={v.id} className="rounded-lg border border-[var(--line,#e6e6e6)] p-3">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-[0.8rem] font-semibold text-[var(--t1,#222326)]">{v.id}</span>
                <code className="rounded bg-[var(--tbg,#f4f4f5)] px-1.5 py-0.5 text-[0.72rem] text-[var(--t2,#62666d)]">
                  {v.dir}
                </code>
                {v.service && (
                  <span className="rounded border border-[var(--line,#e6e6e6)] px-1.5 py-0.5 text-[0.7rem] text-[var(--t2,#62666d)]">
                    {t("pages.workspaces.dataBoundService", { service: v.service })}
                  </span>
                )}
                <span className="ml-auto">
                  <Button
                    variant="soft"
                    size="sm"
                    disabled={busy}
                    onClick={() => setSnapshotTarget(v)}
                    className="gap-1"
                  >
                    <DatabaseBackup className="size-3.5" />
                    {t("pages.workspaces.dataSnapshotBtn")}
                  </Button>
                </span>
              </div>

              {v.snapshots.length === 0 ? (
                <p className="mt-2 text-[0.72rem] text-[var(--t3,#8a8f98)]">
                  {t("pages.workspaces.dataNoSnapshots")}
                </p>
              ) : (
                <ul className="mt-2 flex flex-col gap-1.5">
                  {v.snapshots.map((s) => (
                    <li
                      key={s.id}
                      className="flex flex-wrap items-center gap-2 text-[0.75rem] text-[var(--t2,#62666d)]"
                    >
                      <History className="size-3.5 shrink-0" />
                      <span className="whitespace-nowrap">
                        {new Date(s.created_at).toLocaleString()}
                      </span>
                      <span className="whitespace-nowrap">
                        {t("pages.workspaces.dataSnapshotMeta", {
                          files: s.file_count,
                          size: formatBytes(s.total_bytes),
                        })}
                      </span>
                      {s.note && (
                        <span className="truncate text-[var(--t3,#8a8f98)]" title={s.note}>
                          {s.note}
                        </span>
                      )}
                      <span className="ml-auto flex gap-1">
                        <Button
                          variant="soft"
                          size="sm"
                          disabled={busy}
                          onClick={() => void openPreview(v, s.id)}
                          className="gap-1"
                        >
                          <RotateCcw className="size-3.5" />
                          {t("pages.workspaces.dataRestoreBtn")}
                        </Button>
                        <Button
                          variant="soft"
                          size="sm"
                          disabled={busy}
                          onClick={() => setDeleteTarget({ volumeId: v.id, snapshotId: s.id })}
                          className="gap-1"
                        >
                          <Trash2 className="size-3.5" />
                          {t("common.delete")}
                        </Button>
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          ))}
        </div>
      )}

      {/* 创建快照：离线语义确认 */}
      <ConfirmDialog
        open={snapshotTarget != null}
        title={t("pages.workspaces.dataSnapshotConfirmTitle")}
        description={
          snapshotTarget
            ? snapshotTarget.service
              ? t("pages.workspaces.dataSnapshotConfirmDesc", {
                  dir: snapshotTarget.dir,
                  service: snapshotTarget.service,
                })
              : t("pages.workspaces.dataSnapshotConfirmNoService", {
                  dir: snapshotTarget.dir,
                })
            : ""
        }
        onConfirm={() => snapshotTarget && void doCreate(snapshotTarget)}
        onCancel={() => setSnapshotTarget(null)}
      />

      {/* 恢复预览：覆盖面陈述 + 确认恢复（destructive；未就绪仅展示 blockers） */}
      <ConfirmDialog
        open={preview != null}
        title={t("pages.workspaces.dataPreviewTitle")}
        description={preview ? <PreviewBody preview={preview} /> : ""}
        confirmText={t("pages.workspaces.dataRestoreConfirmBtn")}
        destructive={preview?.ready ?? false}
        onConfirm={() => {
          if (preview?.ready) void doRestore();
          else setPreview(null);
        }}
        onCancel={() => setPreview(null)}
      />

      {/* 删除快照 */}
      <ConfirmDialog
        open={deleteTarget != null}
        title={t("pages.workspaces.dataDeleteTitle")}
        description={t("pages.workspaces.dataDeleteDesc", { id: deleteTarget?.snapshotId ?? "" })}
        destructive
        onConfirm={() => void doDelete()}
        onCancel={() => setDeleteTarget(null)}
      />
    </Card>
  );
}

/** 恢复预览正文：写入面 / 删除面 / blockers。 */
function PreviewBody({ preview }: { preview: WorkspaceDataRestorePreviewOut }) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col gap-2 text-left text-[0.78rem] text-[var(--t2,#62666d)]">
      <p>
        {t("pages.workspaces.dataPreviewWrite", {
          files: preview.snapshot_files,
          size: formatBytes(preview.total_bytes),
        })}
      </p>
      {preview.target_exists ? (
        <p className="text-[var(--t1,#222326)]">
          {t("pages.workspaces.dataPreviewRemoves", { count: preview.remove_count })}
        </p>
      ) : (
        <p>{t("pages.workspaces.dataPreviewNoRemove")}</p>
      )}
      {preview.remove_sample.length > 0 && (
        <ul className="max-h-32 overflow-auto rounded bg-[var(--tbg,#f4f4f5)] p-2 font-mono text-[0.7rem]">
          {preview.remove_sample.map((p) => (
            <li key={p}>{p}</li>
          ))}
        </ul>
      )}
      {preview.blockers.length > 0 && (
        <div>
          <p className="font-semibold text-[var(--t1,#222326)]">
            {t("pages.workspaces.dataBlockersTitle")}
          </p>
          <ul className="list-disc pl-4">
            {preview.blockers.map((b, i) => (
              <li key={i}>{b}</li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

/** 字节 human readable（快照元信息展示用）。 */
function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}
