import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { Slot } from "radix-ui"

import { cn } from "@/lib/utils"

const buttonVariants = cva(
  // 圆角统一 --r-sm(8px)；禁止在带文字的按钮上用 rounded-full（会裁切字/图标）
  // leading-none 不能放这里：tailwind-merge 会认为后面的 text-* 尺寸类与 leading 冲突并把它删掉，须写在各 size 的 text-* 之后
  "group/button inline-flex cursor-pointer shrink-0 items-center justify-center gap-1.5 overflow-visible rounded-[var(--r-sm)] border bg-clip-padding font-semibold whitespace-nowrap transition-[color,background-color,border-color,box-shadow,transform,opacity] duration-150 ease-[var(--st-ease)] outline-none select-none focus-visible:outline-2 focus-visible:outline-[var(--st-accent)] focus-visible:outline-offset-2 focus-visible:ring-0 active:scale-[.97] disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-destructive [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-3.5",
  {
    variants: {
      variant: {
        // 主操作：启动 / 安装 / 主 CTA
        default:
          "border-[var(--st-accent)] bg-[var(--st-accent)] text-[var(--primary-foreground)] shadow-[0_1px_2px_color-mix(in_srgb,var(--st-accent)_25%,transparent)] hover:border-[var(--st-accent-hover)] hover:bg-[var(--st-accent-hover)] hover:shadow-[var(--st-glow)]",
        // 中性次操作：打开 / 收起 / 刷新 / 建议 — 白底 + 强描边
        outline:
          "border-[var(--line-strong)] bg-[var(--surface)] text-[var(--t1)] shadow-[var(--shadow-1)] hover:border-[var(--t3)] hover:bg-[var(--surface-2)] aria-expanded:bg-[var(--surface-2)] aria-expanded:text-[var(--t1)]",
        // 辅助灰底：构建 jar 等
        secondary:
          "border-[var(--line-strong)] bg-[var(--surface-2)] text-[var(--t1)] hover:border-[var(--t3)] hover:bg-[var(--surface)]",
        // 低强调：行内次要
        ghost:
          "border-transparent bg-transparent text-[var(--t2)] hover:border-transparent hover:bg-[var(--surface-2)] hover:text-[var(--t1)] aria-expanded:bg-[var(--surface-2)] aria-expanded:text-[var(--t1)]",
        // 品牌软色：检查 / 探测 / 校验
        soft:
          "border-[var(--st-accent-tint-line)] bg-[var(--st-accent-tint)] text-[var(--st-accent)] hover:border-[var(--st-accent)] hover:bg-[color-mix(in_srgb,var(--st-accent)_16%,transparent)]",
        // 成功软色：保存 / 确认写入
        success:
          "border-[color-mix(in_srgb,var(--st-ok)_45%,transparent)] bg-[var(--st-ok-tint)] text-[var(--st-ok-deep)] hover:border-[var(--st-ok)] hover:bg-[color-mix(in_srgb,var(--st-ok)_18%,transparent)]",
        // 警示软色：重启 / 升级
        warn:
          "border-[var(--st-warn-line)] bg-[var(--st-warn-tint)] text-[var(--st-warn)] hover:border-[var(--st-warn)]/50 hover:bg-[color-mix(in_srgb,var(--st-warn-dot)_20%,transparent)]",
        // 破坏性：停止 / 删除 — 实心红
        destructive:
          "border-[var(--st-danger)] bg-[var(--st-danger)] text-white shadow-[0_1px_2px_color-mix(in_srgb,var(--st-danger)_22%,transparent)] hover:border-[var(--st-danger-deep)] hover:bg-[var(--st-danger-deep)] hover:text-white",
        link: "h-auto border-transparent bg-transparent p-0 text-[var(--st-accent)] underline-offset-4 hover:underline active:scale-100",
      },
      size: {
        default:
          "min-h-9 px-3.5 py-2 text-sm leading-none has-data-[icon=inline-end]:pr-2.5 has-data-[icon=inline-start]:pl-2.5",
        xs: "min-h-7 gap-1 rounded-[var(--r-sm,8px)] px-2 text-[0.75rem] leading-none has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 [&_svg:not([class*='size-'])]:size-3",
        sm: "min-h-8 gap-1 px-3 py-1.5 text-[0.8125rem] leading-none has-data-[icon=inline-end]:pr-2 has-data-[icon=inline-start]:pl-2 [&_svg:not([class*='size-'])]:size-3.5",
        lg: "min-h-10 gap-1.5 px-4 py-2 text-sm leading-none has-data-[icon=inline-end]:pr-3 has-data-[icon=inline-start]:pl-3",
        icon: "size-9 rounded-[var(--r-sm,8px)]",
        "icon-xs": "size-7 rounded-[var(--r-sm,8px)] [&_svg:not([class*='size-'])]:size-3",
        "icon-sm": "size-8 rounded-[var(--r-sm,8px)]",
        "icon-lg": "size-10 rounded-[var(--r-sm,8px)]",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  }
)

function Button({
  className,
  variant = "default",
  size = "default",
  asChild = false,
  ...props
}: React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean
  }) {
  const Comp = asChild ? Slot.Root : "button"

  return (
    <Comp
      data-slot="button"
      data-variant={variant}
      data-size={size}
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  )
}

export { Button, buttonVariants }
