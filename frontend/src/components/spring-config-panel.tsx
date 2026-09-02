import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { copyText } from "@/lib/copy-text";
import { useToast } from "@/components/ui/toast";
import { apiSpringInspect } from "../ipc/api";
import type { EnvEffectiveEntry, SpringConfigEntry, SpringConfigOut } from "@/ipc/protocol";
import {
  FileText,
  RefreshCw,
  ChevronDown,
  ChevronRight,
  Copy,
  Check,
  Loader2,
  Replace,
} from "lucide-react";

/* ---------- 纯函数 ---------- */

/** basename application-<p>.* → p；否则 null（基础文件）。 */
function profileOfFile(file: string): string | null {
  const base = file.split("/").pop() ?? file;
  if (!base.startsWith("application-")) return null;
  const m = base.slice("application-".length).match(/^([^.]+)/);
  return m?.[1] ?? null;
}

type FileGroup = { file: string; profile: string | null; entries: SpringConfigEntry[] };

/** 保后端序分组：基础组（所有基础文件）在前，各 profile 组在后。 */
function groupByFile(entries: SpringConfigEntry[]): FileGroup[] {
  const map = new Map<string, FileGroup>();
  for (const e of entries) {
    const profile = profileOfFile(e.file);
    const key = profile ?? "__base__";
    const existing = map.get(key);
    if (existing) {
      if (!existing.entries.some((x) => x.file === e.file && x.key === e.key)) {
        existing.entries.push(e);
      }
    } else {
      map.set(key, { file: e.file, profile, entries: [e] });
    }
  }
  return [...map.values()];
}

/** 依次查 svcEnv、effEnvEntries、base entries 的 SPRING_PROFILES_ACTIVE。 */
function detectActiveProfiles(
  svcEnv: Record<string, string> | undefined,
  effEnvEntries: EnvEffectiveEntry[] | undefined,
  baseEntries: SpringConfigEntry[],
): string[] {
  const raw =
    svcEnv?.SPRING_PROFILES_ACTIVE ??
    effEnvEntries?.find((e) => e.key === "SPRING_PROFILES_ACTIVE")?.value ??
    baseEntries.find((e) => e.key === "spring.profiles.active")?.value;
  if (!raw) return [];
  return raw
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}

type MergedRow = {
  key: string;
  value: string;
  masked: boolean;
  file: string;
  status: "override" | "new" | "same";
  baseValue?: string;
};

/** 合并视图：base ∪ profile；同键值不同 → override + baseValue；仅 profile 有 → new。 */
function buildMerged(baseEntries: SpringConfigEntry[], profileEntries: SpringConfigEntry[]): MergedRow[] {
  const baseMap = new Map(baseEntries.map((e) => [e.key, e]));
  const rows: MergedRow[] = [];
  const seen = new Set<string>();
  for (const e of profileEntries) {
    if (seen.has(e.key)) continue;
    seen.add(e.key);
    const b = baseMap.get(e.key);
    if (!b) {
      rows.push({ key: e.key, value: e.value, masked: e.masked, file: e.file, status: "new" });
    } else if (b.value !== e.value) {
      rows.push({ key: e.key, value: e.value, masked: e.masked, file: e.file, status: "override", baseValue: b.value });
    } else {
      rows.push({ key: e.key, value: e.value, masked: e.masked, file: e.file, status: "same" });
    }
  }
  return rows;
}

/** spring 点分键 → 可能的注入 env 键候选（`.`→`_` 大写；以及原样大写）。 */
function envCandidates(key: string): string[] {
  return [key.replace(/\./g, "_").toUpperCase(), key.toUpperCase()];
}

/**
 * 标记 supertask 注入链可复写的键：
 * - 静态规则：spring-boot 的 server.port → SERVER_PORT（端口注入层）
 * - 动态规则：点分键转 env 候选，若 svcEnv 或 effEnvEntries 命中则标记
 */
function injectableEnv(
  key: string,
  svcEnv: Record<string, string> | undefined,
  effEnvEntries: EnvEffectiveEntry[] | undefined,
): string | null {
  // 静态规则：server.port
  if (key === "server.port") return "SERVER_PORT";
  // 动态规则：点分键转 env 候选
  const candidates = envCandidates(key);
  for (const c of candidates) {
    if (svcEnv?.[c]) return c;
    if (effEnvEntries?.some((e) => e.key === c)) return c;
  }
  return null;
}

