import { useState } from "react";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { TFunction } from "i18next";
import type { ProviderCategory } from "@/types";
import {
  ProviderPresetSelector,
  filterPresetEntries,
  getPresetDisplayName,
  getVisiblePresetEntries,
  getVisiblePresetRows,
  type PresetEntry,
} from "@/components/providers/forms/ProviderPresetSelector";
import {
  PresetStepContext,
  type PresetStepState,
} from "@/components/providers/forms/presetStep";
import {
  domainBody,
  groupPresetRows,
  presetGroup,
} from "@/components/providers/forms/presetGroups";
import { providerPresets } from "@/config/claudeProviderPresets";
import { PRESET_FAMILIES, PRESET_VERSION_KEYS } from "@/config/presetFamilies";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: ({ name }: { name: string }) => (
    <span data-testid="provider-icon" data-name={name} />
  ),
}));

const translations: Record<string, string> = {
  "preset.alpha": "Alpha 本地名",
  "preset.zhipu": "智谱 GLM",
};
const t = ((key: string) => translations[key] ?? key) as TFunction;

function preset(
  name: string,
  category: ProviderCategory,
  websiteUrl: string,
  extra: Record<string, unknown> = {},
) {
  return {
    name,
    websiteUrl,
    settingsConfig: {},
    category,
    ...extra,
  } as PresetEntry["preset"];
}

const entries: PresetEntry[] = [
  {
    id: "gamma",
    preset: preset("Gamma", "aggregator", "https://api.gamma.com"),
  },
  {
    id: "alpha",
    preset: preset("Alpha Raw", "official", "https://alpha.example.com", {
      nameKey: "preset.alpha",
    }),
  },
  {
    id: "huoshan",
    preset: preset("火山引擎", "cn_official", "https://www.volcengine.com"),
  },
  {
    id: "zhipu",
    preset: preset("Zhipu GLM", "cn_official", "https://open.bigmodel.cn", {
      nameKey: "preset.zhipu",
    }),
  },
  {
    id: "bedrock",
    preset: preset("AWS Bedrock", "cloud_provider", "https://aws.amazon.com"),
  },
  {
    id: "copilot",
    preset: preset("GitHub Copilot", "third_party", "https://github.com", {
      providerType: "github_copilot",
    }),
  },
];

// 同一家的三个版本（按预设文件顺序；显示时按 versionOrder 排），外加一个单独的预设
const kimiEntries: PresetEntry[] = [
  {
    id: "kimi-cn",
    preset: preset("Kimi", "cn_official", "https://platform.kimi.com", {
      family: "kimi",
      versionKey: "paygCn",
    }),
  },
  {
    id: "kimi-intl",
    preset: preset("Kimi Global", "cn_official", "https://platform.kimi.ai", {
      family: "kimi",
      versionKey: "paygIntl",
    }),
  },
  {
    id: "kimi-coding",
    preset: preset("Kimi For Coding", "cn_official", "https://www.kimi.com", {
      family: "kimi",
      versionKey: "codingCn",
    }),
  },
];
const familyEntries: PresetEntry[] = [...entries, ...kimiEntries];

describe("preset helpers", () => {
  it("groups presets from existing fields without touching category", () => {
    expect(entries.map((entry) => presetGroup(entry.preset))).toEqual([
      "thirdparty",
      "login",
      "vendor",
      "vendor",
      "cloud",
      "login",
    ]);
  });

  it("sorts only by name, with Chinese names placed by pinyin initial", () => {
    expect(
      getVisiblePresetEntries(entries, { query: "", t }).map(
        (entry) => entry.id,
      ),
    ).toEqual(["alpha", "bedrock", "gamma", "copilot", "huoshan", "zhipu"]);
  });

  it("searches display names, domains and aliases but not TLDs", () => {
    expect(domainBody("api.gamma.com")).toBe("gamma");
    const ids = (query: string) =>
      filterPresetEntries(entries, query, t).map((entry) => entry.id);
    expect(ids("智谱")).toEqual(["zhipu"]);
    expect(ids("bigmodel")).toEqual(["zhipu"]);
    expect(ids("com")).toEqual([]);
    expect(getPresetDisplayName(entries[1].preset, t)).toBe("Alpha 本地名");
  });
});

