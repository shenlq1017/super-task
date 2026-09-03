import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useOutletContext } from "react-router-dom";
import { Cloud, Loader2, Settings } from "lucide-react";
import type { ShellCtx } from "../app/AppShell";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { cn } from "@/lib/utils";
import { useSession } from "../providers/session-provider";
import { useRuntime } from "../providers/runtime-provider";
import { useOperations } from "../providers/operation-provider";
import { apiCloudSetEndpoint, apiCloudStatus, apiCloudTelemetrySet, apiSavePrefs, apiUpdateCheck, apiUpdateInstall } from "../ipc/api";
import { IpcFailure } from "../ipc/protocol";
import type { CloudStatusOut, UpdateCheckResult } from "../ipc/protocol";
import { opErrorLabel } from "../lib/status";
import { errorDisplayText } from "@/lib/error-messages";
import { useToast } from "@/components/ui/toast";
import { applyLocalePreference } from "@/i18n";
import { isSupportedLocale, resolveLocale, SUPPORTED_LOCALES } from "@/i18n/resolve-locale";

/** 偏好开关行：与既有 checkbox 样式一致，说明文案放第二行。 */
function PrefRow({
  label,
  desc,
  checked,
  onChange,
  disabled,
}: {
  label: string;
  desc: string;
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <label className={cn("flex items-center justify-between gap-4 py-1.5 text-[0.875rem] text-[var(--t1,#222326)]", disabled && "opacity-60")}>
      <span>
        {label}
        <span className="block text-[0.75rem] leading-relaxed text-[var(--t3,#8a8f98)]">{desc}</span>
      </span>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
        aria-label={label}
        className="shrink-0"
      />
    </label>
  );
}

/** 语言选项（跟随系统 + 四语）。标签用各语言自称，不随当前语言翻译。 */
const LOCALE_OPTIONS: { value: "auto" | (typeof SUPPORTED_LOCALES)[number]; label: string }[] = [
  { value: "auto", label: "跟随系统" },
  { value: "zh-CN", label: "简体中文" },
  { value: "zh-TW", label: "繁體中文" },
  { value: "en-US", label: "English" },
  { value: "ja-JP", label: "日本語" },
];

