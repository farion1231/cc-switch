import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { TFunction } from "i18next";
import { useForm } from "react-hook-form";
import { Form } from "@/components/ui/form";
import type { ProviderCategory } from "@/types";
import {
  ProviderPresetSelector,
  filterPresetEntries,
  getPresetDisplayName,
  getPresetSearchText,
  getVisiblePresetEntries,
  sortPresetEntries,
  type PresetSortMode,
} from "@/components/providers/forms/ProviderPresetSelector";

const presetCategoryLabels = {
  official: "官方",
  cn_official: "国产官方",
  aggregator: "聚合服务",
  third_party: "第三方",
};

const translations: Record<string, string> = {
  "preset.alpha": "Alpha 本地名",
  "preset.gamma": "Gamma 本地名",
};

const t = ((key: string) => translations[key] ?? key) as TFunction;

type TestPresetEntry = {
  id: string;
  preset: {
    name: string;
    nameKey?: string;
    websiteUrl: string;
    settingsConfig: Record<string, never>;
    category: ProviderCategory;
  };
};

const presetEntries: TestPresetEntry[] = [
  {
    id: "gamma",
    preset: {
      name: "Gamma Raw",
      nameKey: "preset.gamma",
      websiteUrl: "https://gamma.example.com",
      settingsConfig: {},
      category: "aggregator",
    },
  },
  {
    id: "alpha",
    preset: {
      name: "Alpha Raw",
      nameKey: "preset.alpha",
      websiteUrl: "https://alpha.example.com/v1",
      settingsConfig: {},
      category: "official",
    },
  },
  {
    id: "beta",
    preset: {
      name: "Beta Gateway",
      websiteUrl: "https://CN-Gateway.example.com",
      settingsConfig: {},
      category: "cn_official",
    },
  },
  {
    id: "delta",
    preset: {
      name: "Delta Mirror",
      websiteUrl: "https://delta.example.com",
      settingsConfig: {},
      category: "third_party",
    },
  },
] satisfies TestPresetEntry[];

function getIds(entries: ReadonlyArray<{ id: string }>) {
  return entries.map((entry) => entry.id);
}

function renderSelector({
  entries = presetEntries,
  onPresetChange = vi.fn(),
}: {
  entries?: TestPresetEntry[];
  onPresetChange?: (value: string) => void;
} = {}) {
  const Wrapper = () => {
    const form = useForm();

    return (
      <Form {...form}>
        <ProviderPresetSelector
          selectedPresetId="custom"
          presetEntries={entries}
          presetCategoryLabels={presetCategoryLabels}
          onPresetChange={onPresetChange}
        />
      </Form>
    );
  };

  return render(<Wrapper />);
}

function getPresetButtonTexts() {
  const knownNames = new Set([
    "providerPreset.custom",
    ...presetEntries.flatMap((entry) => [
      entry.preset.name,
      entry.preset.nameKey ?? entry.preset.name,
    ]),
  ]);

  return screen
    .getAllByRole("button")
    .map((button) => button.textContent?.trim() ?? "")
    .filter((text) => knownNames.has(text));
}

function getSearchButton() {
  return screen.getByRole("button", {
    name: /providerPreset\.(search|searchAriaLabel|openSearch)|搜索|search/i,
  });
}

function getSortButton() {
  return screen.getByRole("button", {
    name: /providerPreset\.(sort|sortByName|restoreOriginalOrder)|按名称排序|恢复原顺序|sort/i,
  });
}

function getSearchInput() {
  return screen.getByRole("textbox", {
    name: /providerPreset\.(searchInput|searchPlaceholder)|搜索预设|search/i,
  });
}

