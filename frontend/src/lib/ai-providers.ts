import type { AiProviderKey } from "../ipc/protocol";

export type AiProviderPreset = {
  label: string;
  group: "api" | "cli";
  endpoint: string;
  model: string;
  icon?: string;
  keyOptional?: boolean;
  cliProgram?: string;
  cliArgs?: string[];
};

/** Mirror `supertask_core::ai::PROVIDER_PRESETS`.
 * CLI argv defaults follow dbx's non-interactive execution modes, but stay
 * editable because CLI flags evolve independently from SuperTask. */
export const AI_PROVIDER_PRESETS: Record<AiProviderKey, AiProviderPreset> = {
  "openai-compatible": {
    label: "OpenAI Compatible", group: "api", endpoint: "https://api.openai.com/v1",
    model: "gpt-4o-mini", icon: "openai",
  },
  claude: {
    label: "Claude", group: "api", endpoint: "https://api.anthropic.com",
    model: "claude-sonnet-4-5", icon: "anthropic",
  },
  "anthropic-compatible": {
    label: "Anthropic Compatible", group: "api", endpoint: "", model: "",
    icon: "anthropic", keyOptional: true,
  },
  gemini: {
    label: "Gemini", group: "api", endpoint: "https://generativelanguage.googleapis.com/v1beta/openai",
    model: "gemini-2.0-flash", icon: "googlegemini",
  },
  deepseek: {
    label: "DeepSeek", group: "api", endpoint: "https://api.deepseek.com",
    model: "deepseek-chat", icon: "deepseek",
  },
  kimi: {
    label: "Kimi", group: "api", endpoint: "https://api.moonshot.cn/v1",
    model: "kimi-k2-0905-preview",
  },
  qwen: {
    label: "Qwen", group: "api", endpoint: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    model: "qwen-plus", icon: "alibabacloud",
  },
  minimax: {
    label: "MiniMax", group: "api", endpoint: "https://api.minimaxi.com/v1",
    model: "MiniMax-Text-01", icon: "minimax",
  },
  ollama: {
    label: "Ollama", group: "api", endpoint: "http://localhost:11434/v1",
    model: "qwen2.5:7b", icon: "ollama", keyOptional: true,
  },
  "claude-code-cli": {
    label: "Claude Code CLI", group: "cli", endpoint: "", model: "default",
    icon: "claudecode", cliProgram: "claude",
    cliArgs: ["--print", "--output-format", "stream-json", "--verbose", "--input-format", "text", "--no-session-persistence", "--permission-mode", "dontAsk", "--tools", ""],
  },
  "codex-cli": {
    label: "Codex CLI", group: "cli", endpoint: "", model: "default",
    icon: "codex", cliProgram: "codex",
    cliArgs: ["exec", "--json", "--skip-git-repo-check", "--sandbox", "read-only", "-c", "features.shell_tool=false", "-c", "web_search=\"disabled\"", "-"],
  },
  "opencode-cli": {
    label: "OpenCode CLI", group: "cli", endpoint: "", model: "default",
    icon: "opencode", cliProgram: "opencode", cliArgs: ["run", "--format", "json", "--pure"],
  },
  "cursor-cli": {
    label: "Cursor CLI", group: "cli", endpoint: "", model: "default",
    icon: "cursor", cliProgram: "cursor-agent", cliArgs: ["--print", "--output-format", "text"],
  },
  "codebuddy-cli": {
    label: "CodeBuddy Code", group: "cli", endpoint: "", model: "default",
    icon: "codebuddy", cliProgram: "codebuddy",
    cliArgs: ["--print", "--output-format", "stream-json", "--verbose"],
  },
  "qoder-cli": {
    label: "Qoder CLI", group: "cli", endpoint: "", model: "default",
    cliProgram: "qodercli", cliArgs: ["--print", "--output-format", "stream-json"],
  },
  "pi-agent-cli": {
    label: "Pi Coding Agent", group: "cli", endpoint: "", model: "default",
    icon: "pi", cliProgram: "pi", cliArgs: ["--print"],
  },
  custom: { label: "Custom", group: "api", endpoint: "", model: "" },
};

export const API_PROVIDERS = (Object.keys(AI_PROVIDER_PRESETS) as AiProviderKey[])
  .filter((key) => key !== "custom" && AI_PROVIDER_PRESETS[key].group === "api");
export const CLI_PROVIDERS = (Object.keys(AI_PROVIDER_PRESETS) as AiProviderKey[])
  .filter((key) => AI_PROVIDER_PRESETS[key].group === "cli");

export function isCliProvider(provider: AiProviderKey): boolean {
  return AI_PROVIDER_PRESETS[provider].group === "cli";
}
