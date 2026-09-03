import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  Cloud,
  CloudOff,
  CloudUpload,
  Eye,
  EyeOff,
  HardDrive,
  LogOut,
  RefreshCw,
  Settings,
  ShieldAlert,
  Wifi,
  WifiOff,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { useToast } from "@/components/ui/toast";
import { useSession } from "../providers/session-provider";
import { CloudAdvancedDialog } from "@/components/cloud-advanced-dialog";
import { CloudMigrationDialog } from "@/components/cloud-migration-dialog";
import {
  apiCloudLogin,
  apiCloudLogout,
  apiCloudMigrateApply,
  apiCloudMigratePlan,
  apiCloudResolve,
  apiCloudSetEndpoint,
  apiCloudStatus,
  apiCloudSync,
  apiCloudTelemetrySet,
} from "../ipc/api";
import type {
  CloudMigratePlanOut,
  CloudResolveChoice,
  CloudStatusOut,
  CloudSyncOut,
} from "../ipc/protocol";
import { IpcFailure } from "../ipc/protocol";
import { errorDisplayText } from "@/lib/error-messages";
import { cn } from "@/lib/utils";

const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
type Action =
  | "status"
  | "login"
  | "logout"
  | "sync"
  | "resolve"
  | "migrate"
  | "endpoint"
  | "telemetry"
  | null;

function cloudError(error: unknown): string {
  return error instanceof IpcFailure
    ? errorDisplayText(error.code, error.message)
    : error instanceof Error
      ? error.message
      : String(error);
}

function formatTime(ts: number | null | undefined, fallback: string): string {
  if (!ts) return fallback;
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(new Date(ts));
}