describe("ProviderPresetSelector pure helpers", () => {
  it("优先使用 nameKey 翻译作为显示名，否则使用原始 name", () => {
    expect(getPresetDisplayName(presetEntries[1].preset, t)).toBe(
      "Alpha 本地名",
    );
    expect(getPresetDisplayName(presetEntries[2].preset, t)).toBe(
      "Beta Gateway",
    );
  });

  it("拼接显示名、原始名称、URL、分类 label，并统一 lower-case", () => {
    const searchText = getPresetSearchText(
      presetEntries[1],
      presetCategoryLabels,
      t,
    );

    expect(searchText).toContain("alpha 本地名");
    expect(searchText).toContain("alpha raw");
    expect(searchText).toContain("https://alpha.example.com/v1");
    expect(searchText).toContain("官方");
    expect(searchText).toBe(searchText.toLowerCase());
  });

  it("空 query 返回原数组，非空 query 大小写不敏感匹配", () => {
    expect(
      filterPresetEntries(presetEntries, "   ", presetCategoryLabels, t),
    ).toBe(presetEntries);
    expect(
      getIds(
        filterPresetEntries(
          presetEntries,
          "ALPHA 本地名",
          presetCategoryLabels,
          t,
        ),
      ),
    ).toEqual(["alpha"]);
  });

  it("支持通过 URL 和分类 label 搜索", () => {
    expect(
      getIds(
        filterPresetEntries(
          presetEntries,
          "cn-gateway.example.com",
          presetCategoryLabels,
          t,
        ),
      ),
    ).toEqual(["beta"]);
    expect(
      getIds(
        filterPresetEntries(presetEntries, "聚合", presetCategoryLabels, t),
      ),
    ).toEqual(["gamma"]);
  });

  it("支持 A-Z 排序、original 副本恢复原顺序，并且 getVisible 先 filter 再 sort", () => {
    const originalMode: PresetSortMode = "original";
    const nameAscMode: PresetSortMode = "nameAsc";

    const original = sortPresetEntries(presetEntries, originalMode, t);
    expect(original).not.toBe(presetEntries);
    expect(getIds(original)).toEqual(["gamma", "alpha", "beta", "delta"]);

    expect(getIds(sortPresetEntries(presetEntries, nameAscMode, t))).toEqual([
      "alpha",
      "beta",
      "delta",
      "gamma",
    ]);
    expect(getIds(presetEntries)).toEqual(["gamma", "alpha", "beta", "delta"]);

    expect(
      getIds(
        getVisiblePresetEntries(presetEntries, {
          query: "a",
          sortMode: nameAscMode,
          presetCategoryLabels,
          t,
        }),
      ),
    ).toEqual(["alpha", "beta", "delta", "gamma"]);
  });
});

describe("ProviderPresetSelector", () => {
  it("默认按传入的预设数组顺序渲染，不按分类或名称重新排序", () => {
    renderSelector();

    expect(getPresetButtonTexts()).toEqual([
      "providerPreset.custom",
      "preset.gamma",
      "preset.alpha",
      "Beta Gateway",
      "Delta Mirror",
    ]);
  });

  it("点击排序按钮后普通 preset A-Z，再点恢复原顺序", async () => {
    const user = userEvent.setup();
    renderSelector();

    await user.click(getSortButton());

    expect(getPresetButtonTexts()).toEqual([
      "providerPreset.custom",
      "Beta Gateway",
      "Delta Mirror",
      "preset.alpha",
      "preset.gamma",
    ]);

    await user.click(getSortButton());

    expect(getPresetButtonTexts()).toEqual([
      "providerPreset.custom",
      "preset.gamma",
      "preset.alpha",
      "Beta Gateway",
      "Delta Mirror",
    ]);
  });

  it("搜索只过滤普通 preset，自定义配置始终保留", async () => {
    const user = userEvent.setup();
    renderSelector();

    await user.click(getSearchButton());
    await user.type(getSearchInput(), "gateway");

    expect(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Beta Gateway" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "preset.gamma" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "preset.alpha" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Delta Mirror" }),
    ).not.toBeInTheDocument();
  });

  it("搜索无普通 preset 结果时保留自定义配置并显示空状态", async () => {
    const user = userEvent.setup();
    renderSelector();

    await user.click(getSearchButton());
    await user.type(getSearchInput(), "not-found");

    expect(
      screen.getByRole("button", { name: "providerPreset.custom" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "preset.gamma" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "preset.alpha" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Beta Gateway" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Delta Mirror" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText(
        /providerPreset\.(empty|noResults)|没有匹配|无结果|no matching presets/i,
      ),
    ).toBeInTheDocument();
  });
});
