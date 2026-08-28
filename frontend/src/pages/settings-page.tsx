import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { cn } from "@/lib/utils";
import { useSession } from "../providers/session-provider";
import { useRuntime } from "../providers/runtime-provider";
import { useOperations } from "../providers/operation-provider";
import { apiSavePrefs, apiUpdateCheck, apiUpdateInstall } from "../ipc/api";
import { IpcFailure } from "../ipc/protocol";
import type { UpdateCheckResult } from "../ipc/protocol";
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
}: {
  label: string;
  desc: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="flex items-center justify-between gap-4 py-1.5 text-[0.875rem] text-[var(--t1,#222326)]">
      <span>
        {label}
        <span className="block text-[0.75rem] leading-relaxed text-[var(--t3,#8a8f98)]">{desc}</span>
      </span>
      <input
        type="checkbox"
        checked={checked}
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

export function SettingsPage() {
  const { state } = useSession();
  const { toast } = useToast();
  const { t } = useTranslation();
  const [theme, setTheme] = useState(state.app?.prefs.theme ?? "light");
  const [locale, setLocale] = useState<string>(state.app?.prefs?.locale ?? "auto");
  const [restore, setRestore] = useState(state.app?.prefs.restoreLast ?? true);
  const [closeToTray, setCloseToTray] = useState(state.app?.prefs.closeToTray ?? true);
  const [startOnLogin, setStartOnLogin] = useState(state.app?.prefs.startOnLogin ?? false);
  const [updateCheck, setUpdateCheck] = useState(state.app?.prefs.updateCheck ?? true);

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
          </Card>

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
