import type { TFunction } from "i18next";
import type { AppId } from "@/lib/api";
import type { McpServer, Provider } from "@/types";
import type { Prompt } from "@/lib/api/prompts";
import type { InstalledSkill } from "@/lib/api/skills";
import type { AppPage, GlobalPage, SettingsSection } from "@/lib/navigation";
import { SETTINGS_SECTIONS } from "@/lib/navigation";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { DIRECTORY_APPS } from "@/components/settings/sections/AppConfigSection";
import { extractProviderBaseUrl } from "@/utils/providerConfigUtils";
import { isLinux } from "@/lib/platform";

/**
 * 全局搜索的索引：页面、设置项、各页的条目（供应商、MCP、Skills、提示词、插件）。
 * 这里只产出纯数据；数据从哪来见 useGlobalSearchItems，选中后怎么走见 App 的 navigateToSearchTarget。
 */

export type OpenClawConfigTab = "env" | "tools" | "agents";

export type SearchTarget =
  | {
      kind: "app";
      app: AppId;
      page?: AppPage;
      openclawTab?: OpenClawConfigTab;
    }
  | { kind: "page"; page: GlobalPage }
  | {
      kind: "settings";
      section: SettingsSection;
      /** 要定位的锚点（`setting:<名称>`）；不给就只打开分组 */
      anchor?: string;
      /** 锚点因状态没画出来（如开机自启关着时的静默启动）时退而定位这个 */
      fallback?: string;
      /** 应用配置里某个应用的那一节 */
      appConfigApp?: AppId;
    }
  | { kind: "provider"; app: AppId; id: string }
  | { kind: "mcp"; id: string }
  | { kind: "skill"; id: string }
  | { kind: "prompt"; app: AppId; id: string };

export type SearchGroup =
  | "pages"
  | "settings"
  | "providers"
  | "mcp"
  | "skills"
  | "prompts";

export const SEARCH_GROUPS: SearchGroup[] = [
  "pages",
  "settings",
  "providers",
  "mcp",
  "skills",
  "prompts",
];

export interface SearchItem {
  /** 全局唯一 */
  id: string;
  group: SearchGroup;
  title: string;
  /** 位置说明，如「设置 › 通用」「Claude Code」 */
  subtitle?: string;
  /** 和标题同等对待的别名（英文名、应用 id） */
  aliases?: string[];
  /** 只用来匹配、排得靠后的文字（备注、地址、描述、正文） */
  details?: string[];
  /** 图标：应用图标用应用 id，否则用页面 / 种类的图标名 */
  icon: { app: AppId } | { page: GlobalPage | "settings" | AppPage };
  target: SearchTarget;
}

// ─── 页面 ─────────────────────────────────────────────────────────────────

const GLOBAL_PAGE_LABEL: Record<
  Exclude<GlobalPage, "skillsDiscovery">,
  string
> = {
  mcp: "MCP",
  skills: "Skills",
  prompts: "nav.prompts",
  sessions: "nav.sessions",
  auth: "nav.auth",
  usage: "nav.usage",
  apps: "nav.apps",
};

const tr = (t: TFunction, key: string) =>
  key.includes(".") ? (t(key) as string) : key;

/** 英文名做别名：中文界面里打英文也能搜到 */
const en = (t: TFunction, key: string) =>
  key.includes(".") ? (t(key, { lng: "en" }) as string) : key;

