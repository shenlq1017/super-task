import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { Slot } from "radix-ui"

import { cn } from "@/lib/utils"

const badgeVariants = cva(
  "group/badge inline-flex w-fit shrink-0 items-center justify-center gap-1 overflow-hidden rounded-full border border-transparent px-2 py-0.5 text-xs font-medium whitespace-nowrap transition-all focus-visible:outline-2 focus-visible:outline-[var(--st-accent,#5e6ad2)] focus-visible:outline-offset-2 has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 [&>svg]:pointer-events-none [&>svg]:size-3!",
  {
    variants: {
      variant: {
        default: "bg-[var(--st-accent,#5e6ad2)] text-white",
        secondary:
          "h-auto rounded-full border-0 bg-[rgb(0_0_0_/_0.05)] px-1.5 py-0.5 font-mono text-[0.56rem] font-semibold text-[var(--t3,#8a8f98)]",
        destructive: "bg-[var(--st-danger-tint,#fdecec)] text-[var(--st-danger,#dc2626)]",
        outline:
          "border-[var(--line,#e6e6e6)] bg-[var(--surface,#fff)] text-[var(--t2,#62666d)]",
        ghost: "bg-transparent text-[var(--t3,#8a8f98)]",
        link: "bg-transparent text-[var(--st-accent,#5e6ad2)] underline-offset-4 hover:underline",
        soon:
          "h-auto rounded-full border border-[rgb(94_106_210_/_0.25)] bg-[var(--st-accent-tint,#eef0fb)] px-1.5 py-0.5 font-mono text-[0.56rem] font-semibold text-[var(--st-accent-hover,#4f5ac8)]",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  }
)

function Badge({
  className,
  variant = "default",
  asChild = false,
  ...props
}: React.ComponentProps<"span"> &
  VariantProps<typeof badgeVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot.Root : "span"

  return (
    <Comp
      data-slot="badge"
      data-variant={variant}
      className={cn(badgeVariants({ variant }), className)}
      {...props}
    />
  )
}

export { Badge, badgeVariants }
