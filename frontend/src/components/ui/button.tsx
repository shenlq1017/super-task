import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { Slot } from "radix-ui"

import { cn } from "@/lib/utils"

const buttonVariants = cva(
  // 圆角统一 --r-sm(8px)；禁止在带文字的按钮上用 rounded-full（会裁切字/图标）
  // leading-none 不能放这里：tailwind-merge 会认为后面的 text-* 尺寸类与 leading 冲突并把它删掉，须写在各 size 的 text-* 之后
  "group/button inline-flex cursor-pointer shrink-0 items-center justify-center gap-1.5 overflow-visible rounded-[var(--r-sm,8px)] border bg-clip-padding font-semibold whitespace-nowrap transition-[color,background-color,border-color,box-shadow,transform,opacity] duration-150 ease-[var(--st-ease,cubic-bezier(.22,1,.36,1))] outline-none select-none focus-visible:outline-2 focus-visible:outline-[var(--st-accent,#5e6ad2)] focus-visible:outline-offset-2 focus-visible:ring-0 active:scale-[.97] disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-destructive [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-3.5",
  {
    variants: {
      variant: {
        // 主操作：启动 / 安装 / 主 CTA
        default:
          "border-[var(--st-accent,#5e6ad2)] bg-[var(--st-accent,#5e6ad2)] text-white shadow-[0_1px_2px_rgb(94_106_210_/_0.25)] hover:border-[var(--st-accent-hover,#4f5ac8)] hover:bg-[var(--st-accent-hover,#4f5ac8)] hover:shadow-[var(--st-glow,0_4px_16px_rgb(94_106_210_/_0.28))]",
        // 中性次操作：打开 / 收起 / 刷新 / 建议 — 白底 + 强描边
        outline:
          "border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] text-[var(--t1,#222326)] shadow-[0_1px_0_rgb(16_24_40_/_0.04)] hover:border-[var(--t3,#8a8f98)] hover:bg-[var(--surface-2,#f3f4f5)] aria-expanded:bg-[var(--surface-2,#f3f4f5)] aria-expanded:text-[var(--t1,#222326)]",
        // 辅助灰底：构建 jar 等
        secondary:
          "border-[var(--line-strong,#d0d6e0)] bg-[var(--surface-2,#f3f4f5)] text-[var(--t1,#222326)] hover:border-[var(--t3,#8a8f98)] hover:bg-[var(--surface,#fff)]",
        // 低强调：行内次要
        ghost:
          "border-transparent bg-transparent text-[var(--t2,#62666d)] hover:border-transparent hover:bg-[rgb(0_0_0_/_0.05)] hover:text-[var(--t1,#222326)] aria-expanded:bg-[rgb(0_0_0_/_0.05)] aria-expanded:text-[var(--t1,#222326)]",
        // 品牌软色：检查 / 探测 / 校验
        soft:
          "border-[rgb(94_106_210_/_0.45)] bg-[var(--st-accent-tint,#eef0fb)] text-[var(--st-accent,#5e6ad2)] hover:border-[var(--st-accent,#5e6ad2)] hover:bg-[rgb(94_106_210_/_0.16)]",
        // 成功软色：保存 / 确认写入
        success:
          "border-[rgb(39_166_68_/_0.45)] bg-[var(--st-ok-tint,#e9f7ed)] text-[var(--st-ok-deep,#1e7e35)] hover:border-[var(--st-ok,#27a644)] hover:bg-[rgb(39_166_68_/_0.18)]",
        // 警示软色：重启 / 升级
        warn:
          "border-[var(--st-warn-line,#f0dcb0)] bg-[var(--st-warn-tint,#fff8e1)] text-[var(--st-warn,#9a6700)] hover:border-[#E0C080] hover:bg-[rgb(234_179_8_/_0.2)]",
        // 破坏性：停止 / 删除 — 实心红
        destructive:
          "border-[var(--st-danger,#dc2626)] bg-[var(--st-danger,#dc2626)] text-white shadow-[0_1px_2px_rgb(220_38_38_/_0.22)] hover:border-[#B91C1C] hover:bg-[#B91C1C] hover:text-white",
        link: "h-auto border-transparent bg-transparent p-0 text-[var(--st-accent,#5e6ad2)] underline-offset-4 hover:underline active:scale-100",
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
