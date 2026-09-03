import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Globe,
  KeyRound,
  Loader2,
  Network,
  Play,
  RotateCw,
  ShieldCheck,
  Square,
  Plus,
  Trash2,
  FileText,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { useToast } from "@/components/ui/toast";
import { useWorkspace } from "@/providers/workspace-provider";
import { useRuntime } from "@/providers/runtime-provider";
import { useUnsavedEntry } from "@/providers/unsaved-guard";
import {
  apiGatewayApply,
  apiGatewayPreview,
  apiGatewayRestart,
  apiGatewayStart,
  apiGatewayStatus,
  apiGatewayStop,
  apiGatewayTrust,
  apiGatewayValidate,
  apiToolchainProbe,
  apiYamlGet,
} from "../ipc/api";
import { IpcFailure } from "../ipc/protocol";
import type {
  GatewayConf,
  GatewayKind,
  GatewayProbe,
  GatewayRouteSpec,
  GatewayStatusOut,
  GatewayValidateOut,
  RtState,
  ToolProbe,
  YamlView,
} from "../ipc/protocol";
import { opErrorLabel, GATEWAY_STATE_TINT, GATEWAY_STATE_DOT } from "@/lib/status";

/* ---------------- helpers ---------------- */

const KINDS: GatewayKind[] = ["nginx", "caddy", "apache"];

function emptyConf(): GatewayConf {
  return { kind: null, enabled: true, port: 8080, bin: null, tls: "off", routes: [] };
}

/** yaml 段（可能为 `{}` 未配置）→ 可编辑草稿。 */
function confFromSpec(gw: GatewayConf | null | undefined): GatewayConf {
  if (!gw) return emptyConf();
  return {
    kind: gw.kind ?? null,
    enabled: gw.enabled ?? true,
    port: gw.port ?? 8080,
    bin: gw.bin ?? null,
    tls: gw.tls ?? "off",
    routes: (gw.routes ?? []).map((r) => ({ ...r })),
  };
}

function confEquals(a: GatewayConf | null, b: GatewayConf | null): boolean {
  return JSON.stringify(a ?? emptyConf()) === JSON.stringify(b ?? emptyConf());
}

function StateChip({ state }: { state: RtState | null }) {
  const { t } = useTranslation();
  if (!state) return null;
  return (
    <span className={cn("inline-flex h-5 items-center gap-1 rounded-full px-2 font-mono text-[10px] font-semibold leading-none", GATEWAY_STATE_TINT[state])}>
      <span className={cn("size-1.5 rounded-full", GATEWAY_STATE_DOT[state])} />
      {t(`pages.gateway.state_${state}`)}
    </span>
  );
}

function ProbeRow({ name, probe }: { name: string; probe: ToolProbe | undefined }) {
  const { t } = useTranslation();
  const missing = !probe?.found;
  return (
    <div className="flex items-center gap-2 py-1 text-[0.8rem]">
      <span className={cn("size-1.5 shrink-0 rounded-full", missing ? "bg-[#8a8f98]" : "bg-[#27a644]")} />
      <span className="w-16 shrink-0 font-mono text-[var(--t1,#222326)]">{name}</span>
      {missing ? (
        <span className="min-w-0 truncate text-[var(--t3,#8a8f98)]">
          {t("pages.gateway.notFound")} · {probe?.path ?? "—"}
        </span>
      ) : (
        <>
          <span className="font-mono text-[var(--t2,#62666d)]">{probe?.version ?? "?"}</span>
          <span className="min-w-0 truncate text-[var(--t3,#8a8f98)]">{probe?.path}</span>
        </>
      )}
    </div>
  );
}

/* ---------------- page ---------------- */

