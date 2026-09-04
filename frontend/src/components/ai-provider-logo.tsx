import { useState } from "react";
import { Settings2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { AI_PROVIDER_PRESETS } from "@/lib/ai-providers";
import type { AiProviderKey } from "../ipc/protocol";

const INVERT_IN_DARK = new Set([
  "openai-compatible", "claude", "anthropic-compatible", "ollama",
  "opencode-cli", "cursor-cli",
]);

export function AiProviderLogo({ provider, className }: { provider: AiProviderKey; className?: string }) {
  const [failed, setFailed] = useState(false);
  const preset = AI_PROVIDER_PRESETS[provider];
  if (provider === "custom") {
    return <Settings2 className={cn("size-4 text-[var(--t3)]", className)} aria-hidden />;
  }
  if (preset.icon && !failed) {
    return (
      <img
        src={`/icons/ai/${preset.icon}.svg`}
        alt=""
        onError={() => setFailed(true)}
        className={cn("size-4 shrink-0 object-contain", INVERT_IN_DARK.has(provider) && "dark:invert", className)}
      />
    );
  }
  return (
    <span className={cn("flex size-4 shrink-0 items-center justify-center rounded bg-[var(--surface-3)] text-[8px] font-bold", className)} aria-hidden>
      {preset.label.slice(0, 1).toUpperCase()}
    </span>
  );
}
