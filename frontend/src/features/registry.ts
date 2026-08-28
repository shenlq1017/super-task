import type { Feature } from "../ipc/protocol";

export type NavGroup = "workspace" | "extend" | "system";

export type NavMeta = {
  label: string;
  group: NavGroup;
};

/** UI-only labels/groups. Status/path come from `session.hello`. */
export const NAV_META: Record<string, NavMeta> = {
  run: { label: "运行", group: "workspace" },
  logs: { label: "日志", group: "workspace" },
  config: { label: "配置", group: "workspace" },
  env: { label: "环境（探测）", group: "workspace" },
  workspaces: { label: "工作区", group: "workspace" },
  discover: { label: "发现", group: "workspace" },
  templates: { label: "模板", group: "workspace" },
  git: { label: "Git", group: "workspace" },
  docker: { label: "容器", group: "workspace" },
  gateway: { label: "网关", group: "extend" },
  cloud: { label: "云", group: "extend" },
  ai: { label: "AI", group: "extend" },
  settings: { label: "设置", group: "system" },
};

/** Sidebar group titles — mirror the clickable prototype (方案 H / Linear). */
export const GROUP_TITLE: Record<NavGroup, string> = {
  workspace: "主功能",
  extend: "未来版本",
  system: "系统",
};

/** Feature ids rendered by hand at the bottom of the sidebar (not in a group list). */
export const PINNED_NAV = ["settings"] as const;

const GROUP_ORDER: NavGroup[] = ["workspace", "extend", "system"];

export function navLabel(id: string): string {
  return NAV_META[id]?.label ?? id;
}

export function groupedFeatures(features: Feature[]): { group: NavGroup; items: Feature[] }[] {
  return GROUP_ORDER.map((group) => ({
    group,
    items: features.filter(
      (f) => (NAV_META[f.id]?.group ?? "extend") === group && !PINNED_NAV.includes(f.id as (typeof PINNED_NAV)[number]),
    ),
  })).filter((g) => g.items.length > 0);
}
