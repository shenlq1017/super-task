/**
 * 声明式需求 needs 面板（ipc.md §10.17）。
 * resolve-only dry-run：把 supertask.yaml 顶层 needs 声明解析为四态
 * （已存在 / 可安装 / 可从归档供给 / 不可满足），解析本身零副作用；
 * installable 行的「安装」复用 env 页既有 toolchain.install 长操作链路
 * （发起与终态轮询由调用方负责，本组件只回调）。
 */
import { useTranslation } from "react-i18next";
import { Download, Loader2, Search } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { cn } from "@/lib/utils";
import type { NeedItem, NeedStatus, NeedsResolveOut } from "@/ipc/protocol";

/** 四态徽标（配色沿用 adopt-panel / env 页既有色调：绿=满足、蓝=可安装、紫=归档、红=不可满足）。 */
const STATUS_BADGE: Record<NeedStatus, string> = {
  satisfied: "border-[rgb(39_166_68_/_0.35)] bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)]",
  installable: "border-[rgb(94_106_210_/_0.35)] bg-[rgb(94_106_210_/_0.08)] text-[var(--st-accent,#5e6ad2)]",
  archive: "border-[rgb(136_84_208_/_0.35)] bg-[rgb(136_84_208_/_0.08)] text-[#8854d0]",
  unsatisfiable: "border-red-200 bg-[var(--st-danger-tint,#fdecec)] text-[#DC2626]",
};

function NeedRow({
  item,
  installing,
  onInstall,
}: {
  item: NeedItem;
  installing: boolean;
  onInstall: (item: NeedItem) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] p-2.5">
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant="outline" className={cn("shrink-0", STATUS_BADGE[item.status])}>
          {t(`pages.env.needs.status.${item.status}`)}
        </Badge>
        <span className="font-mono text-[0.78rem] font-semibold text-[var(--t1,#222326)]">{item.need}</span>
        {item.status === "satisfied" && item.found_version ? (
          <Badge variant="outline" className="shrink-0 font-mono text-[10px]">
            {item.found_version}
          </Badge>
        ) : null}
        {item.status === "installable" ? (
          <Button
            variant="default"
            size="sm"
            className="ml-auto h-7 shrink-0 gap-1 px-2 text-[0.72rem]"
            disabled={installing}
            onClick={() => onInstall(item)}
          >
            {installing ? <Loader2 className="size-3.5 animate-spin" /> : <Download className="size-3.5" />}
            {installing ? t("pages.env.needs.installing") : t("pages.env.needs.install")}
          </Button>
        ) : null}
      </div>
      {item.status === "satisfied" && item.found_path ? (
        <span
          className="mt-1 block truncate font-mono text-[0.72rem] text-[var(--t3,#8a8f98)]"
          title={item.found_path}
        >
          {item.found_path}
        </span>
      ) : null}
      <span className="mt-1 block break-words text-[0.72rem] leading-relaxed text-[var(--t2,#62666d)]">
        {item.reason}
      </span>
    </div>
  );
}

export function NeedsPanel({
  needs,
  output,
  loading,
  error,
  installingIds,
  onResolve,
  onInstall,
}: {
  /** 当前工作区 spec 顶层 needs 声明（未声明为 undefined / 空数组）。 */
  needs: string[] | undefined;
  output: NeedsResolveOut | null;
  loading: boolean;
  error: string | null;
  /** 正在安装的工具 id（该行按钮禁用）。 */
  installingIds: string[];
  onResolve: () => void;
  onInstall: (item: NeedItem) => void;
}) {
  const { t } = useTranslation();
  const declared = needs ?? [];
  return (
    <Card className="mt-3 p-3 sm:p-4" data-env-needs="1">
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="text-[0.875rem] font-semibold text-[var(--t1,#222326)]">{t("pages.env.needs.title")}</h3>
        {declared.length > 0 ? (
          <span className="flex min-w-0 flex-wrap items-center gap-1">
            {declared.map((n) => (
              <Badge key={n} variant="secondary" className="font-mono text-[10px]">
                {n}
              </Badge>
            ))}
          </span>
        ) : (
          <span className="min-w-0 flex-1 text-[0.72rem] text-[var(--t3,#8a8f98)]">{t("pages.env.needs.noNeeds")}</span>
        )}
        <Button variant="soft" size="sm" className="ml-auto shrink-0 gap-1" onClick={onResolve} disabled={loading}>
          {loading ? <Loader2 className="size-3.5 animate-spin" /> : <Search className="size-3.5" />}
          {loading ? t("pages.env.needs.checking") : t("pages.env.needs.check")}
        </Button>
      </div>
      <p className="mt-1.5 text-[0.72rem] leading-relaxed text-[var(--t3,#8a8f98)]">{t("pages.env.needs.hint")}</p>

      {error ? (
        <div
          className="mt-2 rounded-[var(--r-sm,8px)] border border-[rgb(192_53_53_/_0.3)] bg-[var(--st-danger-tint,#fdeeee)] px-2.5 py-1.5 text-[0.74rem] leading-relaxed text-[var(--st-danger,#c03535)]"
          role="alert"
        >
          {error}
        </div>
      ) : null}

      {output && output.warnings.length > 0 ? (
        <div
          className="mt-2 rounded-[var(--r-sm,8px)] border border-[#f0d58a] bg-[#fdf6e3] px-2.5 py-1.5 text-[0.74rem] leading-relaxed text-[#B7791F]"
          role="alert"
          aria-label={t("pages.env.needs.warnings")}
        >
          {output.warnings.map((w, i) => (
            <div key={i}>{w}</div>
          ))}
        </div>
      ) : null}

      {output ? (
        output.items.length === 0 ? (
          <p className="mt-2 text-[0.75rem] text-[var(--t3,#8a8f98)]">{t("pages.env.needs.noNeeds")}</p>
        ) : (
          <div className="mt-2 flex flex-col gap-1.5">
            {output.items.map((it) => (
              <NeedRow
                key={it.need}
                item={it}
                installing={installingIds.includes(it.id)}
                onInstall={onInstall}
              />
            ))}
          </div>
        )
      ) : null}
    </Card>
  );
}