describe("preset families", () => {
  it("merges the versions of one vendor into a single row", () => {
    const rows = groupPresetRows(familyEntries);
    expect(rows).toHaveLength(entries.length + 1);
    const kimi = rows.find((row) => row.family === "kimi");
    expect(kimi?.versions.map((entry) => entry.id)).toEqual([
      "kimi-cn",
      "kimi-coding",
      "kimi-intl",
    ]);
  });

  it("matches the whole vendor by name, or the versions a query names", () => {
    const hits = (query: string) =>
      getVisiblePresetRows(familyEntries, { query, t }).map(({ row, hits }) => [
        row.key,
        hits,
      ]);
    expect(hits("kimi")).toEqual([["family:kimi", []]]);
    expect(hits("coding")).toEqual([["family:kimi", [1]]]);
    // 带点的词可以命中某个版本自己的完整域名
    expect(hits("kimi.ai")).toEqual([["family:kimi", [2]]]);
    // 别名算整家命中
    expect(hits("moonshot")).toEqual([["family:kimi", []]]);
  });

  it("merges Claude's 94 presets into 71 rows", () => {
    const claudeEntries = providerPresets
      .filter((item) => !item.hidden)
      .map((item, index) => ({ id: `claude-${index}`, preset: item }));
    expect(groupPresetRows(claudeEntries)).toHaveLength(71);
  });

  it("orders versions as the design does where a family says so", () => {
    const claudeEntries = providerPresets
      .filter((item) => !item.hidden)
      .map((item, index) => ({ id: `claude-${index}`, preset: item }));
    const versionsOf = (family: string) =>
      groupPresetRows(claudeEntries)
        .find((row) => row.family === family)
        ?.versions.map((entry) => entry.preset.versionKey);
    expect(versionsOf("kimi")).toEqual([
      "paygCn",
      "codingCn",
      "paygIntl",
      "codingIntl",
    ]);
    expect(versionsOf("tencent")?.slice(2, 4)).toEqual([
      "enterpriseLiteCn",
      "enterpriseLiteIntl",
    ]);
    // 没写 versionOrder 的保持文件顺序
    expect(versionsOf("volcengine")).toEqual([
      "agentPlan",
      "codingPlan",
      "payg",
    ]);
  });

  it("has every family name and version label in all four locales", () => {
    const lookup = (data: unknown, key: string) =>
      key
        .split(".")
        .reduce<unknown>(
          (node, part) =>
            node && typeof node === "object"
              ? (node as Record<string, unknown>)[part]
              : undefined,
          data,
        );
    const keys = [
      ...PRESET_VERSION_KEYS.map((key) => `providerPreset.version.${key}`),
      ...Object.values(PRESET_FAMILIES).flatMap((info) =>
        "nameKey" in info ? [info.nameKey] : [],
      ),
    ];
    for (const locale of [zh, zhTW, en, ja]) {
      for (const key of keys) {
        expect(typeof lookup(locale, key), key).toBe("string");
      }
    }
  });
});

function TwoSteps({
  onPresetChange,
  presetEntries = entries,
}: {
  onPresetChange: (id: string) => void;
  presetEntries?: PresetEntry[];
}) {
  const [step, setStep] = useState<"pick" | "form">("pick");
  const [host, setHost] = useState<HTMLDivElement | null>(null);
  const [selected, setSelected] = useState<string | null>("custom");
  // 表单的替身：选预设时程序重填（清空 Key），用户可以手动输入
  const [apiKey, setApiKey] = useState("");
  const state: PresetStepState = {
    appId: "claude",
    step,
    setStep,
    host,
    registerSelector: () => () => undefined,
  };
  return (
    <PresetStepContext.Provider value={state}>
      {step === "pick" && <div data-testid="host" ref={setHost} />}
      <form>
        <ProviderPresetSelector
          selectedPresetId={selected}
          presetEntries={presetEntries}
          onPresetChange={(id) => {
            setSelected(id);
            setApiKey("");
            onPresetChange(id);
          }}
        />
        {step === "form" && (
          <input
            aria-label="api-key"
            value={apiKey}
            onChange={(event) => setApiKey(event.target.value)}
          />
        )}
      </form>
    </PresetStepContext.Provider>
  );
}

