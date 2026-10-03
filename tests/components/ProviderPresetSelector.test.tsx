import { useState } from "react";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
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
  presetVersionLayout,
  sectionPresetRows,
} from "@/components/providers/forms/presetGroups";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import {
  PRESET_FAMILIES,
  PRESET_PLAN_KEYS,
  PRESET_REGION_KEYS,
} from "@/config/presetFamilies";
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

// 同一家的四个版本：套餐 × 地区成完整网格（文件顺序故意打乱；显示时按 planOrder 排）
const kimiEntries: PresetEntry[] = [
  {
    id: "kimi-cn",
    preset: preset("Kimi", "cn_official", "https://platform.kimi.com", {
      family: "kimi",
      planKey: "payg",
      regionKey: "cn",
    }),
  },
  {
    id: "kimi-coding",
    preset: preset("Kimi For Coding", "cn_official", "https://www.kimi.com", {
      family: "kimi",
      planKey: "coding",
      regionKey: "cn",
    }),
  },
  {
    id: "kimi-intl",
    preset: preset("Kimi Global", "cn_official", "https://platform.kimi.ai", {
      family: "kimi",
      planKey: "payg",
      regionKey: "intl",
    }),
  },
  {
    id: "kimi-coding-intl",
    preset: preset(
      "Kimi For Coding Global",
      "cn_official",
      "https://www.kimi.com",
      { family: "kimi", planKey: "coding", regionKey: "intl" },
    ),
  },
];
const familyEntries: PresetEntry[] = [...entries, ...kimiEntries];

// 只在一个维度上变化的一家（智谱：国内 / 海外）
const zhipuFamily: PresetEntry[] = [
  {
    id: "glm-cn",
    preset: preset("GLM", "cn_official", "https://open.bigmodel.cn", {
      family: "zhipu",
      regionKey: "cn",
    }),
  },
  {
    id: "glm-intl",
    preset: preset("GLM en", "cn_official", "https://z.ai", {
      family: "zhipu",
      regionKey: "intl",
    }),
  },
];