/** 更新区块：检查更新（长操作）→ 结果 → 确认安装。 */
function UpdateCard() {
  const { state } = useSession();
  const { get } = useOperations();
  const runtime = useRuntime();
  const { toast } = useToast();
  const { t } = useTranslation();

  const [checkOpId, setCheckOpId] = useState<string | null>(null);
  const [installOpId, setInstallOpId] = useState<string | null>(null);
  const [confirmInstall, setConfirmInstall] = useState(false);
  const [blocked, setBlocked] = useState<string | null>(null);

  const checkOp = checkOpId ? get(checkOpId) : null;
  const installOp = installOpId ? get(installOpId) : null;

  const checkRunning = !!checkOp && (checkOp.state === "queued" || checkOp.state === "running");
  const installRunning = !!installOp && (installOp.state === "queued" || installOp.state === "running");

  const available: UpdateCheckResult | null = (() => {
    if (!checkOp || checkOp.state !== "succeeded") return null;
    const r = checkOp.result as UpdateCheckResult | null;
    if (!r || typeof r !== "object" || r.status !== "available") return null;
    return r;
  })();

  const startCheck = async () => {
    setBlocked(null);
    try {
      const { operation_id } = await apiUpdateCheck();
      setCheckOpId(operation_id);
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  const startInstall = async (version: string) => {
    setBlocked(null);
    try {
      const { operation_id } = await apiUpdateInstall(version);
      setInstallOpId(operation_id);
    } catch (e) {
      // 同步拒绝（UPDATE_BLOCKED_RUNNING / UPDATE_FAILED）：给出原因与下一步
      if (e instanceof IpcFailure && e.code === "UPDATE_BLOCKED_RUNNING") {
        setBlocked(t("pages.settings.installBlockedHint"));
      } else {
        toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
      }
    }
  };

  // 安装 operation 失败：UPDATE_BLOCKED_RUNNING → 显示原因 + 停止全部快捷键
  useEffect(() => {
    if (installOp?.state === "failed" && installOp.error_code === "UPDATE_BLOCKED_RUNNING") {
      setBlocked(t("pages.settings.installBlockedHint"));
    }
  }, [installOp, t]);

  return (
    <Card className="p-4">
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-[0.875rem] font-semibold text-[var(--t1,#222326)]">{t("pages.settings.updateTitle")}</h3>
        <span className="font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]">
          {t("pages.settings.currentVersion", { version: state.hello?.product_version ?? "—" })}
        </span>
      </div>
      <p className="mt-1 text-[0.75rem] leading-relaxed text-[var(--t3,#8a8f98)]">
        {t("pages.settings.updatePolicy")}
      </p>

      <div className="mt-3 flex items-center gap-2">
        <Button size="sm" variant="soft" onClick={() => void startCheck()} disabled={checkRunning || installRunning}>
          {t("pages.settings.checkUpdate")}
        </Button>
        {checkRunning ? (
          <span className="flex items-center gap-1.5 text-[0.76rem] text-[var(--t2,#62666d)]" role="status">
            <Loader2 className="size-3.5 animate-spin text-[var(--st-accent,#5e6ad2)]" />
            {checkOp.message ?? t("pages.settings.checkingUpdate")}
          </span>
        ) : null}
      </div>

      {checkOp?.state === "failed" ? (
        <div className="mt-2.5 rounded-lg border border-red-200 bg-[var(--st-danger-tint,#fdecec)] px-3 py-2 text-[0.78rem] text-[#DC2626]" role="alert">
          {errorDisplayText(checkOp.error_code, checkOp.message, opErrorLabel(checkOp.error_code))}
        </div>
      ) : null}

      {checkOp?.state === "succeeded" && !available ? (
        <div className="mt-2.5 flex items-center gap-2 text-[0.8rem] text-[var(--st-ok-deep,#1e7e35)]" role="status">
          <span className="size-1.5 rounded-full bg-[var(--st-ok,#27a644)]" />
          {t("pages.settings.upToDate")}
        </div>
      ) : null}

      {available ? (
        <div className="mt-3 rounded-[var(--r-md,12px)] border border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] p-3">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[0.85rem] font-semibold text-[var(--t1,#222326)]">{t("pages.settings.newVersionFound")}</span>
            <span className="font-mono text-[0.8rem] text-[var(--st-accent,#5e6ad2)]">{available.version}</span>
            {available.date ? (
              <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.settings.releasedAt", { date: available.date })}</span>
            ) : null}
            <Button
              size="sm"
              className="ml-auto"
              disabled={installRunning || checkRunning}
              onClick={() => setConfirmInstall(true)}
            >
              {t("pages.settings.installUpdate")}
            </Button>
          </div>
          {available.notes ? (
            <p className="mt-1.5 whitespace-pre-wrap break-words text-[0.76rem] leading-relaxed text-[var(--t2,#62666d)]">
              {available.notes}
            </p>
          ) : null}
        </div>
      ) : null}

      {installRunning ? (
        <div className="mt-3 rounded-[var(--r-md,12px)] border border-[rgb(94_106_210_/_0.35)] p-3" role="status">
          <div className="flex items-center gap-2 text-[0.8rem] text-[var(--t1,#222326)]">
            <Loader2 className="size-3.5 animate-spin text-[var(--st-accent,#5e6ad2)]" />
            {installOp.message ?? t("pages.settings.processingUpdate")}
          </div>
          {/* progress 可能为 null：只显示状态文案，不伪造百分比 */}
          {installOp.progress != null ? (
            <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-[var(--surface-2,#f3f4f5)]">
              <div
                className="h-full rounded-full bg-[var(--st-accent,#5e6ad2)] transition-all duration-300"
                style={{ width: `${Math.round(Math.min(1, Math.max(0, installOp.progress)) * 100)}%` }}
              />
            </div>
          ) : null}
        </div>
      ) : null}

      {blocked ? (
        <div
          className={cn(
            "mt-3 flex flex-wrap items-center gap-2 rounded-[var(--r-md,12px)] border px-3 py-2 text-[0.78rem]",
            "border-[#f0d58a] bg-[#fdf6e3] text-[#B7791F]",
          )}
          role="alert"
        >
          <span className="min-w-0 flex-1">{blocked}</span>
          <Button variant="destructive" size="sm" onClick={() => void runtime.actions.stopAll()}>
            {t("common.stopAll")}
          </Button>
        </div>
      ) : null}

      <ConfirmDialog
        open={confirmInstall}
        title={t("pages.settings.confirmInstallTitle", { version: available?.version ?? "" })}
        description={t("pages.settings.confirmInstallDesc")}
        confirmText={t("pages.settings.installUpdate")}
        cancelText={t("common.cancel")}
        onConfirm={() => {
          setConfirmInstall(false);
          if (available?.version) void startInstall(available.version);
        }}
        onCancel={() => setConfirmInstall(false)}
      />
    </Card>
  );
}

function CloudSettingsCard() {
  const { t } = useTranslation();
  const { toast } = useToast();
  const [status, setStatus] = useState<CloudStatusOut | null>(null);
  const [endpoint, setEndpoint] = useState("");
  const [endpointError, setEndpointError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState<"endpoint" | "telemetry" | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const loadStatus = async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const next = await apiCloudStatus();
      setStatus(next);
      setEndpoint(next.endpoint);
    } catch (error) {
      setLoadError(error instanceof IpcFailure ? opErrorLabel(error.code) : String(error));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadStatus();
  }, []);

  const saveEndpoint = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const value = endpoint.trim().replace(/\/$/, "");
    if (!/^https?:\/\/[^\s/]+(?:\/[^\s]*)?$/.test(value)) {
      setEndpointError(t("pages.settings.cloudEndpointInvalid"));
      return;
    }
    setEndpointError(null);
    setSaving("endpoint");
    try {
      const out = await apiCloudSetEndpoint(value);
      setEndpoint(out.endpoint);
      setStatus((current) => current ? { ...current, endpoint: out.endpoint } : current);
      toast(out.supported === false || out.local_only === true ? t("pages.settings.cloudEndpointSavedLocal") : t("pages.settings.cloudEndpointSaved"), "ok");
    } catch (error) {
      setEndpointError(error instanceof IpcFailure ? opErrorLabel(error.code) : String(error));
    } finally {
      setSaving(null);
    }
  };

  const toggleTelemetry = async (enabled: boolean) => {
    setSaving("telemetry");
    try {
      const out = await apiCloudTelemetrySet(enabled);
      setStatus((current) => current ? { ...current, telemetry_enabled: out.enabled } : current);
      toast(t("pages.settings.telemetrySaved"), "ok");
    } catch (error) {
      toast(error instanceof IpcFailure ? opErrorLabel(error.code) : String(error), "err");
    } finally {
      setSaving(null);
    }
  };

  return (
    <Card className="p-4">
      <h3 className="mb-1 flex items-center gap-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">
        <Cloud className="size-4 text-[var(--st-accent,#5e6ad2)]" /> {t("pages.settings.cloudTitle")}
      </h3>
      <p className="mb-3 text-[0.75rem] leading-relaxed text-[var(--t3,#8a8f98)]">{t("pages.settings.cloudDesc")}</p>
      {loading ? (
        <div className="flex items-center gap-2 text-[0.75rem] text-[var(--t3,#8a8f98)]" role="status">
          <Loader2 className="size-3.5 animate-spin text-[var(--st-accent,#5e6ad2)]" /> {t("pages.settings.cloudLoading")}
        </div>
      ) : loadError ? (
        <div className="flex flex-wrap items-center gap-2" role="alert">
          <p className="min-w-0 flex-1 text-[0.75rem] text-[#DC2626]">{loadError}</p>
          <Button variant="soft" size="sm" onClick={() => void loadStatus()}>{t("common.retry")}</Button>
        </div>
      ) : (
        <>
          <form onSubmit={(event) => void saveEndpoint(event)} className="flex flex-col gap-2">
            <label htmlFor="settings-cloud-endpoint" className="flex items-center gap-2 text-[0.75rem] text-[var(--t2,#62666d)]">
              <Settings className="size-3.5" /> {t("pages.settings.cloudEndpoint")}
            </label>
            <div className="flex flex-col gap-2 sm:flex-row">
              <Input
                id="settings-cloud-endpoint"
                value={endpoint}
                onChange={(event) => { setEndpoint(event.target.value); setEndpointError(null); }}
                type="url"
                inputMode="url"
                aria-invalid={!!endpointError}
                aria-describedby={endpointError ? "settings-cloud-endpoint-error" : "settings-cloud-endpoint-help"}
                disabled={saving !== null}
              />
              <Button variant="success" size="sm" type="submit" disabled={saving !== null || !endpoint.trim()}>
                {saving === "endpoint" ? t("common.loading") : t("pages.settings.saveCloudEndpoint")}
              </Button>
            </div>
            <p id="settings-cloud-endpoint-help" className="text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.settings.cloudEndpointHint")}</p>
            {endpointError ? <p id="settings-cloud-endpoint-error" className="text-[0.72rem] text-[#DC2626]" role="alert">{endpointError}</p> : null}
          </form>
          <div className="mt-4 border-t border-[var(--line,#e6e6e6)] pt-3">
            <PrefRow
              label={t("pages.settings.telemetry")}
              desc={t("pages.settings.telemetryDesc")}
              checked={!!status?.telemetry_enabled}
              onChange={(enabled) => void toggleTelemetry(enabled)}
            />
            {saving === "telemetry" ? <p className="mt-1 flex items-center gap-1.5 text-[0.72rem] text-[var(--t3,#8a8f98)]" role="status"><Loader2 className="size-3 animate-spin" /> {t("common.loading")}</p> : null}
          </div>
        </>
      )}
    </Card>
  );
}

function SettingsPageInner() {
  const { state } = useSession();
  const { toast } = useToast();
  const { t } = useTranslation();
  const shell = useOutletContext<ShellCtx>();
  const [theme, setTheme] = useState(state.app?.prefs.theme ?? "light");
  const [locale, setLocale] = useState<string>(state.app?.prefs?.locale ?? "auto");
  const [restore, setRestore] = useState(state.app?.prefs.restoreLast ?? true);
  const [closeToTray, setCloseToTray] = useState(state.app?.prefs.closeToTray ?? true);
  const [startOnLogin, setStartOnLogin] = useState(state.app?.prefs.startOnLogin ?? false);
  const [updateCheck, setUpdateCheck] = useState(state.app?.prefs.updateCheck ?? true);
  // 1.7 §8.2：崩溃通知开关（localStorage，读端在 crash-notifier）
  const [crashNotify, setCrashNotify] = useState(() => localStorage.getItem("st:crashNotify") !== "off");

  useEffect(() => {
    setTheme(state.app?.prefs.theme ?? "light");
    setLocale(state.app?.prefs?.locale ?? "auto");
    setRestore(state.app?.prefs.restoreLast ?? true);
    setCloseToTray(state.app?.prefs.closeToTray ?? true);
    setStartOnLogin(state.app?.prefs.startOnLogin ?? false);
    setUpdateCheck(state.app?.prefs.updateCheck ?? true);
  }, [state.app]);

  const save = async () => {
    try {
      localStorage.setItem("st:crashNotify", crashNotify ? "on" : "off");
      await apiSavePrefs({ theme, locale, restoreLast: restore, closeToTray, startOnLogin, updateCheck });
      toast(t("operations.prefsSaved"), "ok");
    } catch (e) {
      if (e instanceof IpcFailure && e.code === "AUTOSTART_FAILED") {
        // 后端已回滚偏好为 false；UI 开关同步回滚为未开启
        setStartOnLogin(false);
      }
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  /** 语言切换：即时生效（changeLanguage），并立即持久化到 app data。 */
  const changeLocale = async (value: string) => {
    setLocale(value);
    applyLocalePreference(value);
    try {
      await apiSavePrefs({ locale: value });
      toast(t("operations.prefsSaved"), "ok");
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  };

  // 显式 locale 未知 → 已回落 zh-CN，给出提示（规格 §2.3.4）
  const unknownLocale = locale !== "auto" && !isSupportedLocale(locale);
  const resolvedAuto = locale === "auto" ? resolveLocale("auto") : null;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto flex max-w-2xl flex-col gap-6">
          <Card className="p-4">
            <h3 className="mb-3 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">{t("pages.settings.general")}</h3>
            <div className="flex flex-col divide-y divide-[var(--line,#e6e6e6)]">
              <PrefRow
                label={t("pages.settings.restoreLast")}
                desc={t("pages.settings.restoreLastDesc")}
                checked={restore}
                onChange={setRestore}
              />
              <PrefRow
                label={t("pages.settings.crashNotify")}
                desc={t("pages.settings.crashNotifyDesc")}
                checked={crashNotify}
                onChange={setCrashNotify}
              />
              <PrefRow
                label={t("pages.settings.closeToTray")}
                desc={t("pages.settings.closeToTrayDesc")}
                checked={closeToTray}
                onChange={setCloseToTray}
              />
              <PrefRow
                label={t("pages.settings.startOnLogin")}
                desc={t("pages.settings.startOnLoginDesc")}
                checked={startOnLogin}
                onChange={setStartOnLogin}
              />
              <PrefRow
                label={t("pages.settings.updateCheck")}
                desc={t("pages.settings.updateCheckDesc")}
                checked={updateCheck}
                onChange={setUpdateCheck}
              />
            </div>
            <div className="mt-3 flex justify-end">
              <Button size="sm" variant="success" onClick={() => void save()}>
                {t("common.save")}
              </Button>
            </div>
          </Card>

          <Card className="p-4">
            <h3 className="mb-3 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">{t("pages.settings.appearance")}</h3>
            <div className="flex gap-2">
              {(["light", "dark"] as const).map((th) => (
                <button
                  key={th}
                  disabled={th === "dark"}
                  onClick={() => setTheme(th)}
                  className={cn(
                    "flex-1 rounded-[var(--r-sm,8px)] border px-3 py-2 text-[0.875rem] transition-colors duration-150",
                    theme === th
                      ? "border-[var(--st-accent,#5e6ad2)] bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent-hover,#4f5ac8)]"
                      : "border-[var(--line,#e6e6e6)] text-[var(--t2,#62666d)] hover:border-[var(--line-strong,#d0d6e0)]",
                    th === "dark" && "opacity-50",
                  )}
                >
                  {th === "light" ? t("pages.settings.themeLight") : t("pages.settings.themeDarkSoon")}
                </button>
              ))}
            </div>

            {/* 语言（1.4 §2.3）：跟随系统 / 简体中文 / 繁體中文 / English / 日本語 */}
            <div className="mt-4 flex items-center justify-between gap-4">
              <label htmlFor="st-locale" className="text-[0.875rem] text-[var(--t1,#222326)]">
                {t("pages.settings.language")}
                <span className="block text-[0.75rem] leading-relaxed text-[var(--t3,#8a8f98)]">
                  {t("pages.settings.languageDesc")}
                </span>
              </label>
              <select
                id="st-locale"
                value={locale}
                onChange={(e) => void changeLocale(e.target.value)}
                className="h-8 shrink-0 cursor-pointer rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] px-2 text-[0.8rem] text-[var(--t1,#222326)] transition-colors duration-150 hover:border-[var(--t3,#8a8f98)] focus-visible:border-[var(--st-accent,#5e6ad2)] focus-visible:outline-none"
              >
                {LOCALE_OPTIONS.map((opt) => (
                  <option key={opt.value} value={opt.value}>
                    {opt.value === "auto" ? t("pages.settings.languageAuto") : opt.label}
                  </option>
                ))}
              </select>
            </div>
            {unknownLocale ? (
              <p className="mt-2 text-[0.75rem] text-[#B7791F]" role="alert">
                {t("pages.settings.unknownLocale", { locale })}
              </p>
            ) : resolvedAuto ? (
              <p className="mt-2 text-[0.75rem] text-[var(--t3,#8a8f98)]">
                {t("pages.settings.autoResolved", { locale: resolvedAuto })}
              </p>
            ) : null}

            {/* 原设置弹框三项迁入：紧凑密度 / 跟随日志即时生效并持久化；健康检查未落地保持禁用占位 */}
            <div className="mt-4 flex flex-col divide-y divide-[var(--line,#e6e6e6)] border-t border-[var(--line,#e6e6e6)] pt-1">
              <PrefRow
                label={t("pages.settings.compactDensity")}
                desc={t("pages.settings.compactDensityDesc")}
                checked={shell.compact}
                onChange={shell.setCompact}
              />
              <PrefRow
                label={t("pages.settings.followLogs")}
                desc={t("pages.settings.followLogsDesc")}
                checked={shell.defaultFollow}
                onChange={shell.setDefaultFollow}
              />
              <PrefRow
                label={t("pages.settings.liveHealth")}
                desc={t("pages.settings.liveHealthDesc")}
                checked={false}
                onChange={() => {}}
                disabled
              />
            </div>
          </Card>

          <CloudSettingsCard />

          {/* 1.7 §9.1：导出卡已迁至 /workspaces（工作区包归位） */}
          <UpdateCard />

          <Card className="p-4">
            <h3 className="mb-3 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">{t("pages.settings.about")}</h3>
            <dl className="grid grid-cols-[120px_1fr] gap-y-1.5 text-[0.875rem]">
              <dt className="text-[var(--t3,#8a8f98)]">{t("pages.settings.productVersion")}</dt>
              <dd className="font-mono text-[var(--t1,#222326)]">{state.hello?.product_version}</dd>
              <dt className="text-[var(--t3,#8a8f98)]">{t("pages.settings.engine")}</dt>
              <dd className="font-mono text-[var(--t1,#222326)]">
                {state.hello?.engine} {state.hello?.engine_version}
              </dd>
              <dt className="text-[var(--t3,#8a8f98)]">{t("pages.settings.protocol")}</dt>
              <dd className="font-mono text-[var(--t1,#222326)]">{state.hello?.protocol}</dd>
              <dt className="text-[var(--t3,#8a8f98)]">{t("pages.settings.os")}</dt>
              <dd className="font-mono text-[var(--t1,#222326)]">{state.hello?.os}</dd>
            </dl>
          </Card>
        </div>
      </div>
    </div>
  );
}

export function SettingsPage() {
  return <SettingsPageInner />;
}