describe("ProviderPresetSelector", () => {
  it("picks a preset in step 1, then shows a preset bar that goes back", async () => {
    const user = userEvent.setup();
    const onPresetChange = vi.fn();
    render(<TwoSteps onPresetChange={onPresetChange} />);

    const host = await screen.findByTestId("host");
    // 自定义配置固定第一行，其余按名称排
    const rows = within(host)
      .getAllByRole("button")
      .filter((button) => !button.hasAttribute("aria-pressed"));
    expect(rows[0]).toHaveTextContent("providerPreset.custom");
    expect(rows[1]).toHaveTextContent("AWS Bedrock");

    await user.click(within(host).getByText("preset.zhipu"));
    expect(onPresetChange).toHaveBeenCalledWith("zhipu");
    expect(screen.queryByTestId("host")).not.toBeInTheDocument();
    expect(screen.getByText("open.bigmodel.cn")).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "providerPreset.change" }),
    );
    expect(await screen.findByTestId("host")).toBeInTheDocument();
  });

  it("filters by category and keeps the search text when clicking around", async () => {
    const user = userEvent.setup();
    render(<TwoSteps onPresetChange={vi.fn()} />);
    const host = await screen.findByTestId("host");

    await user.click(
      within(host).getByRole("button", { name: /providerPreset.group.login/ }),
    );
    expect(within(host).getByText("GitHub Copilot")).toBeInTheDocument();
    expect(within(host).queryByText("Gamma")).not.toBeInTheDocument();
    expect(
      within(host).getByText("providerPreset.loginWith.github"),
    ).toBeInTheDocument();

    const search = within(host).getByRole("textbox", {
      name: "providerPreset.searchAriaLabel",
    });
    await user.type(search, "zzz");
    await user.click(document.body);
    expect(search).toHaveValue("zzz");
    expect(
      within(host).getByText("providerPreset.noResults"),
    ).toBeInTheDocument();
  });

  it("picks the first version of a merged row, then switches versions in the bar", async () => {
    const user = userEvent.setup();
    const onPresetChange = vi.fn();
    render(
      <TwoSteps
        onPresetChange={onPresetChange}
        presetEntries={familyEntries}
      />,
    );
    const host = await screen.findByTestId("host");

    expect(within(host).getAllByText("Kimi")).toHaveLength(1);
    expect(
      within(host).getByText("providerPreset.versionCount"),
    ).toBeInTheDocument();
    await user.click(within(host).getByRole("button", { name: "Kimi" }));
    expect(onPresetChange).toHaveBeenLastCalledWith("kimi-cn");

    const versions = screen.getByRole("group", {
      name: "providerPreset.versionLabel",
    });
    const buttons = within(versions).getAllByRole("button");
    expect(buttons.map((button) => button.textContent)).toEqual([
      "providerPreset.version.paygCn",
      "providerPreset.version.codingCn",
      "providerPreset.version.paygIntl",
    ]);
    expect(buttons[0]).toHaveAttribute("aria-pressed", "true");

    await user.click(buttons[2]);
    // 没手动改过表单：直接切，不弹确认
    expect(
      screen.queryByText("providerPreset.switchVersionTitle"),
    ).not.toBeInTheDocument();
    expect(onPresetChange).toHaveBeenLastCalledWith("kimi-intl");
    // 换版本留在第 2 步
    expect(screen.queryByTestId("host")).not.toBeInTheDocument();
    expect(
      within(
        screen.getByRole("group", { name: "providerPreset.versionLabel" }),
      ).getAllByRole("button")[2],
    ).toHaveAttribute("aria-pressed", "true");
  });

  it("asks before switching versions once the form was edited", async () => {
    const user = userEvent.setup();
    const onPresetChange = vi.fn();
    render(
      <TwoSteps
        onPresetChange={onPresetChange}
        presetEntries={familyEntries}
      />,
    );
    const host = await screen.findByTestId("host");
    await user.click(within(host).getByRole("button", { name: "Kimi" }));
    expect(onPresetChange).toHaveBeenLastCalledWith("kimi-cn");
    onPresetChange.mockClear();

    const versionButtons = () =>
      within(
        screen.getByRole("group", { name: "providerPreset.versionLabel" }),
      ).getAllByRole("button");

    await user.type(screen.getByLabelText("api-key"), "sk-typed");
    await user.click(versionButtons()[1]);
    expect(
      await screen.findByText("providerPreset.switchVersionTitle"),
    ).toBeInTheDocument();
    expect(onPresetChange).not.toHaveBeenCalled();

    // 取消：不切，Key 还在
    await user.click(screen.getByRole("button", { name: "common.cancel" }));
    await waitFor(() =>
      expect(
        screen.queryByText("providerPreset.switchVersionTitle"),
      ).not.toBeInTheDocument(),
    );
    expect(onPresetChange).not.toHaveBeenCalled();
    expect(versionButtons()[0]).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByLabelText("api-key")).toHaveValue("sk-typed");

    // 确认：切过去、表单重填；重填之后再切不用确认
    await user.click(versionButtons()[1]);
    await user.click(
      await screen.findByRole("button", {
        name: "providerPreset.switchVersionConfirm",
      }),
    );
    expect(onPresetChange).toHaveBeenLastCalledWith("kimi-coding");
    expect(screen.getByLabelText("api-key")).toHaveValue("");
    await user.click(versionButtons()[2]);
    expect(
      screen.queryByText("providerPreset.switchVersionTitle"),
    ).not.toBeInTheDocument();
    expect(onPresetChange).toHaveBeenLastCalledWith("kimi-intl");
  });

  it("selects the version a search matched", async () => {
    const user = userEvent.setup();
    const onPresetChange = vi.fn();
    render(
      <TwoSteps
        onPresetChange={onPresetChange}
        presetEntries={familyEntries}
      />,
    );
    const host = await screen.findByTestId("host");

    await user.type(
      within(host).getByRole("textbox", {
        name: "providerPreset.searchAriaLabel",
      }),
      "coding",
    );
    expect(
      within(host).getByText("providerPreset.matchedVersion"),
    ).toBeInTheDocument();
    await user.click(within(host).getByRole("button", { name: "Kimi" }));
    expect(onPresetChange).toHaveBeenLastCalledWith("kimi-coding");
  });

  it("offers the custom config when nothing matches", async () => {
    const user = userEvent.setup();
    const onPresetChange = vi.fn();
    render(<TwoSteps onPresetChange={onPresetChange} />);
    const host = await screen.findByTestId("host");

    await user.type(
      within(host).getByRole("textbox", {
        name: "providerPreset.searchAriaLabel",
      }),
      "nothing-here",
    );
    await user.click(
      within(host).getByRole("button", { name: "providerPreset.useCustom" }),
    );
    await waitFor(() => expect(onPresetChange).toHaveBeenCalledWith("custom"));
  });
});
