import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, CloudUpload, Eye, EyeOff, LogOut, RefreshCw, Settings2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { useToast } from "@/components/ui/toast";
import { useSession } from "../providers/session-provider";
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
  CloudStatusOut,
  CloudSyncOut,
  CloudResolveChoice,
} from "../ipc/protocol";
import { IpcFailure } from "../ipc/protocol";
import { errorDisplayText } from "@/lib/error-messages";

const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

type Action = "status" | "login" | "logout" | "sync" | "resolve" | "migrate" | "endpoint" | "telemetry" | null;

function cloudError(error: unknown): string {
  return error instanceof IpcFailure ? errorDisplayText(error.code, error.message) : error instanceof Error ? error.message : String(error);
}

function formatLastSynced(ts: number | null | undefined, fallback: string): string {
  if (!ts) return fallback;
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(new Date(ts));
}

/** Cloud account, sync, migration and telemetry workspace. The page talks only to typed API wrappers. */
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
  const [endpoint, setEndpoint] = useState("");
  const [endpointError, setEndpointError] = useState<string | null>(null);
  const [includeTemplates, setIncludeTemplates] = useState(true);
  const [includeSettings, setIncludeSettings] = useState(true);

  const busy = action !== null;
  const conflictIds = useMemo(
    () => status?.conflict_ids?.length ? status.conflict_ids : (lastSync?.conflicts ?? []),
    [lastSync?.conflicts, status?.conflict_ids],
  );
  const migrationEntities = migration?.entities ?? [];
  const conflictCount = status?.conflicts ?? conflictIds.length;

  const refresh = async (silent = false) => {
    if (!silent) setAction("status");
    setLoadFailed(false);
    try {
      const next = await apiCloudStatus();
      setStatus(next);
      setEndpoint(next.endpoint);
      if (!next.logged_in) setMigration(null);
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
    if (status?.endpoint && !endpoint) setEndpoint(status.endpoint);
  }, [endpoint, status?.endpoint]);

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
      // Clear the secret immediately; it is never stored in component state after login.
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
      toast(out.conflicts.length ? t("pages.cloud.syncConflicts", { n: out.conflicts.length }) : t("pages.cloud.syncDone", { pushed: out.pushed, pulled: out.pulled }), out.conflicts.length ? "warn" : "ok");
    } catch (error) {
      toast(cloudError(error), "err");
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
      setLastSync((current) => current ? { ...current, conflicts: current.conflicts.filter((id) => id !== entityId) } : current);
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
      setMigration(await apiCloudMigratePlan());
    } catch (error) {
      toast(cloudError(error), "err");
    } finally {
      setAction(null);
    }
  };

  const applyMigration = async () => {
    if (!migration) return;
    setAction("migrate");
    try {
      await apiCloudMigrateApply({
        workspaces: migrationEntities.filter((entity) => entity.type === "workspace").map((entity) => ({ entityId: entity.id, dir: "" })),
        includeTemplates,
        includeSettings,
      });
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
      setStatus((current) => current ? { ...current, endpoint: out.endpoint } : current);
      toast(out.supported === false || out.local_only === true ? t("pages.cloud.endpointSavedLocal") : t("pages.cloud.endpointSaved"), "ok");
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
      setStatus((current) => current ? { ...current, telemetry_enabled: out.enabled } : current);
      toast(t("pages.cloud.telemetrySaved"), "ok");
    } catch (error) {
      toast(cloudError(error), "err");
    } finally {
      setAction(null);
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto flex max-w-3xl flex-col gap-4">
          <div className="flex items-start justify-between gap-3">
            <div>
              <h2 className="text-[1.05rem] font-bold tracking-tight text-[var(--t1,#222326)]">{t("pages.cloud.title")}</h2>
              <p className="mt-0.5 text-[0.78rem] text-[var(--t3,#8a8f98)]">{t("pages.cloud.subtitle")}</p>
            </div>
            <Button variant="soft" size="sm" className="gap-1" onClick={() => void refresh()} disabled={busy} aria-label={t("pages.cloud.refreshStatus")}>
              <RefreshCw className={action === "status" ? "size-3.5 animate-spin" : "size-3.5"} /> {t("common.refresh")}
            </Button>
          </div>

          {status && !status.logged_in ? (
            <Card className="p-4">
              <h3 className="mb-3 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">{t("pages.cloud.loginTitle")}</h3>
              <form onSubmit={(event) => void login(event)} noValidate>
                <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                  <div>
                    <label htmlFor="cloud-email" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.cloud.email")}</label>
                    <Input id="cloud-email" className="mt-1" value={email} onChange={(event) => { setEmail(event.target.value); if (emailError) setEmailError(validateEmail(event.target.value)); }} type="email" required aria-required="true" aria-invalid={!!emailError} aria-describedby={emailError ? "cloud-email-error" : undefined} autoComplete="username" />
                    {emailError ? <p id="cloud-email-error" className="mt-1 text-[0.72rem] text-[#DC2626]" role="alert">{emailError}</p> : null}
                  </div>
                  <div>
                    <label htmlFor="cloud-password" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.cloud.password")}</label>
                    <div className="relative mt-1">
                      <Input id="cloud-password" className="pr-10" type={showPassword ? "text" : "password"} value={password} onChange={(event) => { setPassword(event.target.value); setFormError(null); }} required aria-required="true" autoComplete="current-password" />
                      <button type="button" className="absolute right-1 top-1/2 inline-flex size-6 -translate-y-1/2 cursor-pointer items-center justify-center rounded-[var(--r-sm,8px)] text-[var(--t3,#8a8f98)] transition-colors duration-150 hover:bg-[var(--surface-2,#f3f4f5)] hover:text-[var(--t1,#222326)] focus-visible:outline-2 focus-visible:outline-[var(--st-accent,#5e6ad2)]" onClick={() => setShowPassword((visible) => !visible)} aria-label={showPassword ? t("pages.cloud.hidePassword") : t("pages.cloud.showPassword")}>
                        {showPassword ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
                      </button>
                    </div>
                  </div>
                </div>
                {formError ? <p className="mt-3 rounded-[var(--r-sm,8px)] border border-red-200 bg-[var(--st-danger-tint,#fdecec)] px-3 py-2 text-[0.75rem] text-[#DC2626]" role="alert">{formError}</p> : null}
                <div className="mt-3 flex items-center gap-2">
                  <Button variant="default" size="sm" type="submit" disabled={busy || !email.trim() || !password}>
                    {action === "login" ? t("common.loading") : t("pages.cloud.loginBtn")}
                  </Button>
                  <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.cloud.enterToSubmit")}</span>
                </div>
                <p className="mt-2 text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.cloud.endpointHint", { endpoint: status.endpoint })}</p>
              </form>
            </Card>
          ) : null}

          {status?.logged_in ? (
            <>
              <Card className="p-4">
                <div className="flex flex-wrap items-center gap-2">
                  <h3 className="text-[0.875rem] font-semibold text-[var(--t1,#222326)]">{status.email}</h3>
                  <Badge variant="secondary">{t("pages.cloud.device", { device: status.device.slice(0, 8) })}</Badge>
                  <span className="ml-auto flex gap-2">
                    <Button variant="soft" size="sm" className="gap-1" onClick={() => void sync()} disabled={busy}>
                      <RefreshCw className={action === "sync" ? "size-3.5 animate-spin" : "size-3.5"} /> {t("pages.cloud.syncNow")}
                    </Button>
                    <Button variant="outline" size="sm" className="gap-1" onClick={() => void logout()} disabled={busy}>
                      <LogOut className="size-3.5" /> {t("pages.cloud.logout")}
                    </Button>
                  </span>
                </div>
                <p className="mt-2 text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.cloud.lastSynced", { time: formatLastSynced(status.last_synced_ms, t("pages.cloud.neverSynced")) })}</p>
                {status.quota ? <p className="mt-1 font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.cloud.quota", { entities: status.quota.entities, entitiesMax: status.quota.entities_max, bytes: status.quota.bytes, bytesMax: status.quota.bytes_max })}</p> : null}
              </Card>

              <Card className="p-4">
                <h3 className="mb-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">{t("pages.cloud.syncCenter")}</h3>
                {lastSync ? <p className="mb-2 text-[0.75rem] text-[var(--t2,#62666d)]">{t("pages.cloud.lastSync", { pushed: lastSync.pushed, pulled: lastSync.pulled, pending: lastSync.pending.length })}</p> : null}
                {!conflictCount ? <p className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.cloud.noConflicts")}</p> : <div className="flex flex-col gap-2"><p className="text-[0.78rem] text-[var(--st-warn,#9a6700)]">{t("pages.cloud.conflictCount", { n: conflictCount })}</p>{conflictIds.map((id) => <div key={id} className="flex flex-wrap items-center gap-2 rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] p-2"><span className="font-mono text-[0.72rem]">{id}</span><span className="ml-auto flex flex-wrap gap-1"><Button variant="outline" size="sm" onClick={() => void resolve(id, "local")} disabled={busy}>{t("pages.cloud.keepLocal")}</Button><Button variant="outline" size="sm" onClick={() => void resolve(id, "server")} disabled={busy}>{t("pages.cloud.keepServer")}</Button><Button variant="outline" size="sm" onClick={() => void resolve(id, "both")} disabled={busy}>{t("pages.cloud.keepBoth")}</Button></span></div>)}</div>}
              </Card>
            </>
          ) : null}

          <Card className="p-4">
            <h3 className="mb-2 flex items-center gap-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]"><CloudUpload className="size-4" /> {t("pages.cloud.migrateTitle")}</h3>
            <p className="text-[0.75rem] leading-relaxed text-[var(--t3,#8a8f98)]">{t("pages.cloud.migrateHint")}</p>
            <div className="mt-3 flex flex-wrap items-center gap-2">
              <Button variant="soft" size="sm" onClick={() => void loadMigrationPlan()} disabled={busy || !status?.logged_in}>{action === "migrate" && !migration ? t("common.loading") : t("pages.cloud.previewMigration")}</Button>
              {!status?.logged_in ? <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.cloud.loginForMigration")}</span> : null}
            </div>
            {migration ? <div className="mt-3 rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] p-3"><p className="text-[0.78rem] font-semibold text-[var(--t1,#222326)]">{t("pages.cloud.migrationEntities", { n: migrationEntities.length })}</p><ul className="mt-1 list-inside list-disc text-[0.72rem] text-[var(--t2,#62666d)]">{migrationEntities.map((entity) => <li key={entity.id}>{entity.name ?? entity.id} · {entity.type}</li>)}</ul>{migration.toolchain_gaps.length ? <p className="mt-2 text-[0.72rem] text-[var(--st-warn,#9a6700)]">{t("pages.cloud.toolchainGaps", { n: migration.toolchain_gaps.length })}</p> : null}<div className="mt-3 flex flex-col gap-1.5 text-[0.75rem] text-[var(--t2,#62666d)]"><label className="flex cursor-pointer items-center gap-2"><input type="checkbox" checked={includeTemplates} onChange={(event) => setIncludeTemplates(event.target.checked)} />{t("pages.cloud.includeTemplates")}</label><label className="flex cursor-pointer items-center gap-2"><input type="checkbox" checked={includeSettings} onChange={(event) => setIncludeSettings(event.target.checked)} />{t("pages.cloud.includeSettings")}</label></div><Button className="mt-3 gap-1" variant="default" size="sm" onClick={() => void applyMigration()} disabled={busy}><Check className="size-3.5" /> {t("pages.cloud.applyMigration")}</Button></div> : null}
          </Card>

          <Card className="p-4">
            <h3 className="mb-2 flex items-center gap-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]"><Settings2 className="size-4" /> {t("pages.cloud.advancedTitle")}</h3>
            <form onSubmit={(event) => void saveEndpoint(event)} className="flex flex-col gap-2">
              <label htmlFor="cloud-endpoint" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.cloud.endpoint")}</label>
              <div className="flex flex-col gap-2 sm:flex-row"><Input id="cloud-endpoint" value={endpoint} onChange={(event) => { setEndpoint(event.target.value); setEndpointError(null); }} type="url" inputMode="url" aria-invalid={!!endpointError} aria-describedby={endpointError ? "cloud-endpoint-error" : "cloud-endpoint-help"} /><Button variant="success" size="sm" type="submit" disabled={busy || !endpoint.trim()}>{t("pages.cloud.saveEndpoint")}</Button></div>
              <p id="cloud-endpoint-help" className="text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.cloud.endpointLocalHint")}</p>
              {endpointError ? <p id="cloud-endpoint-error" className="text-[0.72rem] text-[#DC2626]" role="alert">{endpointError}</p> : null}
            </form>
            <div className="mt-4 border-t border-[var(--line,#e6e6e6)] pt-3"><label className="flex cursor-pointer items-start justify-between gap-4 text-[0.8rem] text-[var(--t1,#222326)]"><span>{t("pages.cloud.telemetry")}</span><input type="checkbox" checked={!!status?.telemetry_enabled} onChange={(event) => void toggleTelemetry(event.target.checked)} disabled={busy} aria-label={t("pages.cloud.telemetry")} /></label><p className="mt-1 text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.cloud.telemetryHint")}</p></div>
          </Card>

          {loadFailed ? <Card className="p-4"><p className="text-[0.78rem] text-[var(--st-warn,#9a6700)]" role="alert">{t("pages.cloud.loadFailed")}</p><Button className="mt-2 gap-1" variant="soft" size="sm" onClick={() => void refresh()} disabled={busy}><RefreshCw className="size-3.5" /> {t("pages.cloud.retry")}</Button></Card> : null}
          {!state.hello ? <p className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("common.connectingEngine")}</p> : null}
        </div>
      </div>
    </div>
  );
}
