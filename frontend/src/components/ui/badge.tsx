import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { Slot } from "radix-ui"

import { cn } from "@/lib/utils"

const badgeVariants = cva(
  "group/badge inline-flex h-5 w-fit shrink-0 items-center justify-center gap-1 overflow-hidden rounded-full border border-transparent px-2 text-xs font-medium leading-none whitespace-nowrap transition-all focus-visible:outline-2 focus-visible:outline-[var(--st-accent)] focus-visible:outline-offset-2 has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 [&>svg]:pointer-events-none [&>svg]:size-3!",
  {
    variants: {
      variant: {
        default: "bg-[var(--st-accent)] text-[var(--primary-foreground)]",
        secondary:
          "rounded-full border-0 bg-[var(--surface-2)] px-1.5 font-mono text-[0.62rem] font-semibold leading-none text-[var(--t3)]",
        destructive: "bg-[var(--st-danger-tint)] text-[var(--st-danger)]",
        outline:
          "border-[var(--line)] bg-[var(--surface)] text-[var(--t2)]",
        ghost: "bg-transparent text-[var(--t3)]",
        link: "bg-transparent text-[var(--st-accent)] underline-offset-4 hover:underline",
        soon:
          "rounded-full border border-[var(--line-strong)] bg-[var(--surface-2)] px-1.5 font-mono text-[0.62rem] font-semibold leading-none text-[var(--t3)]",
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