// 两维都变但缺格子（腾讯在 Codex 里多一个只有国内的按量付费）
const tencentPartial: PresetEntry[] = [
  ["tp-cn", "tokenPlan", "cn"],
  ["tp-intl", "tokenPlan", "intl"],
  ["pro-cn", "enterprisePro", "cn"],
  ["pro-intl", "enterprisePro", "intl"],
  ["hunyuan", "payg", "cn"],
].map(([id, planKey, regionKey]) => ({
  id,
  preset: preset(`Tencent ${id}`, "cn_official", "https://cloud.tencent.com", {
    family: "tencent",
    planKey,
    regionKey,
  }),
}));

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
    // 先按套餐（planOrder），同一套餐里先国内后海外
    expect(kimi?.versions.map((entry) => entry.id)).toEqual([
      "kimi-cn",
      "kimi-intl",
      "kimi-coding",
      "kimi-coding-intl",
    ]);
  });

  it("matches the whole vendor by name, or the versions a query names", () => {
    const hits = (query: string) =>
      getVisiblePresetRows(familyEntries, { query, t }).map(({ row, hits }) => [
        row.key,
        hits,
      ]);
    expect(hits("kimi")).toEqual([["family:kimi", []]]);
    expect(hits("coding")).toEqual([["family:kimi", [2, 3]]]);
    // 带点的词可以命中某个版本自己的完整域名
    expect(hits("kimi.ai")).toEqual([["family:kimi", [1]]]);
    // 别名算整家命中
    expect(hits("moonshot")).toEqual([["family:kimi", []]]);
  });

  it("merges Claude's visible presets into 71 rows", () => {
    const claudeEntries = providerPresets
      .filter((item) => !item.hidden)
      .map((item, index) => ({ id: `claude-${index}`, preset: item }));
    expect(groupPresetRows(claudeEntries)).toHaveLength(71);
  });

  it("orders plans and regions as the design does where a family says so", () => {
    const claudeEntries = providerPresets
      .filter((item) => !item.hidden)
      .map((item, index) => ({ id: `claude-${index}`, preset: item }));
    const versionsOf = (family: string) =>
      groupPresetRows(claudeEntries)
        .find((row) => row.family === family)
        ?.versions.map((entry) =>
          [entry.preset.planKey, entry.preset.regionKey]
            .filter(Boolean)
            .join("|"),
        );
    expect(versionsOf("kimi")).toEqual([
      "payg|cn",
      "payg|intl",
      "coding|cn",
      "coding|intl",
    ]);
    expect(versionsOf("tencent")).toEqual([
      "tokenPlan|cn",
      "tokenPlan|intl",
      "enterpriseLite|cn",
      "enterpriseLite|intl",
      "enterprisePro|cn",
      "enterprisePro|intl",
    ]);
    // 没写 planOrder 的保持文件顺序
    expect(versionsOf("volcengine")).toEqual([
      "agentPlan",
      "codingPlan",
      "payg",
    ]);
  });

  it("lays out versions by plan × region: segments, a grid or a dropdown", () => {
    const layoutOf = (
      presets: PresetEntry["preset"][],
      family: string,
    ): ReturnType<typeof presetVersionLayout> | undefined => {
      const row = groupPresetRows(
        presets.map((item, index) => ({ id: `p-${index}`, preset: item })),
      ).find((item) => item.family === family);
      return row ? presetVersionLayout(row.versions) : undefined;
    };
    const claude = providerPresets.filter((item) => !item.hidden);

    expect(layoutOf(claude, "kimi")).toEqual({
      kind: "grid",
      plans: ["payg", "coding"],
      regions: ["cn", "intl"],
    });
    expect(layoutOf(claude, "tencent")).toEqual({
      kind: "grid",
      plans: ["tokenPlan", "enterpriseLite", "enterprisePro"],
      regions: ["cn", "intl"],
    });
    // Codex 的腾讯多一个只有国内的混元：不成网格，用下拉
    expect(layoutOf(codexProviderPresets, "tencent")).toEqual({
      kind: "list",
    });
    expect(layoutOf(claude, "volcengine")).toEqual({
      kind: "single",
      dimension: "plan",
    });
    expect(layoutOf(claude, "zhipu")).toEqual({
      kind: "single",
      dimension: "region",
    });
    // 两维都没写：用域名区分
    expect(layoutOf(claude, "sudocode")).toEqual({
      kind: "single",
      dimension: null,
    });
    expect(presetVersionLayout(tencentPartial)).toEqual({ kind: "list" });
  });

  it("splits the all view into category sections, skipping empty ones", () => {
    const sections = (query: string) =>
      sectionPresetRows(getVisiblePresetRows(familyEntries, { query, t })).map(
        (section) => [section.group, section.items.map(({ row }) => row.key)],
      );
    expect(sections("")).toEqual([
      ["login", ["alpha", "copilot"]],
      ["vendor", ["huoshan", "family:kimi", "zhipu"]],
      ["thirdparty", ["gamma"]],
      ["cloud", ["bedrock"]],
    ]);
    expect(sections("gamma")).toEqual([["thirdparty", ["gamma"]]]);
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
      "providerPreset.versionLabel",
      "providerPreset.planLabel",
      "providerPreset.regionLabel",
      ...PRESET_PLAN_KEYS.map((key) => `providerPreset.plan.${key}`),
      ...PRESET_REGION_KEYS.map((key) => `providerPreset.region.${key}`),
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
  beforeAll(() => {
    // Radix Select 打开时会滚动到选中项，jsdom 没有 scrollIntoView
    Element.prototype.scrollIntoView ??= vi.fn();
  });

  it("picks a preset in step 1, then shows a preset bar that goes back", async () => {
    const user = userEvent.setup();
    const onPresetChange = vi.fn();
    render(<TwoSteps onPresetChange={onPresetChange} />);

    const host = await screen.findByTestId("host");
    // 自定义配置固定第一行；其余按分类分段（账号登录 → 模型厂商 → 第三方平台 → 云服务商），段内按名称
    const rows = within(host)
      .getAllByRole("button")
      .filter((button) => !button.hasAttribute("aria-pressed"));
    expect(rows[0]).toHaveTextContent("providerPreset.custom");
    expect(
      within(host)
        .getAllByRole("heading")
        .map((heading) => heading.textContent),
    ).toEqual([
      "providerPreset.group.login",
      "providerPreset.group.vendor",
      "providerPreset.group.thirdparty",
      "providerPreset.group.cloud",
    ]);
    const login = within(host).getByRole("region", {
      name: "providerPreset.group.login",
    });
    expect(
      within(login)
        .getAllByRole("button")
        .map((button) => button.getAttribute("aria-label")),
    ).toEqual(["GitHub Copilot", "preset.alpha"]);
    expect(
      within(
        within(host).getByRole("region", {
          name: "providerPreset.group.cloud",
        }),
      ).getByRole("button", { name: "AWS Bedrock" }),
    ).toBeInTheDocument();

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

  it("picks the first version of a merged row, then switches plan and region in the bar", async () => {
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

    // 完整网格：套餐一组、地区一组
    const plans = () =>
      within(
        screen.getByRole("group", { name: "providerPreset.planLabel" }),
      ).getAllByRole("button");
    const regions = () =>
      within(
        screen.getByRole("group", { name: "providerPreset.regionLabel" }),
      ).getAllByRole("button");
    expect(plans().map((button) => button.textContent)).toEqual([
      "providerPreset.plan.payg",
      "providerPreset.plan.coding",
    ]);
    expect(regions().map((button) => button.textContent)).toEqual([
      "providerPreset.region.cn",
      "providerPreset.region.intl",
    ]);
    expect(
      screen.queryByRole("group", { name: "providerPreset.versionLabel" }),
    ).not.toBeInTheDocument();
    expect(plans()[0]).toHaveAttribute("aria-pressed", "true");
    expect(regions()[0]).toHaveAttribute("aria-pressed", "true");

    await user.click(regions()[1]);
    // 没手动改过表单：直接切，不弹确认
    expect(
      screen.queryByText("providerPreset.switchVersionTitle"),
    ).not.toBeInTheDocument();
    expect(onPresetChange).toHaveBeenLastCalledWith("kimi-intl");
    // 换版本留在第 2 步
    expect(screen.queryByTestId("host")).not.toBeInTheDocument();
    expect(regions()[1]).toHaveAttribute("aria-pressed", "true");

    // 切套餐时地区保持海外
    await user.click(plans()[1]);
    expect(onPresetChange).toHaveBeenLastCalledWith("kimi-coding-intl");
    expect(plans()[1]).toHaveAttribute("aria-pressed", "true");
    expect(regions()[1]).toHaveAttribute("aria-pressed", "true");
  });

  it("uses one segmented control, labelled by the dimension that changes", async () => {
    const user = userEvent.setup();
    const onPresetChange = vi.fn();
    render(
      <TwoSteps
        onPresetChange={onPresetChange}
        presetEntries={[...entries.slice(0, 1), ...zhipuFamily]}
      />,
    );
    const host = await screen.findByTestId("host");
    await user.click(within(host).getByRole("button", { name: "Zhipu GLM" }));
    expect(onPresetChange).toHaveBeenLastCalledWith("glm-cn");

    const versions = screen.getByRole("group", {
      name: "providerPreset.versionLabel",
    });
    expect(
      within(versions)
        .getAllByRole("button")
        .map((button) => button.textContent),
    ).toEqual(["providerPreset.region.cn", "providerPreset.region.intl"]);
    expect(
      screen.queryByRole("group", { name: "providerPreset.planLabel" }),
    ).not.toBeInTheDocument();
    await user.click(within(versions).getAllByRole("button")[1]);
    expect(onPresetChange).toHaveBeenLastCalledWith("glm-intl");
  });

  it("falls back to a dropdown when plans and regions do not form a grid", async () => {
    const user = userEvent.setup();
    const onPresetChange = vi.fn();
    render(
      <TwoSteps
        onPresetChange={onPresetChange}
        presetEntries={[...entries.slice(0, 1), ...tencentPartial]}
      />,
    );
    const host = await screen.findByTestId("host");
    await user.click(
      within(host).getByRole("button", {
        name: "providerPreset.family.tencent",
      }),
    );
    expect(onPresetChange).toHaveBeenLastCalledWith("tp-cn");
    expect(
      screen.queryByRole("group", { name: "providerPreset.planLabel" }),
    ).not.toBeInTheDocument();

    const trigger = screen.getByRole("combobox", {
      name: "providerPreset.versionLabel",
    });
    expect(trigger).toHaveTextContent(
      "providerPreset.plan.tokenPlan · providerPreset.region.cn",
    );
    await user.click(trigger);
    const options = await screen.findAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual([
      "providerPreset.plan.tokenPlan · providerPreset.region.cn",
      "providerPreset.plan.tokenPlan · providerPreset.region.intl",
      "providerPreset.plan.enterprisePro · providerPreset.region.cn",
      "providerPreset.plan.enterprisePro · providerPreset.region.intl",
      "providerPreset.plan.payg · providerPreset.region.cn",
    ]);
    await user.click(options[4]);
    expect(onPresetChange).toHaveBeenLastCalledWith("hunyuan");
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

    const plans = () =>
      within(
        screen.getByRole("group", { name: "providerPreset.planLabel" }),
      ).getAllByRole("button");
    const regions = () =>
      within(
        screen.getByRole("group", { name: "providerPreset.regionLabel" }),
      ).getAllByRole("button");

    await user.type(screen.getByLabelText("api-key"), "sk-typed");
    await user.click(plans()[1]);
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
    expect(plans()[0]).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByLabelText("api-key")).toHaveValue("sk-typed");

    // 确认：切过去、表单重填；重填之后再切不用确认
    await user.click(plans()[1]);
    await user.click(
      await screen.findByRole("button", {
        name: "providerPreset.switchVersionConfirm",
      }),
    );
    expect(onPresetChange).toHaveBeenLastCalledWith("kimi-coding");
    expect(screen.getByLabelText("api-key")).toHaveValue("");
    await user.click(regions()[1]);
    expect(
      screen.queryByText("providerPreset.switchVersionTitle"),
    ).not.toBeInTheDocument();
    expect(onPresetChange).toHaveBeenLastCalledWith("kimi-coding-intl");
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
    // 命中两个编程订阅版本，选中第一个
    expect(
      within(host).getByText("providerPreset.matchedVersions"),
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