export function buildPageItems(t: TFunction, apps: AppId[]): SearchItem[] {
  const items: SearchItem[] = [];
  const providersLabel = t("appPage.providers");

  for (const app of apps) {
    const name = APP_DISPLAY_NAME[app];
    items.push({
      id: `page:app:${app}`,
      group: "pages",
      title: name,
      subtitle: providersLabel,
      aliases: [app, en(t, "appPage.providers")],
      icon: { app },
      target: { kind: "app", app },
    });
    if (app === "openclaw") {
      items.push({
        id: "page:openclaw:workspace",
        group: "pages",
        title: t("appPage.workspace"),
        subtitle: name,
        aliases: [en(t, "appPage.workspace")],
        icon: { app },
        target: { kind: "app", app, page: "workspace" },
      });
      for (const tab of ["env", "tools", "agents"] as const) {
        items.push({
          id: `page:openclaw:config:${tab}`,
          group: "pages",
          title: t(`openclaw.${tab}.title`),
          subtitle: `${name} › ${t("appPage.openclawConfig")}`,
          aliases: [en(t, `openclaw.${tab}.title`)],
          icon: { app },
          target: {
            kind: "app",
            app,
            page: "openclawConfig",
            openclawTab: tab,
          },
        });
      }
    }
    if (app === "hermes") {
      items.push({
        id: "page:hermes:memory",
        group: "pages",
        title: t("appPage.memory"),
        subtitle: name,
        aliases: [en(t, "appPage.memory")],
        icon: { app },
        target: { kind: "app", app, page: "hermesMemory" },
      });
    }
  }

  for (const [page, key] of Object.entries(GLOBAL_PAGE_LABEL) as Array<
    [Exclude<GlobalPage, "skillsDiscovery">, string]
  >) {
    items.push({
      id: `page:${page}`,
      group: "pages",
      title: tr(t, key),
      aliases: [en(t, key), page],
      icon: { page },
      target: { kind: "page", page },
    });
    if (page === "skills") {
      items.push({
        id: "page:skillsDiscovery",
        group: "pages",
        title: t("skillsPage.viewDiscover"),
        subtitle: "Skills",
        aliases: [en(t, "skillsPage.viewDiscover")],
        icon: { page: "skills" },
        target: { kind: "page", page: "skillsDiscovery" },
      });
    }
  }

  const settingsLabel = t("nav.settings");
  for (const section of SETTINGS_SECTIONS) {
    items.push({
      id: `page:settings:${section}`,
      group: "pages",
      title: t(`settings.sections.${section}`),
      subtitle: settingsLabel,
      aliases: [en(t, `settings.sections.${section}`), en(t, "nav.settings")],
      icon: { page: "settings" },
      target: { kind: "settings", section },
    });
  }
  return items;
}

// ─── 设置项 ───────────────────────────────────────────────────────────────

interface SettingEntry {
  section: SettingsSection;
  /** 设置行 / 分节标题的翻译键：显示名和页面上的锚点都由它算出 */
  label: string;
  /** 只在条件成立时才画出来的行，退而定位的那一行 */
  fallback?: string;
  /** 只在部分平台上有 */
  when?: () => boolean;
}

/**
 * 设置页里能单独定位的项。名称必须和设置页上画出来的一致（SettingsRow / SettingsBlock 的
 * label / title 用的就是这些键），锚点按名称自动挂上，见 SettingsLayout。
 */
