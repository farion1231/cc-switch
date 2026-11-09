import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import type { TFunction } from "i18next";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import tr from "@/i18n/locales/tr.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import type { McpServer, Provider } from "@/types";
import {
  SEARCH_GROUPS,
  SETTING_ENTRIES,
  buildMcpItems,
  buildPageItems,
  buildProviderItems,
  buildSettingItems,
  mcpSearchDetails,
  scoreSearchItem,
  searchItems,
  type SearchItem,
} from "@/components/search/searchIndex";
import {
  SettingsBlock,
  SettingsRow,
  SettingsSwitchRow,
} from "@/components/settings/SettingsLayout";

/** 测试里的 t：原样返回键名，带 lng 时加上语言前缀，方便断言别名。 */
const t = ((key: string, options?: { lng?: string }) =>
  options?.lng ? `${options.lng}:${key}` : key) as unknown as TFunction;

const item = (overrides: Partial<SearchItem>): SearchItem => ({
  id: overrides.id ?? overrides.title ?? "item",
  group: "pages",
  title: "Item",
  icon: { page: "mcp" },
  target: { kind: "page", page: "mcp" },
  ...overrides,
});

function getTranslation(locale: unknown, path: string): unknown {
  return path.split(".").reduce<unknown>((value, key) => {
    if (!value || typeof value !== "object") return undefined;
    return (value as Record<string, unknown>)[key];
  }, locale);
}

const locales = [
  ["en", en],
  ["zh", zh],
  ["zh-TW", zhTW],
  ["ja", ja],
  ["tr", tr],
] as const;

describe("scoreSearchItem", () => {
  it("ranks a title prefix above a title match, aliases, location and details", () => {
    const prefix = item({ title: "Proxy settings" });
    const inTitle = item({ title: "Global proxy" });
    const alias = item({ title: "网络代理", aliases: ["proxy"] });
    const subtitle = item({ title: "Listen", subtitle: "Proxy" });
    const detail = item({ title: "Relay", details: ["https://proxy.example"] });

    const scores = [prefix, inTitle, alias, subtitle, detail].map((entry) =>
      scoreSearchItem(entry, "proxy"),
    );
    expect(scores.every((score) => score > 0)).toBe(true);
    expect([...scores].sort((a, b) => b - a)).toEqual(scores);
  });

  it("requires every whitespace-separated token to match somewhere", () => {
    const entry = item({ title: "Claude Code", subtitle: "Providers" });
    expect(scoreSearchItem(entry, "claude prov")).toBeGreaterThan(0);
    expect(scoreSearchItem(entry, "claude codex")).toBe(0);
  });

  it("matches case-insensitively and prefers an exact title", () => {
    const exact = item({ title: "MCP" });
    const longer = item({ title: "MCP servers config" });
    expect(scoreSearchItem(exact, "mcp")).toBeGreaterThan(
      scoreSearchItem(longer, "mcp"),
    );
  });
});

describe("searchItems", () => {
  it("returns groups in a fixed order, sorted by score and capped per group", () => {
    const providers = Array.from({ length: 10 }, (_, index) =>
      item({
        id: `p${index}`,
        group: "providers",
        title: index === 9 ? "Relay" : `Other relay ${index}`,
      }),
    );
    const page = item({ id: "page", group: "pages", title: "Relay page" });
    const groups = searchItems([...providers, page], "relay", 3);

    expect(groups.map((group) => group.group)).toEqual(["pages", "providers"]);
    const providerGroup = groups[1];
    expect(providerGroup.items).toHaveLength(3);
    expect(providerGroup.items[0].id).toBe("p9");
    expect(providerGroup.hidden).toBe(7);
  });

  it("leaves out groups without matches", () => {
    expect(searchItems([item({ title: "Theme" })], "nothing")).toEqual([]);
  });
});