function formatBytes(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = Math.max(0, n);
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 && unit > 0 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

function connectionTone(connection: CloudStatusOut["connection"]): string {
  switch (connection) {
    case "online":
      return "text-[var(--st-ok)]";
    case "offline":
      return "text-[var(--st-warn)]";
    case "auth_required":
      return "text-[var(--st-danger)]";
    default:
      return "text-[var(--t3)]";
  }
}

/** Cloud account, sync, migration and telemetry workspace. */
export function CloudPage() {
  const { state } = useSession();
  const { t } = useTranslation();
  const { toast } = useToast();
  const [status, setStatus] = useState<CloudStatusOut | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [emailError, setEmailError] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [action, setAction] = useState<Action>(null);
  const [lastSync, setLastSync] = useState<CloudSyncOut | null>(null);
  const [migration, setMigration] = useState<CloudMigratePlanOut | null>(null);
  const [workspaceDirs, setWorkspaceDirs] = useState<Record<string, string>>({});
  const [endpoint, setEndpoint] = useState("");
  const [endpointError, setEndpointError] = useState<string | null>(null);
  const [includeTemplates, setIncludeTemplates] = useState(true);
  const [includeSettings, setIncludeSettings] = useState(true);
  const [advancedOpen, setAdvancedOpen] = useState(false);

  const busy = action !== null;
  const conflictDetails = useMemo(() => {
    if (status?.conflict_details?.length) return status.conflict_details;
    const ids = status?.conflict_ids?.length ? status.conflict_ids : (lastSync?.conflicts ?? []);
    return ids.map((id) => ({
      id,
      entity_type: "unknown",
      server_rev: 0,
      has_local: true,
      has_server: true,
    }));
  }, [lastSync?.conflicts, status?.conflict_details, status?.conflict_ids]);
  const conflictCount = status?.conflicts ?? conflictDetails.length;
  const syncing = status?.runtime?.phase === "syncing" || action === "sync" || action === "migrate";
  const connection = status?.connection ?? (status?.logged_in ? "unknown" : "auth_required");

  const refresh = async (silent = false) => {
    if (!silent) setAction("status");
    setLoadFailed(false);
    try {
      const next = await apiCloudStatus();
      setStatus(next);
      setEndpoint(next.endpoint);
      if (!next.logged_in) {
        setMigration(null);
        setWorkspaceDirs({});
      }
      return next;
    } catch (error) {
      setLoadFailed(true);
      if (!silent) toast(cloudError(error), "err");
      throw error;
    } finally {
      if (!silent) setAction(null);
    }
  };

  useEffect(() => {
    void refresh(true).catch(() => undefined);
    // Bootstrap once; action handlers own subsequent refreshes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!status?.logged_in) return;
    const id = window.setInterval(() => {
      void refresh(true).catch(() => undefined);
    }, syncing ? 1500 : 12000);
    return () => window.clearInterval(id);
  }, [status?.logged_in, syncing]);

  const validateEmail = (value: string) => {
    const normalized = value.trim();
    if (!normalized) return t("pages.cloud.emailRequired");
    if (!EMAIL_PATTERN.test(normalized)) return t("pages.cloud.emailInvalid");
    return null;
  };

  const login = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const nextEmailError = validateEmail(email);
    setEmailError(nextEmailError);
    setFormError(null);
    if (nextEmailError || !password) {
      if (!password) setFormError(t("pages.cloud.passwordRequired"));
      return;
    }
    setAction("login");
    try {
      await apiCloudLogin(email.trim(), password);
      setPassword("");
      const next = await apiCloudStatus();
      setStatus(next);
      setEndpoint(next.endpoint);
      toast(t("pages.cloud.loggedIn"), "ok");
    } catch (error) {
      setFormError(cloudError(error));
    } finally {
      setAction(null);
    }
  };

  const logout = async () => {
    setAction("logout");
    try {
      await apiCloudLogout();
      setLastSync(null);
      setMigration(null);
      setWorkspaceDirs({});
      setPassword("");
      await refresh(true);
      toast(t("pages.cloud.loggedOut"), "ok");
    } catch (error) {
      toast(cloudError(error), "err");
    } finally {
      setAction(null);
    }
  };

  const sync = async () => {
    setAction("sync");
    try {
      const out = await apiCloudSync();
      setLastSync(out);
      const next = await apiCloudStatus();
      setStatus(next);
      toast(
        out.conflicts.length
          ? t("pages.cloud.syncConflicts", { n: out.conflicts.length })
          : t("pages.cloud.syncDone", { pushed: out.pushed, pulled: out.pulled }),
        out.conflicts.length ? "warn" : "ok",
      );
    } catch (error) {
      toast(cloudError(error), "err");
      await refresh(true).catch(() => undefined);
    } finally {
      setAction(null);
    }
  };

  const resolve = async (entityId: string, choice: CloudResolveChoice) => {
    setAction("resolve");
    try {
      await apiCloudResolve(entityId, choice);
      const next = await apiCloudStatus();
      setStatus(next);
      setLastSync((current) =>
        current ? { ...current, conflicts: current.conflicts.filter((id) => id !== entityId) } : current,
      );
      toast(t("pages.cloud.resolved"), "ok");
    } catch (error) {
      toast(cloudError(error), "err");
    } finally {
      setAction(null);
    }
  };

  const loadMigrationPlan = async () => {
    setAction("migrate");
    try {
      const plan = await apiCloudMigratePlan();
      setMigration(plan);
      setWorkspaceDirs({});
    } catch (error) {
      toast(cloudError(error), "err");
    } finally {
      setAction(null);
    }
  };

  const applyMigration = async () => {
    if (!migration) return;
    const workspaces = (migration.entities ?? []).filter((entity) => entity.type === "workspace");
    for (const entity of workspaces) {
      if (!workspaceDirs[entity.id]?.trim()) {
        toast(t("pages.cloud.mapAllWorkspaces"), "err");
        return;
      }
    }
    setAction("migrate");
    try {
      const out = await apiCloudMigrateApply({
        workspaces: workspaces.map((entity) => ({
          entityId: entity.id,
          dir: workspaceDirs[entity.id].trim(),
        })),
        includeTemplates,
        includeSettings,
      });
      setLastSync(out);
      setMigration(null);
      setWorkspaceDirs({});
      await refresh(true);
      toast(t("pages.cloud.migrationApplied"), "ok");
    } catch (error) {
      toast(cloudError(error), "err");
    } finally {
      setAction(null);
    }
  };

  const saveEndpoint = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const value = endpoint.trim().replace(/\/$/, "");
    if (!/^https?:\/\/[^\s/]+(?:\/[^\s]*)?$/.test(value)) {
      setEndpointError(t("pages.cloud.endpointInvalid"));
      return;
    }
    setEndpointError(null);
    setAction("endpoint");
    try {
      const out = await apiCloudSetEndpoint(value);
      setEndpoint(out.endpoint);
      setStatus((current) => (current ? { ...current, endpoint: out.endpoint } : current));
      toast(
        out.supported === false || out.local_only === true
          ? t("pages.cloud.endpointSavedLocal")
          : t("pages.cloud.endpointSaved"),
        "ok",
      );
      setAdvancedOpen(false);
    } catch (error) {
      setEndpointError(cloudError(error));
    } finally {
      setAction(null);
    }
  };

  const toggleTelemetry = async (enabled: boolean) => {
    setAction("telemetry");
    try {
      const out = await apiCloudTelemetrySet(enabled);
      setStatus((current) => (current ? { ...current, telemetry_enabled: out.enabled } : current));
      toast(t("pages.cloud.telemetrySaved"), "ok");
    } catch (error) {
      toast(cloudError(error), "err");
    } finally {
      setAction(null);
    }
  };

  const ConnectionIcon =
    connection === "online" ? Wifi : connection === "offline" ? WifiOff : connection === "auth_required" ? ShieldAlert : CloudOff;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto flex max-w-4xl flex-col gap-4">
          <div className="flex items-start justify-between gap-3">
            <div>
              <h2 className="text-[1.05rem] font-bold tracking-tight text-[var(--t1)]">{t("pages.cloud.title")}</h2>
              <p className="mt-0.5 text-[0.78rem] text-[var(--t3)]">{t("pages.cloud.subtitle")}</p>
            </div>
            <span className="flex shrink-0 gap-2">
              <Button variant="outline" size="sm" className="gap-1" onClick={() => setAdvancedOpen(true)} disabled={busy}>
                <Settings className="size-3.5" /> {t("pages.cloud.advancedTitle")}
              </Button>
              <Button variant="soft" size="sm" className="gap-1" onClick={() => void refresh()} disabled={busy} aria-label={t("pages.cloud.refreshStatus")}>
                <RefreshCw className={action === "status" ? "size-3.5 animate-spin" : "size-3.5"} /> {t("common.refresh")}
              </Button>
            </span>
          </div>

          {status && !status.logged_in ? (
            <Card className="overflow-hidden p-0">
              <div className="grid gap-0 md:grid-cols-[1.1fr_0.9fr]">
                <div className="p-5">
                  <h3 className="mb-3 text-[0.875rem] font-semibold text-[var(--t1)]">{t("pages.cloud.loginTitle")}</h3>
                  <form onSubmit={(event) => void login(event)} noValidate>
                    <div className="grid grid-cols-1 gap-3">
                      <div>
                        <label htmlFor="cloud-email" className="text-[0.75rem] text-[var(--t3)]">{t("pages.cloud.email")}</label>
                        <Input
                          id="cloud-email"
                          className="mt-1"
                          value={email}
                          onChange={(event) => {
                            setEmail(event.target.value);
                            if (emailError) setEmailError(validateEmail(event.target.value));
                          }}
                          type="email"
                          required
                          aria-invalid={!!emailError}
                          autoComplete="username"
                        />
                        {emailError ? <p className="mt-1 text-[0.72rem] text-[#DC2626]" role="alert">{emailError}</p> : null}
                      </div>
                      <div>
                        <label htmlFor="cloud-password" className="text-[0.75rem] text-[var(--t3)]">{t("pages.cloud.password")}</label>
                        <div className="relative mt-1">
                          <Input
                            id="cloud-password"
                            className="pr-10"
                            type={showPassword ? "text" : "password"}
                            value={password}
                            onChange={(event) => {
                              setPassword(event.target.value);
                              setFormError(null);
                            }}
                            required
                            autoComplete="current-password"
                          />
                          <button
                            type="button"
                            className="absolute right-1 top-1/2 inline-flex size-6 -translate-y-1/2 items-center justify-center rounded-[var(--r-sm)] text-[var(--t3)] hover:bg-[var(--surface-2)]"
                            onClick={() => setShowPassword((visible) => !visible)}
                            aria-label={showPassword ? t("pages.cloud.hidePassword") : t("pages.cloud.showPassword")}
                          >
                            {showPassword ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
                          </button>
                        </div>
                      </div>
                    </div>
                    {formError ? (
                      <p className="mt-3 rounded-[var(--r-sm)] border border-red-200 bg-[var(--st-danger-tint)] px-3 py-2 text-[0.75rem] text-[#DC2626]" role="alert">
                        {formError}
                      </p>
                    ) : null}
                    <div className="mt-3 flex items-center gap-2">
                      <Button variant="default" size="sm" type="submit" disabled={busy || !email.trim() || !password}>
                        {action === "login" ? t("common.loading") : t("pages.cloud.loginBtn")}
                      </Button>
                      <span className="text-[0.72rem] text-[var(--t3)]">{t("pages.cloud.enterToSubmit")}</span>
                    </div>
                  </form>
                </div>
                <aside className="border-t border-[var(--line)] bg-[var(--surface-2)] p-5 md:border-l md:border-t-0">
                  <p className="text-[0.75rem] font-semibold text-[var(--t1)]">{t("pages.cloud.localFirstTitle")}</p>
                  <ul className="mt-2 list-inside list-disc text-[0.72rem] leading-relaxed text-[var(--t2)]">
                    <li>{t("pages.cloud.localFirstBullet1")}</li>
                    <li>{t("pages.cloud.localFirstBullet2")}</li>
                    <li>{t("pages.cloud.localFirstBullet3")}</li>
                  </ul>
                  <p className="mt-4 text-[0.72rem] text-[var(--t3)]">{t("pages.cloud.endpointHint", { endpoint: status.endpoint })}</p>
                </aside>
              </div>
            </Card>
          ) : null}

          {status?.logged_in ? (
            <>
              <Card className="p-4">
                <div className="flex flex-wrap items-start gap-3">
                  <div className="flex size-11 items-center justify-center rounded-full bg-[var(--st-accent-tint)] text-[var(--st-accent-hover)]">
                    <Cloud className="size-5" />
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <h3 className="text-[0.95rem] font-semibold text-[var(--t1)]">{status.email}</h3>
                      <Badge variant="secondary">{t("pages.cloud.device", { device: status.device.slice(0, 8) })}</Badge>
                      <span className={cn("inline-flex items-center gap-1 text-[0.72rem] font-medium", connectionTone(connection))}>
                        <ConnectionIcon className="size-3.5" />
                        {t(`pages.cloud.connection.${connection}`)}
                      </span>
                      {syncing ? <Badge variant="outline">{t("pages.cloud.syncing")}</Badge> : null}
                    </div>
                    <p className="mt-1 text-[0.72rem] text-[var(--t3)]">
                      {t("pages.cloud.lastSynced", {
                        time: formatTime(status.last_synced_ms ?? status.runtime?.last_success_ms, t("pages.cloud.neverSynced")),
                      })}
                    </p>
                    {status.health_detail ? <p className="mt-1 text-[0.72rem] text-[var(--st-warn)]">{status.health_detail}</p> : null}
                    {status.runtime?.last_error ? (
                      <p className="mt-1 text-[0.72rem] text-[var(--st-danger)]">{t("pages.cloud.lastError", { error: status.runtime.last_error })}</p>
                    ) : null}
                  </div>
                  <span className="flex shrink-0 gap-2">
                    <Button variant="soft" size="sm" className="gap-1" onClick={() => void sync()} disabled={busy || syncing}>
                      <RefreshCw className={syncing ? "size-3.5 animate-spin" : "size-3.5"} />
                      {syncing ? t("pages.cloud.syncing") : t("pages.cloud.syncNow")}
                    </Button>
                    <Button variant="outline" size="sm" className="gap-1" onClick={() => void logout()} disabled={busy}>
                      <LogOut className="size-3.5" /> {t("pages.cloud.logout")}
                    </Button>
                  </span>
                </div>

                <div className="mt-4 grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
                  <Metric
                    icon={<HardDrive className="size-3.5" />}
                    label={t("pages.cloud.trackedEntities")}
                    value={String(status.tracked?.total ?? "—")}
                    hint={t("pages.cloud.trackedBreakdown", {
                      settings: status.tracked?.settings ?? 0,
                      templates: status.tracked?.templates ?? 0,
                      workspaces: status.tracked?.workspaces ?? 0,
                      mapped: status.tracked?.mapped_workspaces ?? 0,
                    })}
                  />
                  <Metric
                    icon={<AlertTriangle className="size-3.5" />}
                    label={t("pages.cloud.conflictsLabel")}
                    value={String(conflictCount)}
                    hint={t("pages.cloud.pendingTelemetry", { n: status.telemetry_pending ?? 0 })}
                  />
                  <Metric
                    icon={<CloudUpload className="size-3.5" />}
                    label={t("pages.cloud.quotaEntities")}
                    value={status.quota ? `${status.quota.entities}/${status.quota.entities_max}` : "—"}
                    hint={status.quota ? t("pages.cloud.quotaBytes", {
                      bytes: formatBytes(status.quota.bytes),
                      bytesMax: formatBytes(status.quota.bytes_max),
                    }) : t("pages.cloud.quotaUnavailable")}
                  />
                  <Metric
                    icon={<RefreshCw className="size-3.5" />}
                    label={t("pages.cloud.lastResult")}
                    value={
                      status.runtime?.last_result
                        ? `↑${status.runtime.last_result.pushed} ↓${status.runtime.last_result.pulled}`
                        : lastSync
                          ? `↑${lastSync.pushed} ↓${lastSync.pulled}`
                          : "—"
                    }
                    hint={t("pages.cloud.lastResultHint", {
                      pending: status.runtime?.last_result?.pending ?? lastSync?.pending.length ?? 0,
                      skipped: status.runtime?.last_result?.skipped ?? lastSync?.skipped.length ?? 0,
                    })}
                  />
                </div>
              </Card>

              <div className="grid gap-4 lg:grid-cols-[1.35fr_0.85fr]">
                <Card className="p-4">
                  <div className="mb-3 flex items-center justify-between gap-2">
                    <h3 className="text-[0.875rem] font-semibold text-[var(--t1)]">{t("pages.cloud.syncCenter")}</h3>
                    {lastSync || status.runtime?.last_result ? (
                      <span className="text-[0.72rem] text-[var(--t3)]">
                        {t("pages.cloud.lastSync", {
                          pushed: status.runtime?.last_result?.pushed ?? lastSync?.pushed ?? 0,
                          pulled: status.runtime?.last_result?.pulled ?? lastSync?.pulled ?? 0,
                          pending: status.runtime?.last_result?.pending ?? lastSync?.pending.length ?? 0,
                        })}
                      </span>
                    ) : null}
                  </div>
                  {!conflictCount ? (
                    <p className="rounded-[var(--r-sm)] border border-dashed border-[var(--line-strong)] px-3 py-6 text-center text-[0.75rem] text-[var(--t3)]">
                      {t("pages.cloud.noConflicts")}
                    </p>
                  ) : (
                    <div className="flex flex-col gap-2">
                      <p className="text-[0.78rem] text-[var(--st-warn)]">{t("pages.cloud.conflictCount", { n: conflictCount })}</p>
                      {conflictDetails.map((conflict) => (
                        <div key={conflict.id} className="rounded-[var(--r-sm)] border border-[var(--line-strong)] p-3">
                          <div className="flex flex-wrap items-center gap-2">
                            <span className="font-mono text-[0.75rem] font-semibold text-[var(--t1)]">{conflict.id}</span>
                            <Badge variant="outline" className="text-[10px]">{conflict.entity_type}</Badge>
                            {conflict.server_rev ? <span className="text-[0.68rem] text-[var(--t3)]">rev {conflict.server_rev}</span> : null}
                          </div>
                          <p className="mt-1 text-[0.68rem] text-[var(--t3)]">
                            {t("pages.cloud.conflictSides", {
                              local: conflict.has_local ? t("pages.cloud.hasContent") : t("pages.cloud.missingContent"),
                              server: conflict.has_server ? t("pages.cloud.hasContent") : t("pages.cloud.missingContent"),
                            })}
                          </p>
                          <span className="mt-2 flex flex-wrap gap-1">
                            <Button variant="outline" size="sm" onClick={() => void resolve(conflict.id, "local")} disabled={busy}>
                              {t("pages.cloud.keepLocal")}
                            </Button>
                            <Button variant="outline" size="sm" onClick={() => void resolve(conflict.id, "server")} disabled={busy}>
                              {t("pages.cloud.keepServer")}
                            </Button>
                            <Button variant="outline" size="sm" onClick={() => void resolve(conflict.id, "both")} disabled={busy}>
                              {t("pages.cloud.keepBoth")}
                            </Button>
                          </span>
                        </div>
                      ))}
                    </div>
                  )}
                </Card>

                <Card className="p-4">
                  <h3 className="mb-2 flex items-center gap-2 text-[0.875rem] font-semibold text-[var(--t1)]">
                    <CloudUpload className="size-4 text-[var(--st-accent)]" />
                    {t("pages.cloud.migrateTitle")}
                  </h3>
                  <p className="text-[0.75rem] leading-relaxed text-[var(--t3)]">{t("pages.cloud.migrateHint")}</p>
                  <Button className="mt-3" variant="soft" size="sm" onClick={() => void loadMigrationPlan()} disabled={busy || syncing}>
                    {action === "migrate" && !migration ? t("common.loading") : t("pages.cloud.previewMigration")}
                  </Button>
                  <p className="mt-2 text-[0.72rem] text-[var(--t3)]">{t("pages.cloud.migrationCardHint")}</p>
                </Card>
              </div>
            </>
          ) : null}

          {loadFailed ? (
            <Card className="p-4">
              <p className="text-[0.78rem] text-[var(--st-warn)]" role="alert">{t("pages.cloud.loadFailed")}</p>
              <Button className="mt-2 gap-1" variant="soft" size="sm" onClick={() => void refresh()} disabled={busy}>
                <RefreshCw className="size-3.5" /> {t("pages.cloud.retry")}
              </Button>
            </Card>
          ) : null}
          {!state.hello ? <p className="text-[0.75rem] text-[var(--t3)]">{t("common.connectingEngine")}</p> : null}
        </div>
      </div>

      <CloudMigrationDialog
        plan={migration}
        busy={action === "migrate"}
        includeTemplates={includeTemplates}
        includeSettings={includeSettings}
        workspaceDirs={workspaceDirs}
        onIncludeTemplates={setIncludeTemplates}
        onIncludeSettings={setIncludeSettings}
        onWorkspaceDir={(id, dir) => setWorkspaceDirs((current) => ({ ...current, [id]: dir }))}
        onApply={() => void applyMigration()}
        onClose={() => {
          setMigration(null);
          setWorkspaceDirs({});
        }}
      />
      <CloudAdvancedDialog
        open={advancedOpen}
        busy={busy}
        endpoint={endpoint}
        endpointError={endpointError}
        telemetryEnabled={!!status?.telemetry_enabled}
        onEndpointChange={(value) => {
          setEndpoint(value);
          setEndpointError(null);
        }}
        onSaveEndpoint={(event) => void saveEndpoint(event)}
        onTelemetryChange={(enabled) => void toggleTelemetry(enabled)}
        onClose={() => setAdvancedOpen(false)}
      />
    </div>
  );
}

function Metric(props: { icon: React.ReactNode; label: string; value: string; hint: string }) {
  return (
    <div className="rounded-[var(--r-sm)] border border-[var(--line)] bg-[var(--surface-2)] px-3 py-2.5">
      <span className="flex items-center gap-1.5 text-[0.68rem] text-[var(--t3)]">
        {props.icon}
        {props.label}
      </span>
      <strong className="mt-1 block font-mono text-[0.95rem] text-[var(--t1)]">{props.value}</strong>
      <p className="mt-0.5 text-[0.65rem] leading-relaxed text-[var(--t3)]">{props.hint}</p>
    </div>
  );
}
