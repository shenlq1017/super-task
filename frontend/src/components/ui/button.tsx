import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { Slot } from "radix-ui"

import { cn } from "@/lib/utils"

const buttonVariants = cva(
  "group/button inline-flex cursor-pointer shrink-0 items-center justify-center gap-1.5 rounded-[var(--r-sm,8px)] border bg-clip-padding font-semibold leading-none whitespace-nowrap transition-all duration-150 ease-[var(--st-ease,cubic-bezier(.22,1,.36,1))] outline-none select-none focus-visible:outline-2 focus-visible:outline-[var(--st-accent,#5e6ad2)] focus-visible:outline-offset-2 focus-visible:ring-0 active:scale-[.97] disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-destructive [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-3.5",
  {
    variants: {
      variant: {
        default:
          "border-[var(--st-accent,#5e6ad2)] bg-[var(--st-accent,#5e6ad2)] text-white shadow-[0_1px_2px_rgb(94_106_210_/_0.25)] hover:border-[var(--st-accent-hover,#4f5ac8)] hover:bg-[var(--st-accent-hover,#4f5ac8)] hover:shadow-[var(--st-glow,0_4px_16px_rgb(94_106_210_/_0.28))]",
        outline:
          "border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] text-[var(--t1,#222326)] hover:border-[var(--t3,#8a8f98)] hover:bg-[#FCFCFD] aria-expanded:bg-[#FCFCFD] aria-expanded:text-[var(--t1,#222326)]",
        secondary:
          "border-[var(--line,#e6e6e6)] bg-[var(--surface-2,#f3f4f5)] text-[var(--t1,#222326)] hover:border-[var(--line-strong,#d0d6e0)] hover:bg-[var(--surface,#fff)]",
        ghost:
          "border-transparent bg-transparent text-[var(--t2,#62666d)] hover:border-transparent hover:bg-[rgb(0_0_0_/_0.05)] hover:text-[var(--t1,#222326)] aria-expanded:bg-[rgb(0_0_0_/_0.05)] aria-expanded:text-[var(--t1,#222326)]",
        destructive:
          "border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] text-[var(--t1,#222326)] hover:border-[var(--st-danger,#dc2626)] hover:bg-[var(--st-danger-tint,#fdecec)] hover:text-[var(--st-danger,#dc2626)]",
        link: "h-auto border-transparent bg-transparent p-0 text-[var(--st-accent,#5e6ad2)] underline-offset-4 hover:underline active:scale-100",
      },
      size: {
        default:
          "min-h-8 px-3.5 py-2 text-[0.78rem] has-data-[icon=inline-end]:pr-2.5 has-data-[icon=inline-start]:pl-2.5",
        xs: "min-h-6 gap-1 rounded-[var(--r-sm,8px)] px-2 text-[0.72rem] has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 [&_svg:not([class*='size-'])]:size-3",
        sm: "min-h-[1.7rem] gap-1 px-3 py-1.5 text-[0.75rem] has-data-[icon=inline-end]:pr-2 has-data-[icon=inline-start]:pl-2 [&_svg:not([class*='size-'])]:size-3.5",
        lg: "min-h-9 gap-1.5 px-4 py-2 text-[0.82rem] has-data-[icon=inline-end]:pr-3 has-data-[icon=inline-start]:pl-3",
        icon: "size-8",
        "icon-xs": "size-6 rounded-[var(--r-sm,8px)] [&_svg:not([class*='size-'])]:size-3",
        "icon-sm": "size-7 rounded-[var(--r-sm,8px)]",
        "icon-lg": "size-9",
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
