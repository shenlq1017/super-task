import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { CheckCircle2, ChevronDown, Loader2, Terminal, XCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { AiProviderLogo } from "@/components/ai-provider-logo";
import {
  AI_PROVIDER_PRESETS,
  API_PROVIDERS,
  CLI_PROVIDERS,
  isCliProvider,
} from "@/lib/ai-providers";
import { apiAiCliProbe } from "../ipc/api";
import type { AiAuthMethod, AiCliProbeOut, AiProviderKey } from "../ipc/protocol";

export type ConfigForm = {
  id: string | null;
  name: string;
  provider: AiProviderKey;
  authMethod: AiAuthMethod;
  apiKey: string;
  baseUrl: string;
  model: string;
  contextWindow: string;
  timeoutSecs: string;
  maxTokens: string;
  proxyEnabled: boolean;
  proxyUrl: string;
  maxRetries: string;
  cliPath: string;
  /** argv, one token per line — the only way to keep a token containing spaces intact. */
  cliArgs: string;
  /** `NAME=value` per line. */
  cliEnv: string;
};

export function emptyConfigForm(): ConfigForm {
  const preset = AI_PROVIDER_PRESETS["openai-compatible"];
  return {
    id: null,
    name: "",
    provider: "openai-compatible",
    authMethod: "api_key",
    apiKey: "",
    baseUrl: preset.endpoint,
    model: preset.model,
    contextWindow: "",
    timeoutSecs: "120",
    maxTokens: "8192",
    proxyEnabled: false,
    proxyUrl: "",
    maxRetries: "2",
    cliPath: "",
    cliArgs: "",
    cliEnv: "",
  };
}

export function parseCliArgs(text: string): string[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}

export function parseCliEnv(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const eq = trimmed.indexOf("=");
    if (eq <= 0) continue;
    out[trimmed.slice(0, eq).trim()] = trimmed.slice(eq + 1).trim();
  }
  return out;
}

export function formatCliEnv(env: Record<string, string>): string {
  return Object.entries(env)
    .map(([name, value]) => `${name}=${value}`)
    .join("\n");
}

function Field(props: {
  label: string;
  htmlFor?: string;
  hint?: string;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <div className={props.className}>
      <label htmlFor={props.htmlFor} className="text-[0.75rem] text-[var(--t3,#8a8f98)]">
        {props.label}
      </label>
      <div className="mt-1">{props.children}</div>
      {props.hint ? (
        <p className="mt-1 text-[0.68rem] leading-relaxed text-[var(--t3,#8a8f98)]">{props.hint}</p>
      ) : null}
    </div>
  );
}

function Section(props: { title: string; children: React.ReactNode; aside?: React.ReactNode }) {
  return (
    <section>
      <div className="mb-2 flex items-center gap-2">
        <h4 className="text-[0.7rem] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]">
          {props.title}
        </h4>
        {props.aside}
      </div>
      {props.children}
    </section>
  );
}

/** 新增/编辑 AI 配置弹框：HTTP 端点与本地 CLI 两套字段互斥呈现，
 *  高级项默认折叠，避免一屏塞满十几个输入框。 */
