import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Bot, PlugZap, Plus, RefreshCw, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input, Textarea } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useToast } from "@/components/ui/toast";
import {
  apiAiComplete,
  apiAiConfigDefault,
  apiAiConfigDelete,
  apiAiConfigSave,
  apiAiInstructionsSave,
  apiAiModels,
  apiAiStatus,
  apiAiTemplateDelete,
  apiAiTemplateSave,
} from "../ipc/api";
import type {
  AiAuthMethod,
  AiConfigSummary,
  AiProviderKey,
  AiStatusOut,
  AiTemplate,
} from "../ipc/protocol";
import { IpcFailure } from "../ipc/protocol";
import { errorDisplayText } from "@/lib/error-messages";

type Action = "status" | "save" | "test" | "instructions" | "template" | null;

/** provider 预设（与 core PROVIDER_PRESETS 同源；CLI provider 不做，见调研文档 reject 结论）。 */
const AI_PROVIDER_PRESETS: Record<
  AiProviderKey,
  { label: string; endpoint: string; model: string; keyOptional?: boolean }
> = {
  "openai-compatible": { label: "OpenAI Compatible", endpoint: "https://api.openai.com/v1", model: "gpt-4o-mini" },
  claude: { label: "Claude (Anthropic)", endpoint: "https://api.anthropic.com", model: "claude-sonnet-4-5" },
  deepseek: { label: "DeepSeek", endpoint: "https://api.deepseek.com", model: "deepseek-chat" },
  qwen: { label: "Qwen", endpoint: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen-plus" },
  minimax: { label: "MiniMax", endpoint: "https://api.minimaxi.com/v1", model: "MiniMax-Text-01" },
  gemini: { label: "Gemini (OpenAI 兼容)", endpoint: "https://generativelanguage.googleapis.com/v1beta/openai", model: "gemini-2.0-flash" },
  ollama: { label: "Ollama", endpoint: "http://localhost:11434/v1", model: "qwen2.5:7b", keyOptional: true },
  custom: { label: "Custom", endpoint: "", model: "" },
};

type ConfigForm = {
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
};

function emptyForm(): ConfigForm {
  return {
    id: null,
    name: "",
    provider: "openai-compatible",
    authMethod: "api_key",
    apiKey: "",
    baseUrl: "",
    model: "",
    contextWindow: "",
    timeoutSecs: "120",
    maxTokens: "8192",
    proxyEnabled: false,
    proxyUrl: "",
    maxRetries: "2",
  };
}

function aiError(error: unknown): string {
  return error instanceof IpcFailure
    ? errorDisplayText(error.code, error.message)
    : error instanceof Error
      ? error.message
      : String(error);
}

/** /ai（v2.1 规格 §7 + 截图对齐）：命名多配置 + 全局指令 + Prompt 模板 + 用量 + 隐私说明。 */
export function AiPage() {
  const { t } = useTranslation();
  const { toast } = useToast();
  const [status, setStatus] = useState<AiStatusOut | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [action, setAction] = useState<Action>(null);
  // ok = 请求链路成功（body 是模型返回，不代表模型认可内容）；!ok = 连接/请求失败
  const [testResult, setTestResult] = useState<{ ok: boolean; subtitle?: string } | null>(null);

  // 配置编辑
  const [form, setForm] = useState<ConfigForm | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [models, setModels] = useState<string[]>([]);
  const [deleting, setDeleting] = useState<AiConfigSummary | null>(null);

  // 全局指令
  const [instructions, setInstructions] = useState("");

  // 模板编辑
  const [tplForm, setTplForm] = useState<{ id: string | null; name: string; content: string; enabled: boolean } | null>(null);
  const [deletingTpl, setDeletingTpl] = useState<AiTemplate | null>(null);

  const busy = action !== null;

  const refresh = async (silent = false) => {
    if (!silent) setAction("status");
    setLoadFailed(false);
    try {
      const next = await apiAiStatus();
      setStatus(next);
      setInstructions(next.global_instructions ?? "");
      return next;
    } catch (error) {
      setLoadFailed(true);
      if (!silent) toast(aiError(error), "err");
    } finally {
      if (!silent) setAction(null);
    }
  };

  useEffect(() => {
    void refresh(true);
    // Bootstrap once; action handlers own subsequent refreshes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const patch = (p: Partial<ConfigForm>) => setForm((f) => (f ? { ...f, ...p } : f));

  const startCreate = () => {
    setFormError(null);
    setModels([]);
    setForm(emptyForm());
  };

  const startEdit = (c: AiConfigSummary) => {
    setFormError(null);
    setModels([]);
    setForm({
      id: c.id,
      name: c.name,
      provider: (c.provider as AiProviderKey) in AI_PROVIDER_PRESETS ? (c.provider as AiProviderKey) : "custom",
      authMethod: "api_key",
      apiKey: "",
      baseUrl: c.base_url,
      model: c.model,
      contextWindow: "",
      timeoutSecs: "120",
      maxTokens: "8192",
      proxyEnabled: false,
      proxyUrl: "",
      maxRetries: "2",
    });
  };

  const applyPreset = (provider: AiProviderKey) => {
    setForm((f) => {
      if (!f) return f;
      const prevPreset = AI_PROVIDER_PRESETS[f.provider];
      const preset = AI_PROVIDER_PRESETS[provider];
      const baseUrl = !f.baseUrl || f.baseUrl === prevPreset.endpoint ? preset.endpoint : f.baseUrl;
      const model = !f.model || f.model === prevPreset.model ? preset.model : f.model;
      return { ...f, provider, baseUrl, model };
    });
  };

  const fetchModels = async () => {
    setAction("template");
    try {
      const list = await apiAiModels(form?.id ?? undefined);
      setModels(list);
      toast(t("pages.ai.modelsFetched", { n: list.length }), "ok");
    } catch (error) {
      toast(aiError(error), "err");
    } finally {
      setAction(null);
    }
  };

  const saveConfig = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!form) return;
    const name = form.name.trim();
    const baseUrl = form.baseUrl.trim().replace(/\/$/, "");
    const model = form.model.trim();
    if (!name) return setFormError(t("pages.ai.nameRequired"));
    if (!/^https?:\/\/[^\s/]+(?:\/[^\s]*)?$/.test(baseUrl)) return setFormError(t("pages.ai.baseUrlInvalid"));
    if (!model) return setFormError(t("pages.ai.modelRequired"));
    setFormError(null);
    setAction("save");
    try {
      await apiAiConfigSave({
        id: form.id,
        name,
        baseUrl,
        model,
        provider: form.provider,
        authMethod: form.authMethod,
        timeoutSecs: Number(form.timeoutSecs) || undefined,
        maxTokens: Number(form.maxTokens) || undefined,
        contextWindow: form.contextWindow ? Number(form.contextWindow) : undefined,
        proxyEnabled: form.proxyEnabled,
        proxyUrl: form.proxyUrl.trim() || undefined,
        maxRetries: form.maxRetries === "" ? undefined : Number(form.maxRetries),
        // 输入框留空 = 不改动已存 key（api_key 缺省）；「清除 Key」按钮才发空串
        apiKey: form.apiKey || undefined,
      });
      setForm(null);
      await refresh(true);
      toast(t("pages.ai.saved"), "ok");
    } catch (error) {
      setFormError(aiError(error));
    } finally {
      setAction(null);
    }
  };

  const clearKey = async () => {
    setAction("save");
    try {
      await apiAiConfigSave({ id: form?.id ?? undefined, name: form?.name.trim() || "key-clear", baseUrl: form?.baseUrl, model: form?.model.trim() || "-", apiKey: "" });
      await refresh(true);
      toast(t("pages.ai.keyCleared"), "ok");
    } catch (error) {
      toast(aiError(error), "err");
    } finally {
      setAction(null);
    }
  };

  const setDefault = async (id: string) => {
    setAction("save");
    try {
      await apiAiConfigDefault(id);
      await refresh(true);
      toast(t("pages.ai.defaultSet"), "ok");
    } catch (error) {
      toast(aiError(error), "err");
    } finally {
      setAction(null);
    }
  };

  const deleteConfig = async (id: string) => {
    setAction("save");
    try {
      await apiAiConfigDelete(id);
      await refresh(true);
      toast(t("pages.ai.configDeleted"), "ok");
    } catch (error) {
      toast(aiError(error), "err");
    } finally {
      setAction(null);
    }
  };

  const test = async () => {
    setAction("test");
    setTestResult(null);
    const started = performance.now();
    try {
      const out = await apiAiComplete("test_connection", {});
      const ms = Math.max(1, Math.round(performance.now() - started));
      setTestResult({
        ok: true,
        subtitle: t("pages.ai.testOkDetail", { model: out.model, ms }),
      });
      await refresh(true);
      toast(t("pages.ai.testOk"), "ok");
    } catch (error) {
      setTestResult({ ok: false, subtitle: aiError(error) });
      toast(t("pages.ai.testFailed"), "err");
    } finally {
      setAction(null);
    }
  };

  const saveInstructions = async () => {
    setAction("instructions");
    try {
      await apiAiInstructionsSave(instructions);
      await refresh(true);
      toast(t("pages.ai.instructionsSaved"), "ok");
    } catch (error) {
      toast(aiError(error), "err");
    } finally {
      setAction(null);
    }
  };

  const saveTemplate = async () => {
    if (!tplForm) return;
    setAction("template");
    try {
      await apiAiTemplateSave({
        id: tplForm.id,
        name: tplForm.name,
        content: tplForm.content,
        enabled: tplForm.enabled,
      });
      setTplForm(null);
      await refresh(true);
      toast(t("pages.ai.templateSaved"), "ok");
    } catch (error) {
      toast(aiError(error), "err");
    } finally {
      setAction(null);
    }
  };

  const toggleTemplate = async (tpl: AiTemplate) => {
    setAction("template");
    try {
      await apiAiTemplateSave({ id: tpl.id, name: tpl.name, content: tpl.content, enabled: !tpl.enabled });
      await refresh(true);
    } catch (error) {
      toast(aiError(error), "err");
    } finally {
      setAction(null);
    }
  };

  const deleteTemplate = async (id: string) => {
    setAction("template");
    try {
      await apiAiTemplateDelete(id);
      await refresh(true);
      toast(t("pages.ai.templateDeleted"), "ok");
    } catch (error) {
      toast(aiError(error), "err");
    } finally {
      setAction(null);
    }
  };

  const providerLabel = (key: string) => AI_PROVIDER_PRESETS[key as AiProviderKey]?.label ?? key;
  const hasDefault = !!status?.default_id;
  const instructionsLen = useMemo(() => [...instructions].length, [instructions]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto flex max-w-3xl flex-col gap-4">
          <div className="flex items-start justify-between gap-3">
            <div>
              <h2 className="text-[1.05rem] font-bold tracking-tight text-[var(--t1,#222326)]">{t("pages.ai.title")}</h2>
              <p className="mt-0.5 text-[0.78rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.subtitle")}</p>
            </div>
            <Button variant="soft" size="sm" className="gap-1" onClick={() => void refresh()} disabled={busy} aria-label={t("pages.ai.refreshStatus")}>
              <RefreshCw className={action === "status" ? "size-3.5 animate-spin" : "size-3.5"} /> {t("common.refresh")}
            </Button>
          </div>

          {/* 配置列表（dbx 风格：命名配置 + 设为默认 / 编辑 / 删除 + 新增） */}
          <Card className="p-4">
            <div className="mb-3 flex items-center gap-2">
              <Bot className="size-4 text-[var(--st-accent,#5e6ad2)]" />
              <h3 className="text-[0.875rem] font-semibold text-[var(--t1,#222326)]">{t("pages.ai.configListTitle")}</h3>
              {status?.key_set ? <Badge variant="default">{t("pages.ai.keySet")}</Badge> : <Badge variant="outline">{t("pages.ai.keyNotSet")}</Badge>}
              <Button variant="default" size="sm" className="ml-auto gap-1" onClick={startCreate}>
                <Plus className="size-3.5" /> {t("pages.ai.addConfig")}
              </Button>
            </div>
            {status && status.configs.length > 0 ? (
              <div className="flex flex-col gap-2">
                {status.configs.map((c) => (
                  <div
                    key={c.id}
                    className="flex flex-wrap items-center gap-2 rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] px-3 py-2"
                  >
                    <span className="font-mono text-[0.8rem] font-semibold text-[var(--t1,#222326)]">{c.name}</span>
                    <Badge variant="secondary" className="text-[10px]">{providerLabel(c.provider)}</Badge>
                    <span className="min-w-0 truncate font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]" title={`${c.base_url} · ${c.model}`}>
                      {c.model}
                    </span>
                    {c.is_default ? <Badge variant="default" className="text-[10px]">{t("pages.ai.defaultBadge")}</Badge> : null}
                    <span className="ml-auto flex shrink-0 gap-1">
                      {!c.is_default ? (
                        <Button variant="outline" size="sm" onClick={() => void setDefault(c.id)} disabled={busy}>
                          {t("pages.ai.setDefault")}
                        </Button>
                      ) : null}
                      <Button variant="outline" size="sm" onClick={() => startEdit(c)} disabled={busy}>
                        {t("pages.ai.editConfig")}
                      </Button>
                      <Button variant="ghost" size="sm" className="text-[var(--st-danger,#dc2626)] hover:bg-[#FDECEC] hover:text-[var(--st-danger,#dc2626)]" onClick={() => setDeleting(c)} disabled={busy}>
                        {t("common.delete")}
                      </Button>
                    </span>
                  </div>
                ))}
              </div>
            ) : (
              <p className="py-2 text-[0.78rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.noConfigs")}</p>
            )}
            <div className="mt-3 flex flex-wrap items-center gap-2 border-t border-[var(--line,#e6e6e6)] pt-3">
              <Button variant="soft" size="sm" className="gap-1" onClick={() => void test()} disabled={busy || !hasDefault}>
                <PlugZap className={action === "test" ? "size-3.5 animate-pulse" : "size-3.5"} />
                {action === "test" ? t("pages.ai.testing") : t("pages.ai.test")}
              </Button>
              <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.testHint")}</span>
            </div>
            {action === "test" || testResult ? (
              <div
                className={`mt-3 rounded-[var(--r-sm,8px)] border p-3 ${
                  testResult?.ok === false
                    ? "border-[#F0C9C9] bg-[#FDECEC]"
                    : testResult?.ok
                      ? "border-[#BFE0CA] bg-[#E9F7ED]"
                      : "border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)]"
                }`}
              >
                <p
                  className={`text-[0.75rem] font-semibold ${
                    testResult?.ok === false
                      ? "text-[var(--st-danger,#dc2626)]"
                      : testResult?.ok
                        ? "text-[#187A3D]"
                        : "text-[var(--t2,#62666d)]"
                  }`}
                >
                  {action === "test"
                    ? t("pages.ai.testing")
                    : testResult?.ok
                      ? `✓ ${t("pages.ai.testOk")}`
                      : `✗ ${t("pages.ai.testFailed")}`}
                </p>
                {testResult?.subtitle ? (
                  <p className="mt-1 font-mono text-[0.72rem] leading-relaxed text-[var(--t1,#222326)]">
                    {testResult.subtitle}
                  </p>
                ) : null}
              </div>
            ) : null}
            <p className="mt-3 text-[0.72rem] leading-relaxed text-[var(--t3,#8a8f98)]">{t("pages.ai.privacyNote")}</p>
          </Card>

          {/* 配置表单（新增/编辑） */}
          {form ? (
            <Card className="border-[rgb(94_106_210_/_0.35)] p-4">
              <h3 className="mb-3 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">
                {form.id ? t("pages.ai.editConfigTitle", { name: form.name }) : t("pages.ai.addConfigTitle")}
              </h3>
              <form onSubmit={(event) => void saveConfig(event)} noValidate>
                <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                  <div>
                    <label htmlFor="ai-name" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.configName")}</label>
                    <Input id="ai-name" className="mt-1" value={form.name} onChange={(e) => patch({ name: e.target.value })} placeholder={t("pages.ai.configNamePlaceholder")} />
                  </div>
                  <div>
                    <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.provider")}</label>
                    <Select value={form.provider} onValueChange={(v) => applyPreset(v as AiProviderKey)}>
                      <SelectTrigger size="sm" className="mt-1 h-9 w-full cursor-pointer border-[var(--line-strong,#d0d6e0)]" aria-label={t("pages.ai.provider")}>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {(Object.keys(AI_PROVIDER_PRESETS) as AiProviderKey[]).map((k) => (
                          <SelectItem key={k} value={k} className="cursor-pointer">
                            {AI_PROVIDER_PRESETS[k].label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                  <div>
                    <label className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.auth")}</label>
                    <Select value={form.authMethod} onValueChange={(v) => patch({ authMethod: v as AiAuthMethod })}>
                      <SelectTrigger size="sm" className="mt-1 h-9 w-full cursor-pointer border-[var(--line-strong,#d0d6e0)]" aria-label={t("pages.ai.auth")}>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="api_key" className="cursor-pointer">API Key</SelectItem>
                        <SelectItem value="bearer" className="cursor-pointer">Bearer</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <div>
                    <label htmlFor="ai-api-key" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.apiKey")}</label>
                    <Input id="ai-api-key" className="mt-1" type="password" value={form.apiKey} onChange={(e) => patch({ apiKey: e.target.value })} placeholder={t("pages.ai.apiKeyPlaceholder")} autoComplete="off" />
                  </div>
                  <div className="sm:col-span-2">
                    <label htmlFor="ai-base-url" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.baseUrl")}</label>
                    <Input id="ai-base-url" className="mt-1 font-mono" value={form.baseUrl} onChange={(e) => patch({ baseUrl: e.target.value })} type="url" inputMode="url" placeholder="https://api.openai.com/v1" />
                  </div>
                  <div>
                    <label htmlFor="ai-model" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.model")}</label>
                    <div className="mt-1 flex gap-1">
                      <Input id="ai-model" className="flex-1 font-mono" value={form.model} onChange={(e) => patch({ model: e.target.value })} list="ai-models-datalist" />
                      <Button variant="soft" size="sm" type="button" onClick={() => void fetchModels()} disabled={busy} title={t("pages.ai.fetchModels")}>
                        {t("pages.ai.fetchModels")}
                      </Button>
                    </div>
                    <datalist id="ai-models-datalist">
                      {models.map((m) => (
                        <option key={m} value={m} />
                      ))}
                    </datalist>
                  </div>
                  <div>
                    <label htmlFor="ai-context-window" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.contextWindow")}</label>
                    <Input id="ai-context-window" className="mt-1 font-mono" type="number" min={0} value={form.contextWindow} onChange={(e) => patch({ contextWindow: e.target.value })} placeholder="128000" />
                    <p className="mt-1 text-[0.68rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.contextWindowHint")}</p>
                  </div>
                  <div>
                    <label htmlFor="ai-timeout" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.timeout")}</label>
                    <Input id="ai-timeout" className="mt-1 font-mono" type="number" min={1} max={600} value={form.timeoutSecs} onChange={(e) => patch({ timeoutSecs: e.target.value })} />
                  </div>
                  <div>
                    <label htmlFor="ai-max-tokens" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.maxTokens")}</label>
                    <Input id="ai-max-tokens" className="mt-1 font-mono" type="number" min={1} max={32768} value={form.maxTokens} onChange={(e) => patch({ maxTokens: e.target.value })} />
                  </div>
                  <div>
                    <label htmlFor="ai-max-retries" className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.maxRetries")}</label>
                    <Input id="ai-max-retries" className="mt-1 font-mono" type="number" min={0} max={10} value={form.maxRetries} onChange={(e) => patch({ maxRetries: e.target.value })} />
                    <p className="mt-1 text-[0.68rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.maxRetriesHint")}</p>
                  </div>
                  <div className="sm:col-span-2 rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] p-3">
                    <label className="flex cursor-pointer items-center gap-2 text-[0.78rem] text-[var(--t1,#222326)]">
                      <input type="checkbox" checked={form.proxyEnabled} onChange={(e) => patch({ proxyEnabled: e.target.checked })} />
                      {t("pages.ai.proxy")}
                    </label>
                    {form.proxyEnabled ? (
                      <Input className="mt-2 font-mono" value={form.proxyUrl} onChange={(e) => patch({ proxyUrl: e.target.value })} placeholder="127.0.0.1:7890" aria-label={t("pages.ai.proxyUrl")} />
                    ) : null}
                    <p className="mt-1 text-[0.68rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.proxyHint")}</p>
                  </div>
                </div>
                {formError ? <p className="mt-3 rounded-[var(--r-sm,8px)] border border-red-200 bg-[var(--st-danger-tint,#fdecec)] px-3 py-2 text-[0.75rem] text-[#DC2626]" role="alert">{formError}</p> : null}
                <div className="mt-3 flex flex-wrap items-center gap-2">
                  <Button variant="success" size="sm" type="submit" disabled={busy}>
                    {action === "save" ? t("common.saving") : t("common.save")}
                  </Button>
                  <Button variant="outline" size="sm" type="button" onClick={() => setForm(null)} disabled={busy}>
                    {t("common.cancel")}
                  </Button>
                  {status?.key_set ? (
                    <Button variant="ghost" size="sm" type="button" className="text-[var(--st-danger,#dc2626)] hover:bg-[#FDECEC]" onClick={() => void clearKey()} disabled={busy}>
                      {t("pages.ai.clearKey")}
                    </Button>
                  ) : null}
                </div>
              </form>
            </Card>
          ) : null}

          {/* 全局自定义指令 */}
          <Card className="p-4">
            <h3 className="text-[0.875rem] font-semibold text-[var(--t1,#222326)]">{t("pages.ai.instructionsTitle")}</h3>
            <p className="mb-2 mt-0.5 text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.instructionsHint")}</p>
            <Textarea
              value={instructions}
              onChange={(e) => setInstructions(e.target.value)}
              placeholder={t("pages.ai.instructionsPlaceholder")}
              className="min-h-24 resize-y"
              aria-label={t("pages.ai.instructionsTitle")}
            />
            <div className="mt-2 flex items-center gap-2">
              <span className="font-mono text-[0.7rem] text-[var(--t3,#8a8f98)]">{instructionsLen}/8000</span>
              <Button variant="success" size="sm" className="ml-auto" onClick={() => void saveInstructions()} disabled={busy || instructionsLen > 8000}>
                {action === "instructions" ? t("common.saving") : t("common.save")}
              </Button>
            </div>
          </Card>

          {/* 场景 Prompt 模板 */}
          <Card className="p-4">
            <div className="mb-2 flex items-center gap-2">
              <Sparkles className="size-4 text-[var(--st-accent,#5e6ad2)]" />
              <h3 className="text-[0.875rem] font-semibold text-[var(--t1,#222326)]">{t("pages.ai.templatesTitle")}</h3>
              <span className="text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.templatesHint")}</span>
              <Button variant="default" size="sm" className="ml-auto gap-1" onClick={() => setTplForm({ id: null, name: "", content: "", enabled: true })}>
                <Plus className="size-3.5" /> {t("pages.ai.newTemplate")}
              </Button>
            </div>
            {tplForm ? (
              <div className="mb-3 rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] p-3">
                <Input value={tplForm.name} onChange={(e) => setTplForm({ ...tplForm, name: e.target.value })} placeholder={t("pages.ai.templateNamePlaceholder")} aria-label={t("pages.ai.templateNamePlaceholder")} className="max-w-sm" />
                <Textarea
                  value={tplForm.content}
                  onChange={(e) => setTplForm({ ...tplForm, content: e.target.value })}
                  placeholder={t("pages.ai.templateContentPlaceholder")}
                  className="mt-2 min-h-20 resize-y"
                  aria-label={t("pages.ai.templateContentPlaceholder")}
                />
                <div className="mt-2 flex items-center gap-2">
                  <label className="flex cursor-pointer items-center gap-1.5 text-[0.75rem] text-[var(--t2,#62666d)]">
                    <input type="checkbox" checked={tplForm.enabled} onChange={(e) => setTplForm({ ...tplForm, enabled: e.target.checked })} />
                    {t("pages.ai.templateEnabled")}
                  </label>
                  <span className="font-mono text-[0.7rem] text-[var(--t3,#8a8f98)]">{[...tplForm.content].length}/8000</span>
                  <Button variant="success" size="sm" className="ml-auto" onClick={() => void saveTemplate()} disabled={busy}>
                    {t("common.save")}
                  </Button>
                  <Button variant="outline" size="sm" onClick={() => setTplForm(null)} disabled={busy}>
                    {t("common.cancel")}
                  </Button>
                </div>
              </div>
            ) : null}
            {status && status.templates.length > 0 ? (
              <div className="flex flex-col gap-1.5">
                {status.templates.map((tpl) => (
                  <div key={tpl.id} className="flex flex-wrap items-center gap-2 rounded-[var(--r-sm,8px)] border border-[var(--line-strong,#d0d6e0)] px-3 py-2">
                    <label className="flex cursor-pointer items-center gap-1.5 text-[0.78rem] font-medium text-[var(--t1,#222326)]">
                      <input type="checkbox" checked={tpl.enabled} onChange={() => void toggleTemplate(tpl)} disabled={busy} />
                      {tpl.name}
                    </label>
                    <span className="font-mono text-[0.7rem] text-[var(--t3,#8a8f98)]">{[...tpl.content].length}</span>
                    <span className="ml-auto flex shrink-0 gap-1">
                      <Button variant="outline" size="sm" onClick={() => setTplForm({ id: tpl.id, name: tpl.name, content: tpl.content, enabled: tpl.enabled })} disabled={busy}>
                        {t("pages.ai.editConfig")}
                      </Button>
                      <Button variant="ghost" size="sm" className="text-[var(--st-danger,#dc2626)] hover:bg-[#FDECEC]" onClick={() => setDeletingTpl(tpl)} disabled={busy}>
                        {t("common.delete")}
                      </Button>
                    </span>
                  </div>
                ))}
              </div>
            ) : (
              <p className="py-2 text-[0.78rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.noTemplates")}</p>
            )}
          </Card>

          {/* 用量 */}
          <Card className="p-4">
            <h3 className="mb-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">{t("pages.ai.usageTitle")}</h3>
            <p className="font-mono text-[0.78rem] text-[var(--t2,#62666d)]">
              {t("pages.ai.usageText", { count: status?.usage_today.count ?? 0, date: status?.usage_today.date ?? "—" })}
            </p>
            <p className="mt-1 text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.ai.usageHint")}</p>
          </Card>

          <Card className="p-4">
            <h3 className="mb-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">{t("pages.ai.scenesTitle")}</h3>
            <ul className="list-inside list-disc text-[0.78rem] leading-relaxed text-[var(--t2,#62666d)]">
              <li>{t("pages.ai.sceneLogs")}</li>
              <li>{t("pages.ai.sceneConfig")}</li>
              <li>{t("pages.ai.sceneDraft")}</li>
            </ul>
          </Card>

          <Card className="p-4">
            <h3 className="mb-2 text-[0.875rem] font-semibold text-[var(--t1,#222326)]">{t("pages.ai.mcpTitle")}</h3>
            <p className="text-[0.75rem] leading-relaxed text-[var(--t2,#62666d)]">{t("pages.ai.mcpHint")}</p>
          </Card>

          {loadFailed ? (
            <Card className="p-4">
              <p className="text-[0.78rem] text-[var(--st-warn,#9a6700)]" role="alert">{t("pages.ai.loadFailed")}</p>
              <Button className="mt-2 gap-1" variant="soft" size="sm" onClick={() => void refresh()} disabled={busy}>
                <RefreshCw className="size-3.5" /> {t("pages.ai.retry")}
              </Button>
            </Card>
          ) : null}
        </div>
      </div>

      <ConfirmDialog
        open={deleting != null}
        title={t("pages.ai.deleteConfigTitle")}
        description={deleting ? t("pages.ai.deleteConfigDesc", { name: deleting.name }) : undefined}
        confirmText={t("common.delete")}
        destructive
        onConfirm={() => {
          if (deleting) void deleteConfig(deleting.id);
          setDeleting(null);
        }}
        onCancel={() => setDeleting(null)}
      />
      <ConfirmDialog
        open={deletingTpl != null}
        title={t("pages.ai.deleteTemplateTitle")}
        description={deletingTpl ? t("pages.ai.deleteTemplateDesc", { name: deletingTpl.name }) : undefined}
        confirmText={t("common.delete")}
        destructive
        onConfirm={() => {
          if (deletingTpl) void deleteTemplate(deletingTpl.id);
          setDeletingTpl(null);
        }}
        onCancel={() => setDeletingTpl(null)}
      />
    </div>
  );
}