/* ---------- 组件 ---------- */

type Props = {
  workspaceId: string | null;
  serviceId: string;
  specPort: number | null;
  svcEnv: Record<string, string> | undefined;
  effEnvEntries: EnvEffectiveEntry[] | undefined;
  compact: boolean;
};

const LENS_ALL = "__all__";
const LENS_BASE = "__base__";

export function SpringConfigPanel({ workspaceId, serviceId, specPort, svcEnv, effEnvEntries, compact }: Props) {
  const { t } = useTranslation();
  const { toast } = useToast();
  const [cfg, setCfg] = useState<SpringConfigOut | null>(null);
  const [loading, setLoading] = useState(false);
  const [nonce, setNonce] = useState(0);
  const [lens, setLens] = useState(LENS_ALL);
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const lensTouched = useRef(false);
  const [copiedRow, setCopiedRow] = useState<string | null>(null);

  // fetch
  useEffect(() => {
    if (!workspaceId || !serviceId) {
      setCfg(null);
      return;
    }
    let alive = true;
    setLoading(true);
    apiSpringInspect(workspaceId, serviceId)
      .then((out) => {
        if (alive) setCfg(out);
      })
      .catch(() => {
        /* 只读探测：失败静默，下次进页/手动重解析会再取 */
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [workspaceId, serviceId, nonce]);

  // 派生
  const groups = useMemo(() => (cfg ? groupByFile(cfg.entries) : []), [cfg]);
  const baseEntries = useMemo(() => groups.find((g) => g.profile === null)?.entries ?? [], [groups]);
  const profiles = useMemo(() => groups.filter((g) => g.profile !== null), [groups]);
  const activeProfiles = useMemo(
    () => detectActiveProfiles(svcEnv, effEnvEntries, baseEntries),
    [svcEnv, effEnvEntries, baseEntries],
  );

  // 首次到达且用户未手动切换过 → 自动预选第一个激活 profile
  useEffect(() => {
    if (!cfg || lensTouched.current || lens !== LENS_ALL) return;
    const hit = activeProfiles.find((p) => profiles.some((g) => g.profile === p));
    if (hit) setLens(hit);
  }, [cfg, activeProfiles, profiles, lens]);

  const filteredEntries = useMemo(() => {
    if (!cfg) return [];
    const q = query.trim().toLowerCase();
    if (!q) return cfg.entries;
    return cfg.entries.filter((e) => e.key.toLowerCase().includes(q));
  }, [cfg, query]);

  const filteredGroups = useMemo(
    () => (lens === LENS_ALL ? groupByFile(filteredEntries) : []),
    [lens, filteredEntries],
  );

  const mergedRows = useMemo<MergedRow[]>(() => {
    if (lens === LENS_ALL || !cfg) return [];
    const profileEntries =
      lens === LENS_BASE ? baseEntries : (groups.find((g) => g.profile === lens)?.entries ?? []);
    return buildMerged(baseEntries, profileEntries);
  }, [lens, cfg, baseEntries, groups]);

  const visibleRows = useMemo(() => {
    if (lens === LENS_ALL) return [];
    const q = query.trim().toLowerCase();
    if (!q) return mergedRows;
    return mergedRows.filter((r) => r.key.toLowerCase().includes(q));
  }, [lens, mergedRows, query]);

  const copyAll = () => {
    const lines: string[] = [];
    if (lens === LENS_ALL) {
      for (const g of filteredGroups) {
        if (g.profile !== null && collapsed[g.profile]) continue;
        lines.push(`# ${g.file}`);
        for (const e of g.entries) {
          if (e.masked) continue;
          lines.push(`${e.key}=${e.value}`);
        }
      }
    } else {
      for (const r of visibleRows) {
        if (r.masked) continue;
        lines.push(`${r.key}=${r.value}`);
      }
    }
    if (!lines.length) return;
    void copyText(lines.join("\n")).then((ok) => {
      if (!ok) {
        toast(t("pages.run.copyCmdFailed"), "err");
        return;
      }
      toast(t("pages.run.springCopyAllOk"), "ok");
    });
  };

  const copyRow = (row: SpringConfigEntry | MergedRow) => {
    if (row.masked) return;
    const text = `${row.key}=${row.value}`;
    const id = `${row.file}:${row.key}`;
    void copyText(text).then((ok) => {
      if (!ok) return;
      setCopiedRow(id);
      window.setTimeout(() => setCopiedRow(null), 2000);
    });
  };

  if (!workspaceId) return null;

  const total = cfg?.entries.length ?? 0;

  return (
    <section className="flex flex-col gap-2">
      {/* 标题行（解析口径收进悬停提示，不再平铺说明文字） */}
      <div
        className="flex items-center gap-2 text-[0.72rem] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]"
        title={t("pages.run.springHint")}
      >
        <FileText className="size-3.5" /> {t("pages.run.springSection")}
        {activeProfiles.length > 0 ? (
          <span className="inline-flex h-5 items-center rounded-full bg-[var(--st-ok-tint,#e9f7ed)] px-1.5 font-mono text-[10px] font-semibold normal-case leading-none text-[var(--st-ok-deep,#1e7e35)]">
            {t("pages.run.springActiveChip", { profiles: activeProfiles.join(", ") })}
          </span>
        ) : null}
        <button
          type="button"
          onClick={() => setNonce((n) => n + 1)}
          title={t("pages.run.springRefresh")}
          aria-label={t("pages.run.springRefresh")}
          className="ml-auto inline-flex size-5 cursor-pointer items-center justify-center rounded text-[var(--t3,#8a8f98)] transition-colors duration-150 hover:bg-[var(--surface-2,#f3f4f5)] hover:text-[var(--t1,#222326)]"
        >
          {loading ? <Loader2 className="size-3 animate-spin" /> : <RefreshCw className="size-3" />}
        </button>
      </div>

      {/* hint 已收进标题行 tooltip */}
      {cfg && cfg.warnings.length > 0 ? (
        <div className="flex flex-col gap-0.5">
          {cfg.warnings.map((w) => (
            <p key={w} className="text-[0.7rem] text-[var(--st-warn,#9a6700)]">
              {w}
            </p>
          ))}
        </div>
      ) : null}

      {/* 工具栏 */}
      <div className="flex flex-wrap items-center gap-2">
        <Select
          value={lens}
          onValueChange={(v) => {
            setLens(v);
            lensTouched.current = true;
          }}
        >
          <SelectTrigger
            size="sm"
            className="h-7 data-[size=sm]:h-7 min-w-[9rem] cursor-pointer border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] text-xs"
            aria-label={t("pages.run.springProfileSelectAria")}
          >
            <SelectValue placeholder={t("pages.run.springProfileAll")} />
          </SelectTrigger>
          <SelectContent position="popper">
            <SelectItem value={LENS_ALL} className="cursor-pointer text-xs">
              <span className="font-medium">{t("pages.run.springProfileAll")}</span>
              <span className="ml-auto pl-2 font-mono text-[10px] text-[var(--t3,#8a8f98)]">{total}</span>
            </SelectItem>
            <SelectItem value={LENS_BASE} className="cursor-pointer text-xs">
              <span className="font-medium">{t("pages.run.springProfileBase")}</span>
              <span className="ml-auto pl-2 font-mono text-[10px] text-[var(--t3,#8a8f98)]">{baseEntries.length}</span>
            </SelectItem>
            {profiles.map((g) => (
              <SelectItem key={g.profile} value={g.profile!} className="cursor-pointer text-xs">
                <span className="font-medium">{g.profile}</span>
                {activeProfiles.includes(g.profile!) ? (
                  <span className="ml-1 text-[10px] text-[var(--st-ok,#27a644)]" aria-hidden>●</span>
                ) : null}
                <span className="ml-auto pl-2 font-mono text-[10px] text-[var(--t3,#8a8f98)]">{g.entries.length}</span>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("pages.run.springSearchPlaceholder")}
          aria-label={t("pages.run.springSearchAria")}
          className="h-7 flex-1 border-[var(--line-strong,#d0d6e0)] text-xs"
        />
        <span className="font-mono text-[10px] text-[var(--t3,#8a8f98)]">
          {t("pages.run.springEntriesCount", { count: lens === LENS_ALL ? filteredEntries.length : visibleRows.length })}
        </span>
        <Button size="sm" variant="outline" className="h-7 gap-1 text-xs" onClick={copyAll}>
          <Copy className="size-3" /> {t("pages.run.springCopyAll")}
        </Button>
      </div>

      {/* 列表卡片 */}
      <div className={cn("overflow-auto rounded-[var(--r-md,12px)] border border-[var(--line,#e6e6e6)]", compact ? "max-h-56" : "max-h-72")}>
        {loading && !cfg ? (
          <div className="flex items-center gap-2 p-3 text-[0.72rem] text-[var(--t2,#62666d)]">
            <Loader2 className="size-3 animate-spin" /> {t("pages.run.springLoading")}
          </div>
        ) : !cfg || cfg.entries.length === 0 ? (
          <p className="p-3 text-[0.72rem] text-[var(--t2,#62666d)]">{t("pages.run.springEmpty")}</p>
        ) : lens === LENS_ALL ? (
          filteredGroups.map((g) => (
            <div key={g.profile ?? LENS_BASE}>
              <div
                className="sticky top-0 z-10 flex items-center gap-1.5 bg-[var(--surface,#fff)] border-b border-[var(--line,#e6e6e6)] px-2 py-1.5"
              >
                <button
                  type="button"
                  onClick={() =>
                    setCollapsed((prev) => ({ ...prev, [g.profile ?? LENS_BASE]: !prev[g.profile ?? LENS_BASE] }))
                  }
                  className="inline-flex cursor-pointer items-center gap-1 text-[0.72rem] font-semibold text-[var(--t1,#222326)] transition-colors hover:text-[var(--st-accent,#5e6ad2)]"
                >
                  {g.profile === null || !collapsed[g.profile] ? (
                    <ChevronDown className="size-3" />
                  ) : (
                    <ChevronRight className="size-3" />
                  )}
                  {g.file.split("/").pop()}
                  {g.profile && activeProfiles.includes(g.profile) ? (
                    <span className="ml-1 inline-flex h-4 items-center rounded-full bg-[var(--st-ok-tint,#e9f7ed)] px-1 text-[9px] font-semibold leading-none text-[var(--st-ok-deep,#1e7e35)]">
                      {t("pages.run.springActiveChipShort")}
                    </span>
                  ) : null}
                </button>
                <span className="ml-auto font-mono text-[10px] text-[var(--t3,#8a8f98)]">
                  {t("pages.run.springEntriesCount", { count: g.entries.length })}
                </span>
              </div>
              {g.profile === null || !collapsed[g.profile] ? (
                <div className="flex flex-col gap-1 p-2">
                  {g.entries.map((e) => {
                    const injectEnv = injectableEnv(e.key, svcEnv, effEnvEntries);
                    const rowId = `${e.file}:${e.key}`;
                    return (
                      <div
                        key={rowId}
                        className="group/row flex items-center gap-2 rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f7f8fa)] px-2 py-1"
                      >
                        <span className="w-[38%] shrink-0 truncate font-mono text-[0.78rem] font-medium text-[var(--t1,#222326)]" title={e.key}>{e.key}</span>
                        <span
                          className={cn(
                            "min-w-0 flex-1 truncate font-mono text-[0.78rem] text-[var(--t2,#62666d)]",
                            e.masked && "text-[var(--st-warn,#9a6700)]",
                          )}
                          title={e.value}
                        >
                          {e.value}
                        </span>
                        {injectEnv ? (
                          <span
                            className="shrink-0 text-[var(--st-accent,#5e6ad2)]"
                            title={t("pages.run.springInjectTitle", { env: injectEnv })}
                          >
                            <Replace className="size-3" aria-hidden />
                          </span>
                        ) : null}
                        <button
                          type="button"
                          disabled={e.masked}
                          title={e.masked ? t("pages.run.springMaskedNoCopy") : t("pages.run.springCopyRow")}
                          aria-label={t("pages.run.springCopyRow")}
                          className="shrink-0 cursor-pointer opacity-0 transition-opacity duration-150 group-hover/row:opacity-100 disabled:cursor-not-allowed disabled:opacity-30"
                          onClick={() => copyRow(e)}
                        >
                          {copiedRow === rowId ? (
                            <Check className="size-3 text-[var(--st-ok,#27a644)]" />
                          ) : (
                            <Copy className="size-3 text-[var(--t3,#8a8f98)]" />
                          )}
                        </button>
                      </div>
                    );
                  })}
                </div>
              ) : null}
            </div>
          ))
        ) : (
          <div className="flex flex-col gap-1 p-2">
            {visibleRows.length === 0 ? (
              <p className="p-2 text-[0.72rem] text-[var(--t2,#62666d)]">{t("pages.run.springGroupEmpty")}</p>
            ) : (
              visibleRows.map((r) => {
                const injectEnv = injectableEnv(r.key, svcEnv, effEnvEntries);
                const rowId = `${r.file}:${r.key}`;
                return (
                  <div
                    key={rowId}
                    title={r.file}
                    className="group/row flex flex-wrap items-center gap-x-2 gap-y-0.5 rounded-[var(--r-sm,8px)] bg-[var(--surface-2,#f7f8fa)] px-2 py-1"
                  >
                    <span className="w-[38%] shrink-0 truncate font-mono text-[0.78rem] font-medium text-[var(--t1,#222326)]" title={r.key}>{r.key}</span>
                    <span
                      className={cn(
                        "min-w-0 flex-1 truncate font-mono text-[0.78rem] text-[var(--t2,#62666d)]",
                        r.masked && "text-[var(--st-warn,#9a6700)]",
                      )}
                      title={r.value}
                    >
                      {r.value}
                    </span>
                    {r.status === "override" ? (
                      <span
                        className="shrink-0 rounded-full bg-[var(--st-warn-tint,#fff8e1)] px-1 font-mono text-[9px] font-semibold leading-4 text-[var(--st-warn,#9a6700)]"
                        title={t("pages.run.springOverrideTitle", { profile: lens })}
                      >
                        {t("pages.run.springOverrideBadge")}
                      </span>
                    ) : r.status === "new" ? (
                      <span
                        className="shrink-0 rounded-full bg-[var(--st-accent-tint,#eef0fb)] px-1 font-mono text-[9px] font-semibold leading-4 text-[var(--st-accent,#5e6ad2)]"
                        title={t("pages.run.springNewTitle", { profile: lens })}
                      >
                        {t("pages.run.springNewBadge")}
                      </span>
                    ) : null}
                    {injectEnv ? (
                      <span
                        className="shrink-0 text-[var(--st-accent,#5e6ad2)]"
                        title={t("pages.run.springInjectTitle", { env: injectEnv })}
                      >
                        <Replace className="size-3" aria-hidden />
                      </span>
                    ) : null}
                    <button
                      type="button"
                      disabled={r.masked}
                      title={r.masked ? t("pages.run.springMaskedNoCopy") : t("pages.run.springCopyRow")}
                      aria-label={t("pages.run.springCopyRow")}
                      className="shrink-0 cursor-pointer opacity-0 transition-opacity duration-150 group-hover/row:opacity-100 disabled:cursor-not-allowed disabled:opacity-30"
                      onClick={() => copyRow(r)}
                    >
                      {copiedRow === rowId ? (
                        <Check className="size-3 text-[var(--st-ok,#27a644)]" />
                      ) : (
                        <Copy className="size-3 text-[var(--t3,#8a8f98)]" />
                      )}
                    </button>
                    {r.status === "override" && r.baseValue ? (
                      <span className="w-full truncate font-mono text-[0.7rem] text-[var(--t3,#8a8f98)] line-through" title={r.baseValue}>
                        {r.baseValue}
                      </span>
                    ) : null}
                  </div>
                );
              })
            )}
          </div>
        )}
      </div>

      {/* 端口不一致警告 */}
      {cfg?.server_port != null && specPort != null && cfg.server_port !== specPort ? (
        <p className="text-[0.72rem] text-[var(--st-warn,#9a6700)]">
          {t("pages.run.springPortMismatch", { serverPort: cfg.server_port, specPort })}
        </p>
      ) : null}
    </section>
  );
}
