import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { TFunction } from "i18next";
import { useForm } from "react-hook-form";
import { Form } from "@/components/ui/form";
import type { ProviderCategory } from "@/types";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import {
  ProviderPresetSelector,
  filterPresetEntries,
  getPresetDisplayName,
  getPresetSearchText,
  getVisiblePresetEntries,
  sortPresetEntries,
  type PresetSortMode,
} from "@/components/providers/forms/ProviderPresetSelector";

// Render the Popover inline so the combobox list is always visible in jsdom,
// avoiding Radix portal/pointer-capture quirks.
vi.mock("@/components/ui/popover", () => ({
  Popover: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="popover">{children}</div>
  ),
  PopoverTrigger: ({ children }: { children: React.ReactNode }) => (
    <>{children}</>
  ),
  PopoverContent: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="popover-content">{children}</div>
  ),
  PopoverAnchor: ({ children }: { children: React.ReactNode }) => (
    <>{children}</>
  ),
}));

// Mock ProviderIcon 以避免依赖图标库的实际内容
vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: ({
    icon,
    name,
    color,
    size,
  }: {
    icon?: string;
    name: string;
    color?: string;
    size?: number;
  }) => (
    <span
      data-testid="provider-icon"
      data-icon={icon}
      data-name={name}
      data-color={color}
      data-size={size}
    />
  ),
}));

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
    icon?: string;
    iconColor?: string;
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
  selectedPresetId = "custom",
  onPresetChange = vi.fn(),
  onUniversalPresetSelect,
  onManageUniversalProviders,
}: {
  entries?: TestPresetEntry[];
  selectedPresetId?: string | null;
  onPresetChange?: (value: string) => void;
  onUniversalPresetSelect?: Parameters<
    typeof ProviderPresetSelector
  >[0]["onUniversalPresetSelect"];
  onManageUniversalProviders?: () => void;
} = {}) {
  const Wrapper = () => {
    const form = useForm();

    return (
      <Form {...form}>
        <ProviderPresetSelector
          selectedPresetId={selectedPresetId}
          presetEntries={entries}
          presetCategoryLabels={presetCategoryLabels}
          onPresetChange={onPresetChange}
          onUniversalPresetSelect={onUniversalPresetSelect}
          onManageUniversalProviders={onManageUniversalProviders}
        />
      </Form>
    );
  };

  return render(<Wrapper />);
}

// In the rendered component the real react-i18next `t` is used (missing keys
// fall back to the key itself), so preset display names resolve to:
//   gamma -> "preset.gamma", alpha -> "preset.alpha",
//   beta  -> "Beta Gateway",  delta -> "Delta Mirror".
// Alphabetical order is therefore: Beta, Delta, preset.alpha, preset.gamma.
function getOptionTexts() {
  return screen
    .getAllByRole("option")
    .map((option) => option.textContent?.trim() ?? "");
}

function getSearchInput() {
  return screen.getByPlaceholderText(/search presets/i);
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

  it("仅拼接显示名与原始名称、统一 lower-case，不含 URL 或分类 label", () => {
    const searchText = getPresetSearchText(presetEntries[1], t);

    expect(searchText).toContain("alpha 本地名");
    expect(searchText).toContain("alpha raw");
    expect(searchText).not.toContain("example.com");
    expect(searchText).not.toContain("官方");
    expect(searchText).toBe(searchText.toLowerCase());
  });

  it("空 query 返回原数组，非空 query 大小写不敏感匹配", () => {
    expect(filterPresetEntries(presetEntries, "   ", t)).toBe(presetEntries);
    expect(
      getIds(filterPresetEntries(presetEntries, "ALPHA 本地名", t)),
    ).toEqual(["alpha"]);
  });

  it("不再通过 URL 或分类 label 搜索（仅匹配名称）", () => {
    expect(
      getIds(filterPresetEntries(presetEntries, "cn-gateway.example.com", t)),
    ).toEqual([]);
    expect(getIds(filterPresetEntries(presetEntries, "聚合", t))).toEqual([]);
  });

  it("finds Codex cross-protocol presets by provider capability names", () => {
    const codexEntries = codexProviderPresets.map((preset, index) => ({
      id: `codex-${index}`,
      preset,
    }));

    expect(
      filterPresetEntries(codexEntries, "anthropic", t).map((entry) =>
        "apiFormat" in entry.preset ? entry.preset.apiFormat : null,
      ),
    ).toContain("anthropic");
    expect(
      filterPresetEntries(codexEntries, "gemini native", t).map((entry) =>
        "apiFormat" in entry.preset ? entry.preset.apiFormat : null,
      ),
    ).toContain("gemini_native");
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
          t,
        }),
      ),
    ).toEqual(["alpha", "beta", "delta", "gamma"]);
  });
});