export function AiConfigDialog(props: {
  form: ConfigForm | null;
  keySet: boolean;
  busy: boolean;
  saving: boolean;
  error: string | null;
  models: string[];
  onPatch: (patch: Partial<ConfigForm>) => void;
  onProviderChange: (provider: AiProviderKey) => void;
  onFetchModels: () => void;
  onClearKey: () => void;
  onSubmit: (event: React.FormEvent<HTMLFormElement>) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const { form } = props;
  const [advanced, setAdvanced] = useState(false);
  const [probing, setProbing] = useState(false);
  const [probe, setProbe] = useState<AiCliProbeOut | null>(null);

  const provider = form?.provider ?? "openai-compatible";
  const cli = isCliProvider(provider);
  const preset = AI_PROVIDER_PRESETS[provider];

  // A probe belongs to one provider+path pair; drop it as soon as either changes.
  useEffect(() => {
    setProbe(null);
  }, [provider, form?.cliPath]);

  const modelList = useMemo(
    () => (props.models.length ? props.models : cli ? preset.cliArgs && [] : []) || [],
    [props.models, cli, preset],
  );

  const runProbe = async () => {
    if (!form) return;
    setProbing(true);
    try {
      setProbe(await apiAiCliProbe(provider, form.cliPath.trim() || null, parseCliEnv(form.cliEnv)));
    } catch (error) {
      setProbe({
        program: form.cliPath.trim() || preset.cliProgram || provider,
        found: false,
        version: null,
        detail: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setProbing(false);
    }
  };

  return (
    <Dialog open={form != null} onOpenChange={(open) => (!open ? props.onClose() : undefined)}>
      {form ? (
        <DialogContent
          className="max-h-[min(46rem,calc(100vh-3rem))] w-full gap-0 overflow-hidden p-0 sm:max-w-2xl"
          onInteractOutside={(event) => {
            // A half-filled form is easy to lose by a stray click; require Cancel/Esc.
            if (props.saving) event.preventDefault();
          }}
        >
          <form onSubmit={props.onSubmit} noValidate className="flex max-h-[inherit] min-h-0 flex-col">
            <DialogHeader className="gap-1 border-b border-[var(--line,#e6e6e6)] px-5 py-4">
              <DialogTitle className="flex items-center gap-2 text-[0.95rem]">
                <AiProviderLogo provider={provider} className="size-4" />
                {form.id ? t("pages.ai.editConfigTitle", { name: form.name }) : t("pages.ai.addConfigTitle")}
              </DialogTitle>
              <DialogDescription className="text-[0.75rem]">
                {cli ? t("pages.ai.cliDialogHint") : t("pages.ai.apiDialogHint")}
              </DialogDescription>
            </DialogHeader>

            <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
              <div className="flex flex-col gap-5">
                <Section title={t("pages.ai.sectionBasics")}>
                  <div className="grid gap-3 sm:grid-cols-2">
                    <Field label={t("pages.ai.configName")} htmlFor="ai-name">
                      <Input
                        id="ai-name"
                        value={form.name}
                        onChange={(e) => props.onPatch({ name: e.target.value })}
                        placeholder={t("pages.ai.configNamePlaceholder")}
                      />
                    </Field>
                    <Field label={t("pages.ai.provider")}>
                      <Select value={provider} onValueChange={(v) => props.onProviderChange(v as AiProviderKey)}>
                        <SelectTrigger
                          size="sm"
                          className="h-9 w-full cursor-pointer border-[var(--line-strong,#d0d6e0)]"
                          aria-label={t("pages.ai.provider")}
                        >
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent position="popper" className="max-h-72">
                          <SelectGroup>
                            <SelectLabel>{t("pages.ai.providerGroupApi")}</SelectLabel>
                            {API_PROVIDERS.map((key) => (
                              <SelectItem key={key} value={key} className="cursor-pointer">
                                <AiProviderLogo provider={key} />
                                {AI_PROVIDER_PRESETS[key].label}
                              </SelectItem>
                            ))}
                          </SelectGroup>
                          <SelectSeparator />
                          <SelectGroup>
                            <SelectLabel>{t("pages.ai.providerGroupCli")}</SelectLabel>
                            {CLI_PROVIDERS.map((key) => (
                              <SelectItem key={key} value={key} className="cursor-pointer">
                                <AiProviderLogo provider={key} />
                                {AI_PROVIDER_PRESETS[key].label}
                              </SelectItem>
                            ))}
                          </SelectGroup>
                          <SelectSeparator />
                          <SelectItem value="custom" className="cursor-pointer">
                            <AiProviderLogo provider="custom" />
                            {AI_PROVIDER_PRESETS.custom.label}
                          </SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                  </div>
                </Section>

                {cli ? (
                  <Section
                    title={t("pages.ai.sectionCli")}
                    aside={
                      <Badge variant="secondary" className="gap-1 font-mono text-[10px]">
                        <Terminal className="size-3" />
                        {preset.cliProgram}
                      </Badge>
                    }
                  >
                    <div className="flex flex-col gap-3">
                      <Field
                        label={t("pages.ai.cliPath")}
                        htmlFor="ai-cli-path"
                        hint={t("pages.ai.cliPathHint", { program: preset.cliProgram })}
                      >
                        <div className="flex gap-1">
                          <Input
                            id="ai-cli-path"
                            className="flex-1 font-mono"
                            value={form.cliPath}
                            onChange={(e) => props.onPatch({ cliPath: e.target.value })}
                            placeholder={preset.cliProgram}
                            spellCheck={false}
                          />
                          <Button
                            variant="soft"
                            size="sm"
                            type="button"
                            className="gap-1"
                            onClick={() => void runProbe()}
                            disabled={probing}
                          >
                            {probing ? <Loader2 className="size-3.5 animate-spin" /> : null}
                            {t("pages.ai.cliProbe")}
                          </Button>
                        </div>
                      </Field>
                      {probe ? (
                        <p
                          className={cn(
                            "flex items-start gap-1.5 rounded-[var(--r-sm,8px)] px-2.5 py-2 text-[0.72rem] leading-relaxed",
                            probe.found
                              ? "bg-[var(--st-ok-tint,#e9f7ed)] text-[#187A3D]"
                              : "bg-[var(--st-danger-tint,#fdecec)] text-[var(--st-danger,#dc2626)]",
                          )}
                          role="status"
                        >
                          {probe.found ? (
                            <CheckCircle2 className="mt-px size-3.5 shrink-0" />
                          ) : (
                            <XCircle className="mt-px size-3.5 shrink-0" />
                          )}
                          <span className="min-w-0 break-words font-mono">
                            {probe.found
                              ? t("pages.ai.cliFound", { program: probe.program, version: probe.version ?? "" })
                              : t("pages.ai.cliNotFound", { program: probe.program, detail: probe.detail ?? "" })}
                          </span>
                        </p>
                      ) : null}
                      <Field label={t("pages.ai.cliArgs")} htmlFor="ai-cli-args" hint={t("pages.ai.cliArgsHint")}>
                        <textarea
                          id="ai-cli-args"
                          value={form.cliArgs}
                          onChange={(e) => props.onPatch({ cliArgs: e.target.value })}
                          spellCheck={false}
                          rows={Math.min(8, Math.max(3, form.cliArgs.split("\n").length))}
                          className="w-full resize-y rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-transparent px-2.5 py-1.5 font-mono text-[0.75rem] outline-none focus-visible:border-[var(--st-accent,#5e6ad2)]"
                        />
                      </Field>
                      <Field label={t("pages.ai.cliEnv")} htmlFor="ai-cli-env" hint={t("pages.ai.cliEnvHint")}>
                        <textarea
                          id="ai-cli-env"
                          value={form.cliEnv}
                          onChange={(e) => props.onPatch({ cliEnv: e.target.value })}
                          spellCheck={false}
                          rows={2}
                          placeholder="HTTPS_PROXY=http://127.0.0.1:7890"
                          className="w-full resize-y rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] bg-transparent px-2.5 py-1.5 font-mono text-[0.75rem] outline-none focus-visible:border-[var(--st-accent,#5e6ad2)]"
                        />
                      </Field>
                    </div>
                  </Section>
                ) : (
                  <Section title={t("pages.ai.sectionConnection")}>
                    <div className="grid gap-3 sm:grid-cols-2">
                      <Field label={t("pages.ai.baseUrl")} htmlFor="ai-base-url" className="sm:col-span-2">
                        <Input
                          id="ai-base-url"
                          className="font-mono"
                          value={form.baseUrl}
                          onChange={(e) => props.onPatch({ baseUrl: e.target.value })}
                          type="url"
                          inputMode="url"
                          spellCheck={false}
                          placeholder="https://api.openai.com/v1"
                        />
                      </Field>
                      <Field label={t("pages.ai.auth")}>
                        <Select
                          value={form.authMethod}
                          onValueChange={(v) => props.onPatch({ authMethod: v as AiAuthMethod })}
                        >
                          <SelectTrigger
                            size="sm"
                            className="h-9 w-full cursor-pointer border-[var(--line-strong,#d0d6e0)]"
                            aria-label={t("pages.ai.auth")}
                          >
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent position="popper">
                            <SelectItem value="api_key" className="cursor-pointer">API Key</SelectItem>
                            <SelectItem value="bearer" className="cursor-pointer">Bearer</SelectItem>
                          </SelectContent>
                        </Select>
                      </Field>
                      <Field
                        label={t("pages.ai.apiKey")}
                        htmlFor="ai-api-key"
                        hint={preset.keyOptional ? t("pages.ai.keyOptionalHint") : undefined}
                      >
                        <Input
                          id="ai-api-key"
                          type="password"
                          value={form.apiKey}
                          onChange={(e) => props.onPatch({ apiKey: e.target.value })}
                          placeholder={t("pages.ai.apiKeyPlaceholder")}
                          autoComplete="off"
                        />
                      </Field>
                    </div>
                  </Section>
                )}

                <Section title={t("pages.ai.sectionModel")}>
                  <Field
                    label={t("pages.ai.model")}
                    htmlFor="ai-model"
                    hint={cli ? t("pages.ai.cliModelHint") : undefined}
                  >
                    <div className="flex gap-1">
                      <Input
                        id="ai-model"
                        className="flex-1 font-mono"
                        value={form.model}
                        onChange={(e) => props.onPatch({ model: e.target.value })}
                        list="ai-models-datalist"
                        spellCheck={false}
                      />
                      <Button
                        variant="soft"
                        size="sm"
                        type="button"
                        onClick={props.onFetchModels}
                        disabled={props.busy}
                      >
                        {t("pages.ai.fetchModels")}
                      </Button>
                    </div>
                    <datalist id="ai-models-datalist">
                      {modelList.map((m) => (
                        <option key={m} value={m} />
                      ))}
                    </datalist>
                  </Field>
                </Section>

                <section className="rounded-[var(--r-sm,8px)] border border-[var(--line,#e6e6e6)]">
                  <button
                    type="button"
                    onClick={() => setAdvanced((v) => !v)}
                    aria-expanded={advanced}
                    className="flex w-full items-center gap-2 px-3 py-2 text-left text-[0.75rem] font-semibold text-[var(--t2,#62666d)] transition-colors hover:bg-[var(--surface-2,#f3f4f5)]"
                  >
                    <ChevronDown className={cn("size-3.5 transition-transform", advanced && "rotate-180")} />
                    {t("pages.ai.sectionAdvanced")}
                    <span className="ml-auto font-mono text-[0.68rem] font-normal text-[var(--t3,#8a8f98)]">
                      {t("pages.ai.advancedSummary", {
                        timeout: form.timeoutSecs || "-",
                        retries: form.maxRetries || "0",
                      })}
                    </span>
                  </button>
                  {advanced ? (
                    <div className="grid gap-3 border-t border-[var(--line,#e6e6e6)] p-3 sm:grid-cols-2">
                      <Field label={t("pages.ai.timeout")} htmlFor="ai-timeout">
                        <Input
                          id="ai-timeout"
                          className="font-mono"
                          type="number"
                          min={1}
                          max={600}
                          value={form.timeoutSecs}
                          onChange={(e) => props.onPatch({ timeoutSecs: e.target.value })}
                        />
                      </Field>
                      <Field label={t("pages.ai.maxRetries")} htmlFor="ai-max-retries" hint={t("pages.ai.maxRetriesHint")}>
                        <Input
                          id="ai-max-retries"
                          className="font-mono"
                          type="number"
                          min={0}
                          max={10}
                          value={form.maxRetries}
                          onChange={(e) => props.onPatch({ maxRetries: e.target.value })}
                        />
                      </Field>
                      {cli ? null : (
                        <>
                          <Field label={t("pages.ai.maxTokens")} htmlFor="ai-max-tokens">
                            <Input
                              id="ai-max-tokens"
                              className="font-mono"
                              type="number"
                              min={1}
                              max={32768}
                              value={form.maxTokens}
                              onChange={(e) => props.onPatch({ maxTokens: e.target.value })}
                            />
                          </Field>
                          <Field
                            label={t("pages.ai.contextWindow")}
                            htmlFor="ai-context-window"
                            hint={t("pages.ai.contextWindowHint")}
                          >
                            <Input
                              id="ai-context-window"
                              className="font-mono"
                              type="number"
                              min={0}
                              value={form.contextWindow}
                              onChange={(e) => props.onPatch({ contextWindow: e.target.value })}
                              placeholder="128000"
                            />
                          </Field>
                          <div className="rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] p-3 sm:col-span-2">
                            <label className="flex cursor-pointer items-center gap-2 text-[0.78rem] text-[var(--t1,#222326)]">
                              <input
                                type="checkbox"
                                checked={form.proxyEnabled}
                                onChange={(e) => props.onPatch({ proxyEnabled: e.target.checked })}
                              />
                              {t("pages.ai.proxy")}
                            </label>
                            {form.proxyEnabled ? (
                              <Input
                                className="mt-2 font-mono"
                                value={form.proxyUrl}
                                onChange={(e) => props.onPatch({ proxyUrl: e.target.value })}
                                placeholder="127.0.0.1:7890"
                                aria-label={t("pages.ai.proxyUrl")}
                              />
                            ) : null}
                            <p className="mt-1 text-[0.68rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.proxyHint")}</p>
                          </div>
                        </>
                      )}
                    </div>
                  ) : null}
                </section>

                {props.error ? (
                  <p
                    className="rounded-[var(--r-sm,8px)] border border-red-200 bg-[var(--st-danger-tint,#fdecec)] px-3 py-2 text-[0.75rem] text-[#DC2626]"
                    role="alert"
                  >
                    {props.error}
                  </p>
                ) : null}
              </div>
            </div>

            <DialogFooter className="mx-0 mb-0 items-center gap-2 px-5 sm:justify-between">
              {props.keySet && !cli ? (
                <Button
                  variant="ghost"
                  size="sm"
                  type="button"
                  className="text-[var(--st-danger,#dc2626)] hover:bg-[#FDECEC]"
                  onClick={props.onClearKey}
                  disabled={props.busy}
                >
                  {t("pages.ai.clearKey")}
                </Button>
              ) : (
                <span />
              )}
              <span className="flex gap-2">
                <Button variant="outline" size="sm" type="button" onClick={props.onClose} disabled={props.saving}>
                  {t("common.cancel")}
                </Button>
                <Button variant="success" size="sm" type="submit" disabled={props.busy}>
                  {props.saving ? t("common.saving") : t("common.save")}
                </Button>
              </span>
            </DialogFooter>
          </form>
        </DialogContent>
      ) : null}
    </Dialog>
  );
}