export function GatewayPage() {
  const { t } = useTranslation();
  const { state: ws } = useWorkspace();
  const wsId = ws.workspaceId;
  const { toast } = useToast();
  const { state: rt } = useRuntime();

  const [status, setStatus] = useState<GatewayStatusOut | null>(null);
  const [yaml, setYaml] = useState<YamlView | null>(null);
  const [probe, setProbe] = useState<GatewayProbe | null>(null);
  const [draft, setDraft] = useState<GatewayConf | null>(null);
  const [busy, setBusy] = useState(false);
  const [previewText, setPreviewText] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);
  const [validateOut, setValidateOut] = useState<GatewayValidateOut | null>(null);
  const [trustOpen, setTrustOpen] = useState(false);
  const yamlRef = useRef<YamlView | null>(null);
  yamlRef.current = yaml;

  const refresh = useCallback(async () => {
    if (!wsId) return;
    try {
      const [st, yv] = await Promise.all([apiGatewayStatus(wsId), apiYamlGet()]);
      setStatus(st);
      setYaml(yv);
      setDraft(confFromSpec(yv.spec.gateway));
      setValidateOut(null);
    } catch (e) {
      toast(e instanceof IpcFailure ? opErrorLabel(e.code) : String(e), "err");
    }
  }, [wsId, toast]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    let alive = true;
    apiToolchainProbe()
      .then((p) => {
        if (alive) setProbe(p.gateway ?? null);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [wsId]);

  const liveState = rt.gateway?.state ?? null;
  const configured = (draft?.kind ?? null) != null;
  const dirty = useMemo(
    () => yaml != null && !confEquals(draft ?? null, confFromSpec(yaml.spec.gateway)),
    [yaml, draft],
  );
  const running = liveState === "running" || liveState === "starting" || liveState === "unhealthy";

  const services = useMemo(() => Object.entries(yaml?.spec.services ?? {}), [yaml]);

  const setDraftConf = (patch: Partial<GatewayConf>) => {
    setDraft((d) => (d ? { ...d, ...patch } : d));
  };

  const setRoute = (i: number, patch: Partial<GatewayRouteSpec>) => {
    setDraft((d) => {
      if (!d) return d;
      const routes = d.routes.map((r, idx) => (idx === i ? { ...r, ...patch } : r));
      return { ...d, routes };
    });
  };

  const errText = (e: unknown) => (e instanceof IpcFailure ? opErrorLabel(e.code) : String(e));

  const doStart = async () => {
    if (!wsId) return;
    setBusy(true);
    try {
      await apiGatewayStart(wsId);
      toast(t("pages.gateway.startSent"), "ok");
    } catch (e) {
      toast(errText(e), "err");
    } finally {
      setBusy(false);
    }
  };

  const doStop = async () => {
    if (!wsId) return;
    setBusy(true);
    try {
      await apiGatewayStop(wsId);
      toast(t("pages.gateway.stopSent"), "ok");
    } catch (e) {
      toast(errText(e), "err");
    } finally {
      setBusy(false);
    }
  };

  const doRestart = async () => {
    if (!wsId) return;
    setBusy(true);
    try {
      await apiGatewayRestart(wsId);
      toast(t("pages.gateway.restartSent"), "ok");
    } catch (e) {
      toast(errText(e), "err");
    } finally {
      setBusy(false);
    }
  };

  const doValidate = async () => {
    if (!wsId) return;
    setBusy(true);
    try {
      const out = await apiGatewayValidate(wsId, dirty ? draft : null);
      setValidateOut(out);
    } catch (e) {
      toast(errText(e), "err");
    } finally {
      setBusy(false);
    }
  };

  const openPreview = async () => {
    if (!wsId || !draft) return;
    setBusy(true);
    try {
      const out = await apiGatewayPreview(wsId, draft);
      setPreviewText(out.files.map((f) => `# ${f.name}\n${f.content}`).join("\n\n"));
    } catch (e) {
      toast(errText(e), "err");
    } finally {
      setBusy(false);
    }
  };

  const doApply = async (): Promise<boolean> => {
    if (!wsId || !draft || !yamlRef.current) return false;
    setApplying(true);
    try {
      const out = await apiGatewayApply(wsId, draft, yamlRef.current.hash);
      toast(t("pages.gateway.applied", { restarted: out.restarted }), "ok");
      setPreviewText(null);
      await refresh();
      return true;
    } catch (e) {
      toast(errText(e), "err");
      return false;
    } finally {
      setApplying(false);
    }
  };

  // 未保存守卫：网关草稿有改动即视为脏（保存 = 应用配置）
  useUnsavedEntry("gateway.conf", () => dirty, () => doApply());

  const doTrust = async () => {
    if (!wsId) return;
    setTrustOpen(false);
    try {
      await apiGatewayTrust(wsId);
      toast(t("pages.gateway.trustDone"), "ok");
    } catch (e) {
      toast(errText(e), "err");
    }
  };

  const fromServices = () => {
    if (!yaml) return;
    const routes: GatewayRouteSpec[] = services
      .filter(([, s]) => s.enabled && (s.port != null || s.ports.length > 0))
      .map(([id]) => ({ host: null, path: `/${id}`, target: id, upstream: null }));
    setDraftConf({ routes });
  };

  if (!wsId) {
    return (
      <div className="flex flex-1 items-center justify-center text-[0.875rem] text-[var(--t3,#8a8f98)]">
        {t("pages.gateway.noWorkspace")}
      </div>
    );
  }

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-4 p-4">
      <div className="flex items-center gap-2">
        <Network className="size-5 text-[var(--primary,#5E6AD2)]" />
        <h1 className="text-[1.05rem] font-semibold text-[var(--t1,#222326)]">{t("pages.gateway.title")}</h1>
        <StateChip state={liveState} />
        {dirty ? (
          <Badge variant="outline" className="border-[#f0d58a] bg-[#fdf6e3] text-[#B7791F]">
            {t("pages.gateway.unsaved")}
          </Badge>
        ) : null}
      </div>

      {/* 空态：未配置 */}
      {!configured ? (
        <Card className="flex flex-col items-center gap-3 border-dashed p-8 text-center">
          <Globe className="size-8 text-[var(--t3,#8a8f98)]" />
          <p className="max-w-md text-[0.85rem] text-[var(--t2,#62666d)]">{t("pages.gateway.emptyDesc")}</p>
          <div className="flex items-center gap-2">
            <Select
              value={draft?.kind ?? ""}
              onValueChange={(v) => setDraftConf({ kind: v as GatewayKind })}
            >
              <SelectTrigger size="sm" className="w-36">
                <SelectValue placeholder={t("pages.gateway.pickKind")} />
              </SelectTrigger>
              <SelectContent>
                {KINDS.map((k) => (
                  <SelectItem key={k} value={k}>
                    {k}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button variant="soft" size="sm" onClick={fromServices} disabled={!draft}>
              {t("pages.gateway.fromServices")}
            </Button>
          </div>
        </Card>
      ) : null}

      {/* 总览卡 */}
      {configured ? (
        <Card className="flex flex-col gap-3 p-4">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[0.95rem] font-semibold text-[var(--t1,#222326)]">{t("pages.gateway.overview")}</span>
            <div className="ml-auto flex items-center gap-2">
              {running ? (
                <>
                  <Button variant="warn" size="sm" onClick={doRestart} disabled={busy}>
                    <RotateCw className="size-3.5" />
                    {t("pages.gateway.restart")}
                  </Button>
                  <Button variant="destructive" size="sm" onClick={doStop} disabled={busy}>
                    <Square className="size-3.5" />
                    {t("pages.gateway.stop")}
                  </Button>
                </>
              ) : (
                <Button variant="default" size="sm" onClick={doStart} disabled={busy}>
                  {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Play className="size-3.5" />}
                  {t("pages.gateway.start")}
                </Button>
              )}
            </div>
          </div>
          <div className="grid grid-cols-2 gap-x-6 gap-y-1 text-[0.8rem] md:grid-cols-4">
            <label className="flex flex-col gap-1">
              <span className="text-[var(--t3,#8a8f98)]">{t("pages.gateway.kind")}</span>
              <Select value={draft?.kind ?? ""} onValueChange={(v) => setDraftConf({ kind: v as GatewayKind })}>
                <SelectTrigger size="sm" className="w-32">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {KINDS.map((k) => (
                    <SelectItem key={k} value={k}>
                      {k}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-[var(--t3,#8a8f98)]">{t("pages.gateway.listenPort")}</span>
              <Input
                className="h-7 w-24"
                type="number"
                min={1024}
                max={65535}
                value={draft?.port ?? 8080}
                onChange={(e) => setDraftConf({ port: Number(e.target.value) || 0 })}
              />
            </label>
            <div className="flex flex-col gap-1">
              <span className="text-[var(--t3,#8a8f98)]">{t("pages.gateway.runtimeState")}</span>
              <span className="font-mono text-[var(--t1,#222326)]">
                {rt.gateway?.pid ? `pid ${rt.gateway.pid}` : "—"}
              </span>
            </div>
            <div className="flex flex-col gap-1">
              <span className="text-[var(--t3,#8a8f98)]">{t("pages.gateway.conf")}</span>
              <span className="min-w-0 truncate font-mono text-[var(--t2,#62666d)]" title={status?.conf_path ?? undefined}>
                {status?.conf_path ?? "—"}
              </span>
            </div>
          </div>
          {rt.gateway?.last_error ? (
            <div className="rounded-[var(--r-sm,8px)] bg-[#fdecec] px-3 py-2 text-[0.78rem] text-[#dc2626]">
              {rt.gateway.last_error}
            </div>
          ) : null}
        </Card>
      ) : null}

      {/* 路由卡 */}
      {configured ? (
        <Card className="flex flex-col gap-3 p-4">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[0.95rem] font-semibold text-[var(--t1,#222326)]">{t("pages.gateway.routes")}</span>
            <span className="font-mono text-[11px] text-[var(--t3,#8a8f98)]">{draft?.routes.length ?? 0}</span>
            <div className="ml-auto flex items-center gap-2">
              <Button variant="soft" size="sm" onClick={fromServices}>
                {t("pages.gateway.fromServices")}
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={() => setDraftConf({ routes: [...(draft?.routes ?? []), { host: null, path: "/", target: null, upstream: null }] })}
              >
                <Plus className="size-3.5" />
                {t("pages.gateway.addRoute")}
              </Button>
            </div>
          </div>
          {(draft?.routes.length ?? 0) === 0 ? (
            <p className="text-[0.8rem] text-[var(--t3,#8a8f98)]">{t("pages.gateway.noRoutes")}</p>
          ) : (
            <div className="flex flex-col gap-2">
              {draft!.routes.map((r, i) => {
                const manual = r.upstream != null;
                const alive =
                  status?.routes.find((sr) => sr.path === r.path && (sr.host ?? null) === (r.host ?? null))
                    ?.upstream_alive ?? null;
                return (
                  <div
                    key={i}
                    className="grid grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)_auto_minmax(0,1.4fr)_auto_auto_auto] items-center gap-2 rounded-[var(--r-sm,8px)] border border-[#D0D6E0] p-2"
                  >
                    <Input
                      className="h-7 min-w-0 font-mono"
                      placeholder={t("pages.gateway.hostPlaceholder")}
                      value={r.host ?? ""}
                      onChange={(e) => setRoute(i, { host: e.target.value || null })}
                    />
                    <Input
                      className="h-7 min-w-0 font-mono"
                      placeholder="/api"
                      value={r.path}
                      onChange={(e) => setRoute(i, { path: e.target.value })}
                    />
                    <span className="text-[var(--t3,#8a8f98)]">→</span>
                    {manual ? (
                      <Input
                        className="h-7 min-w-0 font-mono"
                        value={r.upstream ?? ""}
                        placeholder="127.0.0.1:9000"
                        onChange={(e) => setRoute(i, { upstream: e.target.value })}
                      />
                    ) : (
                      <Select value={r.target ?? ""} onValueChange={(v) => setRoute(i, { target: v })}>
                        <SelectTrigger size="sm" className="h-7 w-full min-w-0">
                          <SelectValue placeholder={t("pages.gateway.pickTarget")} />
                        </SelectTrigger>
                        <SelectContent>
                          {services.map(([id, s]) => (
                            <SelectItem key={id} value={id}>
                              {id}
                              {s.port != null ? ` · ${s.port}` : ""}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    )}
                    <div className="inline-flex overflow-hidden rounded-[var(--r-sm,8px)] border border-[#D0D6E0] text-[11px]">
                      <button
                        type="button"
                        className={cn(
                          "cursor-pointer px-2 py-1 transition-colors duration-150",
                          !manual
                            ? "bg-[var(--primary,#5E6AD2)] text-white"
                            : "text-[var(--t2,#62666d)] hover:bg-[var(--surface-2,#f3f4f5)]",
                        )}
                        title={t("pages.gateway.useTarget")}
                        onClick={() => setRoute(i, { upstream: null, target: r.target ?? services[0]?.[0] ?? null })}
                      >
                        {t("pages.gateway.targetToggle")}
                      </button>
                      <button
                        type="button"
                        className={cn(
                          "cursor-pointer border-l border-[#D0D6E0] px-2 py-1 transition-colors duration-150",
                          manual
                            ? "bg-[var(--primary,#5E6AD2)] text-white"
                            : "text-[var(--t2,#62666d)] hover:bg-[var(--surface-2,#f3f4f5)]",
                        )}
                        title={t("pages.gateway.useUpstream")}
                        onClick={() => setRoute(i, { upstream: "127.0.0.1:9000", target: null })}
                      >
                        {t("pages.gateway.upstreamToggle")}
                      </button>
                    </div>
                    <span
                      className={cn(
                        "size-2 rounded-full",
                        alive == null ? "bg-transparent" : alive ? "bg-[#27a644]" : "bg-[#c3c6cc]",
                      )}
                      title={alive == null ? undefined : alive ? t("pages.gateway.upstreamAlive") : t("pages.gateway.upstreamDown")}
                    />
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      className="text-[#dc2626] hover:bg-[#FDECEC]"
                      aria-label={t("pages.gateway.removeRoute")}
                      onClick={() => setDraftConf({ routes: draft!.routes.filter((_, idx) => idx !== i) })}
                    >
                      <Trash2 className="size-3.5" />
                    </Button>
                  </div>
                );
              })}
            </div>
          )}
        </Card>
      ) : null}

      {/* 配置预览 + 校验 */}
      {configured ? (
        <Card className="flex flex-col gap-3 p-4">
          <div className="flex flex-wrap items-center gap-2">
            <span className="flex items-center gap-1.5 text-[0.95rem] font-semibold text-[var(--t1,#222326)]">
              <FileText className="size-4" />
              {t("pages.gateway.preview")}
            </span>
            <div className="ml-auto flex items-center gap-2">
              <Button variant="soft" size="sm" onClick={doValidate} disabled={busy}>
                {busy ? <Loader2 className="size-3.5 animate-spin" /> : <ShieldCheck className="size-3.5" />}
                {t("pages.gateway.validate")}
              </Button>
              <Button variant="success" size="sm" onClick={openPreview} disabled={busy || !dirty}>
                {t("pages.gateway.apply")}
              </Button>
            </div>
          </div>
          {validateOut ? (
            validateOut.ok ? (
              <div className="rounded-[var(--r-sm,8px)] bg-[#e9f7ed] px-3 py-2 text-[0.78rem] text-[#1e7e35]">
                {t("pages.gateway.validateOk")}
              </div>
            ) : (
              <div className="rounded-[var(--r-sm,8px)] bg-[#fdecec] px-3 py-2 text-[0.78rem] text-[#dc2626]">
                <p>{validateOut.message}</p>
                {validateOut.stderr ? <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap font-mono">{validateOut.stderr}</pre> : null}
              </div>
            )
          ) : null}
          {previewText ? (
            <pre className="max-h-72 overflow-auto rounded-[var(--r-sm,8px)] bg-[#1b1c1f] p-3 font-mono text-[0.72rem] leading-5 text-[#d6dadd]">
              {previewText}
            </pre>
          ) : null}
        </Card>
      ) : null}

      {/* HTTPS 卡（仅 caddy） */}
      {draft?.kind === "caddy" ? (
        <Card className="flex flex-col gap-3 p-4">
          <span className="text-[0.95rem] font-semibold text-[var(--t1,#222326)]">{t("pages.gateway.https")}</span>
          <div className="flex flex-wrap items-center gap-2 text-[0.8rem]">
            <Select value={draft.tls ?? "off"} onValueChange={(v) => setDraftConf({ tls: v as GatewayConf["tls"] })}>
              <SelectTrigger size="sm" className="w-44">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="off">{t("pages.gateway.tlsOff")}</SelectItem>
                <SelectItem value="internal">{t("pages.gateway.tlsInternal")}</SelectItem>
              </SelectContent>
            </Select>
            {draft.tls === "internal" ? (
              <Button variant="soft" size="sm" onClick={() => setTrustOpen(true)}>
                <KeyRound className="size-3.5" />
                {t("pages.gateway.trust")}
              </Button>
            ) : null}
          </div>
          <p className="text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.gateway.tlsHint")}</p>
        </Card>
      ) : null}

      {/* 工具链卡 */}
      <Card className="flex flex-col gap-1 p-4">
        <span className="mb-1 text-[0.95rem] font-semibold text-[var(--t1,#222326)]">{t("pages.gateway.toolchain")}</span>
        <ProbeRow name="nginx" probe={probe?.nginx} />
        <ProbeRow name="caddy" probe={probe?.caddy} />
        <ProbeRow name="apache" probe={probe?.apache} />
      </Card>

      {/* 应用确认（diff 弹窗） */}
      <Dialog open={previewText != null} onOpenChange={(o) => !o && setPreviewText(null)}>
        <DialogContent className="max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t("pages.gateway.applyTitle")}</DialogTitle>
          </DialogHeader>
          <p className="text-[0.8rem] text-[var(--t2,#62666d)]">{t("pages.gateway.applyDesc")}</p>
          <pre className="max-h-80 overflow-auto rounded-[var(--r-sm,8px)] bg-[#1b1c1f] p-3 font-mono text-[0.72rem] leading-5 text-[#d6dadd]">
            {previewText}
          </pre>
          <DialogFooter>
            <Button variant="outline" size="sm" onClick={() => setPreviewText(null)}>
              {t("common.cancel")}
            </Button>
            <Button variant="success" size="sm" onClick={doApply} disabled={applying}>
              {applying ? <Loader2 className="size-3.5 animate-spin" /> : null}
              {t("pages.gateway.applyConfirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* trust 风险确认 */}
      <ConfirmDialog
        open={trustOpen}
        title={t("pages.gateway.trustTitle")}
        description={t("pages.gateway.trustDesc")}
        confirmText={t("pages.gateway.trustConfirm")}
        cancelText={t("common.cancel")}
        onConfirm={doTrust}
        onCancel={() => setTrustOpen(false)}
      />
    </div>
  );
}