export const SETTING_ENTRIES: SettingEntry[] = [
  // 通用
  { section: "general", label: "settings.language" },
  { section: "general", label: "settings.theme" },
  { section: "general", label: "settings.general.quotaDisplay" },
  { section: "general", label: "settings.general.density" },
  { section: "general", label: "settings.general.zoom" },
  { section: "general", label: "settings.general.visibleApps" },
  { section: "general", label: "settings.appVisibility.showProfileSwitcher" },
  { section: "general", label: "settings.general.checkToolUpdates" },
  { section: "general", label: "settings.launchOnStartup" },
  {
    section: "general",
    label: "settings.silentStartup",
    fallback: "settings.launchOnStartup",
  },
  { section: "general", label: "settings.minimizeToTray" },
  {
    section: "general",
    label: "settings.general.trayStyle",
    when: () => !isLinux(),
  },
  {
    section: "general",
    label: "settings.useAppWindowControls",
    when: () => isLinux(),
  },
  { section: "general", label: "settings.terminal.title" },
  // 应用配置（各应用的配置目录另见 buildSettingItems）
  { section: "appConfig", label: "settings.enableClaudePluginIntegration" },
  { section: "appConfig", label: "settings.skipClaudeOnboarding" },
  { section: "appConfig", label: "settings.codexQuotaProtection.title" },
  {
    section: "appConfig",
    label: "settings.preserveCodexOfficialAuthOnSwitch",
  },
  { section: "appConfig", label: "settings.unifyCodexSessionHistory" },
  { section: "appConfig", label: "settings.codexSessionCompression" },
  // 本地路由
  { section: "routing", label: "routingSettings.service" },
  { section: "routing", label: "routingSettings.listen" },
  { section: "routing", label: "routingSettings.clientHost" },
  { section: "routing", label: "routingSettings.logging" },
  { section: "routing", label: "settings.advanced.failover.title" },
  { section: "routing", label: "settings.advanced.rectifier.title" },
  // 网络
  { section: "network", label: "settings.advanced.globalProxy.title" },
  // 数据
  { section: "data", label: "settings.appConfigDir" },
  { section: "data", label: "settings.advanced.data.title" },
  { section: "data", label: "settings.advanced.backup.title" },
  { section: "data", label: "settings.advanced.cloudSync.title" },
  { section: "data", label: "settings.advanced.logConfig.title" },
];

export function buildSettingItems(t: TFunction): SearchItem[] {
  const settingsLabel = t("nav.settings");
  const where = (section: SettingsSection) =>
    `${settingsLabel} › ${t(`settings.sections.${section}`)}`;
  const items: SearchItem[] = SETTING_ENTRIES.filter(
    (entry) => entry.when?.() ?? true,
  ).map((entry) => ({
    id: `setting:${entry.section}:${entry.label}`,
    group: "settings",
    title: t(entry.label),
    subtitle: where(entry.section),
    aliases: [en(t, entry.label)],
    icon: { page: "settings" },
    target: {
      kind: "settings",
      section: entry.section,
      anchor: `setting:${t(entry.label)}`,
      fallback: entry.fallback ? `setting:${t(entry.fallback)}` : undefined,
    },
  }));

  const configDir = t("settings.appConfig.configDir");
  for (const app of DIRECTORY_APPS) {
    items.push({
      id: `setting:appConfig:dir:${app}`,
      group: "settings",
      title: `${APP_DISPLAY_NAME[app]} · ${configDir}`,
      subtitle: where("appConfig"),
      aliases: [app, en(t, "settings.appConfig.configDir")],
      icon: { app },
      target: { kind: "settings", section: "appConfig", appConfigApp: app },
    });
  }
  return items;
}

// ─── 条目 ─────────────────────────────────────────────────────────────────

export function buildProviderItems(
  app: AppId,
  providers: Record<string, Provider> | undefined,
): SearchItem[] {
  if (!providers) return [];
  return Object.values(providers).map((provider) => ({
    id: `provider:${app}:${provider.id}`,
    group: "providers",
    title: provider.name || provider.id,
    subtitle: APP_DISPLAY_NAME[app],
    details: [
      provider.notes,
      provider.websiteUrl,
      extractProviderBaseUrl(provider.settingsConfig),
    ].filter((value): value is string => Boolean(value)),
    icon: { app },
    target: { kind: "provider", app, id: provider.id },
  }));
}

/**
 * MCP 能搜的字段（显式白名单）：env 和 headers 里可能有凭据，绝不能进可搜的文字。
 */
export function mcpSearchDetails(id: string, server: McpServer): string[] {
  const spec = server.server ?? {};
  const values: unknown[] = [
    id,
    server.id,
    server.description,
    ...(Array.isArray(server.tags) ? server.tags : []),
    spec.type,
    spec.command,
    ...(Array.isArray(spec.args) ? spec.args : []),
    spec.cwd,
    spec.url,
    server.homepage,
    server.docs,
    server.source,
  ];
  return values.filter(
    (value): value is string => typeof value === "string" && value !== "",
  );
}

