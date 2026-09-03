import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * 分组标题统一收敛（22 处 `uppercase tracking-wider` 的错位根因修复）。
 *
 * 根因：
 * 1. `font-mono`（ui-monospace/Cascadia/Consolas）无 CJK 字形，“采集于/当前/启用：/条”
 *    回退到 PingFang/Microsoft YaHei，与 mono 数字基线/字高不一致 → 看起来上下错位；
 * 2. 父级 `tracking-wider` 被 `normal-case` 的子 span 继承（normal-case 只取消大小写，
 *    不取消 letter-spacing），mono 数字被额外拉宽；
 * 3. `flex items-center` 按盒居中而非基线对齐，三种字号（标题 11.5px / meta 10px / 图标 14px）
 *    光学中心不同。
 *
 * 约定：
 * - 标题（中/英文）沿用 `uppercase tracking-wider`，仅加 `leading-none`；
 * - 含中文的 meta 一律 `font-sans + tabular-nums + tracking-normal + leading-none`
 *  （body 已有 tabular-nums，数字依然对齐，无需 mono，避免回退字体错位）；
 * - 纯数字/纯拉丁才用 `mono`（`SectionCount` / `SectionMeta mono`），同样强制 tracking-normal。
 */
export function SectionTitle({
  icon,
  title,
  meta,
  actions,
  className,
  htmlTitle,
}: {
  icon?: ReactNode;
  title: ReactNode;
  meta?: ReactNode;
  actions?: ReactNode;
  className?: string;
  htmlTitle?: string;
}) {
  return (
    <div
      title={htmlTitle}
      className={cn(
        "flex items-center gap-2 text-[0.72rem] font-semibold uppercase tracking-wider text-[var(--t3,#8a8f98)]",
        className,
      )}
    >
      {icon ? <span className="inline-flex shrink-0 items-center [&_svg]:size-3.5">{icon}</span> : null}
      <span className="leading-none">{title}</span>
      {meta}
      {actions ? <span className="ml-auto inline-flex shrink-0 items-center gap-1">{actions}</span> : null}
    </div>
  );
}

/** 含中文的标题后缀：强制 sans + tracking-normal，避免 mono 回退错位。 */
export function SectionMeta({
  children,
  mono = false,
  className,
}: {
  children: ReactNode;
  /** 仅纯数字/纯拉丁时置 true；默认 sans（中西混排安全）。 */
  mono?: boolean;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "text-[10px] font-normal normal-case leading-none tabular-nums tracking-normal text-[var(--t2,#62666d)]",
        mono ? "font-mono" : "font-sans",
        className,
      )}
    >
      {children}
    </span>
  );
}

/** 纯数字计数（如 N 条/当前端口里的数字位）：mono 安全，强制 tracking-normal。 */
export function SectionCount({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <span className={cn("font-mono text-[0.7rem] leading-none tracking-normal text-[var(--t3,#8a8f98)]", className)}>
      {children}
    </span>
  );
}
