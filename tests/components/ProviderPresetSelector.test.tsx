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
  type PresetEntry,
} from "@/components/providers/forms/ProviderPresetSelector";
import {
  PresetStepContext,
  type PresetStepState,
} from "@/components/providers/forms/presetStep";
import {
  domainBody,
  presetGroup,
} from "@/components/providers/forms/presetGroups";

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

function TwoSteps({
  onPresetChange,
}: {
  onPresetChange: (id: string) => void;
}) {
  const [step, setStep] = useState<"pick" | "form">("pick");
  const [host, setHost] = useState<HTMLDivElement | null>(null);
  const [selected, setSelected] = useState<string | null>("custom");
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
      <ProviderPresetSelector
        selectedPresetId={selected}
        presetEntries={entries}
        onPresetChange={(id) => {
          setSelected(id);
          onPresetChange(id);
        }}
      />
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