describe("page and setting items", () => {
  it("lists visible apps, their sub-pages, global pages and settings sections", () => {
    const pages = buildPageItems(t, ["claude", "openclaw"]);
    const ids = pages.map((page) => page.id);
    expect(ids).toContain("page:app:claude");
    expect(ids).toContain("page:openclaw:workspace");
    expect(ids).toContain("page:openclaw:config:tools");
    expect(ids).not.toContain("page:hermes:memory");
    expect(ids).toContain("page:mcp");
    expect(ids).toContain("page:skillsDiscovery");
    expect(ids).toContain("page:settings:routing");
    // 英文名作别名：中文界面打英文也能搜到
    expect(pages.find((page) => page.id === "page:usage")?.aliases).toContain(
      "en:nav.usage",
    );
  });

  it("points each setting at the anchor its settings row renders", () => {
    const language = buildSettingItems(t).find(
      (entry) => entry.id === "setting:general:settings.language",
    );
    expect(language?.target).toEqual({
      kind: "settings",
      section: "general",
      anchor: "setting:settings.language",
      fallback: undefined,
    });

    const { container } = render(
      <>
        <SettingsSwitchRow
          label={t("settings.language")}
          checked={false}
          onCheckedChange={() => undefined}
        />
        <SettingsRow label={t("settings.theme")} />
        <SettingsBlock title={t("settings.advanced.backup.title")}>
          <div />
        </SettingsBlock>
      </>,
    );
    for (const anchor of [
      "setting:settings.language",
      "setting:settings.theme",
      "setting:settings.advanced.backup.title",
    ]) {
      expect(
        container.querySelector(`[data-search-anchor="${anchor}"]`),
      ).not.toBeNull();
    }
  });

  it("gives every app's config directory its own entry", () => {
    const dirs = buildSettingItems(t).filter((entry) =>
      entry.id.startsWith("setting:appConfig:dir:"),
    );
    expect(dirs.length).toBeGreaterThan(0);
    expect(dirs[0].target).toMatchObject({
      kind: "settings",
      section: "appConfig",
      appConfigApp: expect.any(String),
    });
  });

  it.each(locales)(
    "translates every indexed setting and search string in %s",
    (_name, locale) => {
      const keys = [
        ...SETTING_ENTRIES.flatMap((entry) =>
          entry.fallback ? [entry.label, entry.fallback] : [entry.label],
        ),
        "settings.appConfig.configDir",
        ...[
          "title",
          "trigger",
          "triggerTip",
          "placeholder",
          "loading",
          "empty",
          "more",
          "hintMove",
          "hintOpen",
          "hintClose",
        ].map((key) => `globalSearch.${key}`),
        ...SEARCH_GROUPS.map((group) => `globalSearch.groups.${group}`),
      ];
      const missing = keys.filter((key) => {
        const value = getTranslation(locale, key);
        return typeof value !== "string" || !value.trim();
      });
      expect(missing).toEqual([]);
    },
  );
});

describe("entry items", () => {
  it("searches providers by notes, website and request address", () => {
    const provider = {
      id: "relay",
      name: "Relay",
      notes: "team account",
      websiteUrl: "https://relay.example",
      settingsConfig: {
        env: { ANTHROPIC_BASE_URL: "https://api.relay.example" },
      },
    } as unknown as Provider;
    const [entry] = buildProviderItems("claude", { relay: provider });

    expect(entry.target).toEqual({
      kind: "provider",
      app: "claude",
      id: "relay",
    });
    for (const query of ["team", "relay.example", "api.relay"]) {
      expect(scoreSearchItem(entry, query)).toBeGreaterThan(0);
    }
  });

  it("never makes MCP env or header values searchable", () => {
    const server = {
      id: "fetch",
      name: "Fetch",
      description: "Fetch pages",
      server: {
        type: "stdio",
        command: "npx",
        args: ["fetch-server"],
        env: { API_KEY: "secret-env-value" },
        headers: { Authorization: "secret-header-value" },
      },
      apps: {},
    } as unknown as McpServer;

    const details = mcpSearchDetails("fetch", server);
    expect(details).toContain("fetch-server");
    expect(details.join("\n")).not.toContain("secret");

    const [entry] = buildMcpItems({ fetch: server });
    expect(scoreSearchItem(entry, "fetch-server")).toBeGreaterThan(0);
    expect(scoreSearchItem(entry, "secret-env-value")).toBe(0);
    expect(scoreSearchItem(entry, "secret-header-value")).toBe(0);
  });
});