export function buildMcpItems(
  servers: Record<string, McpServer> | undefined,
): SearchItem[] {
  if (!servers) return [];
  return Object.entries(servers).map(([id, server]) => ({
    id: `mcp:${id}`,
    group: "mcp",
    title: server.name || id,
    subtitle: "MCP",
    details: mcpSearchDetails(id, server),
    icon: { page: "mcp" },
    target: { kind: "mcp", id },
  }));
}

export function buildSkillItems(
  skills: InstalledSkill[] | undefined,
): SearchItem[] {
  if (!skills) return [];
  return skills.map((skill) => {
    const repo =
      skill.repoOwner && skill.repoName
        ? `${skill.repoOwner}/${skill.repoName}`
        : undefined;
    return {
      id: `skill:${skill.id}`,
      group: "skills",
      title: skill.name || skill.id,
      subtitle: repo ?? "Skills",
      details: [skill.id, skill.description, skill.directory, repo].filter(
        (value): value is string => Boolean(value),
      ),
      icon: { page: "skills" },
      target: { kind: "skill", id: skill.id },
    };
  });
}

export function buildPromptItems(
  app: AppId,
  prompts: Record<string, Prompt> | undefined,
): SearchItem[] {
  if (!prompts) return [];
  return Object.entries(prompts).map(([recordId, prompt]) => ({
    id: `prompt:${app}:${recordId}`,
    group: "prompts",
    title: prompt.name || prompt.id || recordId,
    subtitle: APP_DISPLAY_NAME[app],
    details: [recordId, prompt.id, prompt.description, prompt.content].filter(
      (value): value is string => Boolean(value),
    ),
    icon: { app },
    target: { kind: "prompt", app, id: recordId },
  }));
}

// ─── 匹配 ─────────────────────────────────────────────────────────────────

const normalize = (value: string) => value.toLocaleLowerCase().trim();

/**
 * 按空格拆词，每个词都要在标题、别名、位置或细节里出现。
 * 得分：标题开头 > 标题包含 > 别名 > 位置 > 细节；不匹配返回 0。
 */
export function scoreSearchItem(item: SearchItem, query: string): number {
  const tokens = normalize(query).split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return 1;
  const title = normalize(item.title);
  const aliases = (item.aliases ?? []).map(normalize);
  const subtitle = normalize(item.subtitle ?? "");
  const details = (item.details ?? []).map(normalize);
  let score = 0;
  for (const token of tokens) {
    if (title.startsWith(token)) score += 100;
    else if (title.includes(token)) score += 60;
    else if (aliases.some((alias) => alias.includes(token))) score += 40;
    else if (subtitle.includes(token)) score += 20;
    else if (details.some((detail) => detail.includes(token))) score += 5;
    else return 0;
  }
  // 完全同名排最前；标题越短越靠前（同分时「MCP」先于「MCP 服务器配置…」）
  if (title === normalize(query)) score += 50;
  return score - Math.min(title.length, 40) / 100;
}

export interface SearchResultGroup {
  group: SearchGroup;
  items: SearchItem[];
  /** 没显示出来的条数 */
  hidden: number;
}

export function searchItems(
  items: SearchItem[],
  query: string,
  limitPerGroup = 8,
): SearchResultGroup[] {
  const byGroup = new Map<SearchGroup, Array<[SearchItem, number]>>();
  for (const item of items) {
    const score = scoreSearchItem(item, query);
    if (score <= 0) continue;
    const list = byGroup.get(item.group) ?? [];
    list.push([item, score]);
    byGroup.set(item.group, list);
  }
  const hasQuery = query.trim() !== "";
  return SEARCH_GROUPS.flatMap((group) => {
    const scored = byGroup.get(group);
    if (!scored?.length) return [];
    // 没输入时保持原有顺序（侧栏顺序），有输入时按得分
    if (hasQuery) scored.sort((a, b) => b[1] - a[1]);
    const shown = scored.slice(0, limitPerGroup).map(([item]) => item);
    return [{ group, items: shown, hidden: scored.length - shown.length }];
  });
}