describe("ProviderPresetSelector dropdown", () => {
  it("renders a combobox trigger for the preset list", () => {
    renderSelector();
    expect(
      screen.getByRole("combobox", { name: "providerPreset.label" }),
    ).toBeInTheDocument();
  });

  it("lists Custom first, then presets in alphabetical order", () => {
    renderSelector();

    expect(getOptionTexts()).toEqual([
      "providerPreset.custom",
      "Beta Gateway",
      "Delta Mirror",
      "preset.alpha",
      "preset.gamma",
    ]);
  });

  it("filters presets by the search box while keeping Custom", async () => {
    const user = userEvent.setup();
    renderSelector();

    await user.type(getSearchInput(), "gateway");

    expect(getOptionTexts()).toEqual(["providerPreset.custom", "Beta Gateway"]);
  });

  it("filters unified provider presets with the same search box", async () => {
    const user = userEvent.setup();
    const onUniversalPresetSelect = vi.fn();
    renderSelector({ onUniversalPresetSelect });

    await user.type(getSearchInput(), "newapi");

    expect(screen.getByRole("option", { name: /newapi/i })).toBeInTheDocument();
    expect(
      screen.queryByRole("option", { name: /自定义网关/i }),
    ).not.toBeInTheDocument();
  });

  it("hides unified provider actions while a non-matching search is active", async () => {
    const user = userEvent.setup();
    renderSelector({
      onUniversalPresetSelect: vi.fn(),
      onManageUniversalProviders: vi.fn(),
    });

    await user.type(getSearchInput(), "zzz-no-match");

    expect(getOptionTexts()).toEqual(["providerPreset.custom"]);
    expect(
      screen.queryByRole("option", { name: /管理统一供应商/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText(
        /providerPreset\.noSearchResults|no matching presets|没有匹配|無結果/i,
      ),
    ).toBeInTheDocument();
  });

  it("shows an empty hint when no preset matches, Custom still present", async () => {
    const user = userEvent.setup();
    renderSelector();

    await user.type(getSearchInput(), "zzz-no-match");

    expect(getOptionTexts()).toEqual(["providerPreset.custom"]);
    expect(
      screen.getByText(
        /providerPreset\.noSearchResults|no matching presets|没有匹配|無結果/i,
      ),
    ).toBeInTheDocument();
  });

  it("selecting a preset calls onPresetChange with its id", async () => {
    const user = userEvent.setup();
    const onPresetChange = vi.fn();
    renderSelector({ onPresetChange });

    await user.click(screen.getByRole("option", { name: /beta gateway/i }));

    expect(onPresetChange).toHaveBeenCalledWith("beta");
  });

  it("selecting Custom calls onPresetChange with 'custom'", async () => {
    const user = userEvent.setup();
    const onPresetChange = vi.fn();
    renderSelector({ onPresetChange });

    await user.click(
      screen.getByRole("option", { name: /providerPreset\.custom/i }),
    );

    expect(onPresetChange).toHaveBeenCalledWith("custom");
  });

  it("renders the preset icon inside its option when provided", () => {
    const entriesWithIcon: TestPresetEntry[] = [
      {
        id: "with-icon",
        preset: {
          name: "With Icon",
          websiteUrl: "https://icon.example.com",
          settingsConfig: {},
          category: "official",
          icon: "claude-api",
          iconColor: "#D4915D",
        },
      },
    ];

    renderSelector({ entries: entriesWithIcon });

    const option = screen.getByRole("option", { name: /with icon/i });
    const icon = option.querySelector('[data-testid="provider-icon"]');
    expect(icon).not.toBeNull();
    expect(icon?.getAttribute("data-icon")).toBe("claude-api");
    expect(icon?.getAttribute("data-color")).toBe("#D4915D");
  });
});
