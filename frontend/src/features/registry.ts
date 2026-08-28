import type { Feature } from "../ipc/protocol";

export type NavGroup = "workspace" | "extend" | "system";

/**
 * UI-only nav metadata。label 为 i18n key（`nav.<labelKey>`，1.4 规格 §6.2），
 * 文案在 `src/i18n/locales/*` 维护；Status/path come from `session.hello`。
 */
export type NavMeta = {
  labelKey: string;
  group: NavGroup;
};

export const NAV_META: Record<string, NavMeta> = {
  run: { labelKey: "run", group: "workspace" },
  logs: { labelKey: "logs", group: "workspace" },
  config: { labelKey: "config", group: "workspace" },
  env: { labelKey: "env", group: "workspace" },
  workspaces: { labelKey: "workspaces", group: "workspace" },
  discover: { labelKey: "discover", group: "workspace" },
  templates: { labelKey: "templates", group: "workspace" },
  git: { labelKey: "git", group: "workspace" },
  docker: { labelKey: "docker", group: "workspace" },
  gateway: { labelKey: "gateway", group: "extend" },
  cloud: { labelKey: "cloud", group: "extend" },
  ai: { labelKey: "ai", group: "extend" },
  settings: { labelKey: "settings", group: "system" },
};

/** Feature ids rendered by hand at the bottom of the sidebar (not in a group list). */
export const PINNED_NAV = ["settings"] as const;

const GROUP_ORDER: NavGroup[] = ["workspace", "extend", "system"];

/** feature id → i18n key（`nav.<labelKey>`）；未知 id 返回 null。 */
export function navTranslationKey(id: string): string | null {
  const meta = NAV_META[id];
  return meta ? `nav.${meta.labelKey}` : null;
}

export function groupedFeatures(features: Feature[]): { group: NavGroup; items: Feature[] }[] {
  return GROUP_ORDER.map((group) => ({
    group,
    items: features.filter(
      (f) => (NAV_META[f.id]?.group ?? "extend") === group && !PINNED_NAV.includes(f.id as (typeof PINNED_NAV)[number]),
    ),
  })).filter((g) => g.items.length > 0);
}
