import * as React from "react";
import { cn } from "@/lib/utils";

export function Input({ className, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      className={cn(
        "flex h-8 w-full rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[var(--surface,#fff)] px-2.5 py-1 text-[0.8125rem] text-[var(--t1,#222326)] shadow-none outline-none transition-[border-color,box-shadow] duration-150 ease-[var(--st-ease,cubic-bezier(.22,1,.36,1))]",
        "placeholder:text-[var(--t3,#8a8f98)] focus-visible:border-[var(--st-accent,#5e6ad2)] focus-visible:ring-[3px] focus-visible:ring-[var(--st-accent-tint,#eef0fb)] focus-visible:ring-offset-0",
        "disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <textarea
      className={cn(
        "flex w-full rounded-[var(--r-md,12px)] border border-[var(--line-strong,#d0d6e0)] bg-[#FBFBFC] px-3 py-2 font-mono text-[0.71rem] leading-[1.8] text-[var(--t1,#222326)] shadow-none outline-none transition-[border-color,box-shadow] duration-150 ease-[var(--st-ease,cubic-bezier(.22,1,.36,1))]",
        "placeholder:text-[var(--t3,#8a8f98)] focus-visible:border-[var(--st-accent,#5e6ad2)] focus-visible:ring-[3px] focus-visible:ring-[var(--st-accent-tint,#eef0fb)] focus-visible:ring-offset-0",
        "disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}
