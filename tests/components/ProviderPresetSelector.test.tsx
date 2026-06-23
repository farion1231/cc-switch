import { render, screen, waitFor } from "@testing-library/react";
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
    primePartner?: boolean;
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

  it("支持 A-Z 排序、original 模式将官方分类置顶，并且 getVisible 先 filter 再 sort", () => {
    const originalMode: PresetSortMode = "original";
    const nameAscMode: PresetSortMode = "nameAsc";

    const original = sortPresetEntries(presetEntries, originalMode, t);
    expect(original).not.toBe(presetEntries);
    // original 模式置顶官方分类（alpha），其余保持传入顺序。
    expect(getIds(original)).toEqual(["alpha", "gamma", "beta", "delta"]);

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

  it("original 模式按「官方 → 尊享伙伴 → 其余」三段排序，各组内部保序且双重身份不重复", () => {
    // 故意打乱传入顺序，验证：
    // - official 组置顶（officialOnly、officialPrime 按出现顺序）；
    // - 非官方且 primePartner 的预设居中（primeOnly）；
    // - 其余保持传入顺序（restFirst、restLast）；
    // - 既是 official 又是 primePartner 的预设只归入官方组、不在 prime 组重复。
    const mixed: TestPresetEntry[] = [
      {
        id: "restFirst",
        preset: {
          name: "Rest First",
          websiteUrl: "https://rest-first.example.com",
          settingsConfig: {},
          category: "third_party",
        },
      },
      {
        id: "primeOnly",
        preset: {
          name: "Prime Only",
          websiteUrl: "https://prime-only.example.com",
          settingsConfig: {},
          category: "cn_official",
          primePartner: true,
        },
      },
      {
        id: "officialOnly",
        preset: {
          name: "Official Only",
          websiteUrl: "https://official-only.example.com",
          settingsConfig: {},
          category: "official",
        },
      },
      {
        id: "officialPrime",
        preset: {
          name: "Official Prime",
          websiteUrl: "https://official-prime.example.com",
          settingsConfig: {},
          category: "official",
          primePartner: true,
        },
      },
      {
        id: "restLast",
        preset: {
          name: "Rest Last",
          websiteUrl: "https://rest-last.example.com",
          settingsConfig: {},
          category: "aggregator",
        },
      },
    ];

    expect(getIds(sortPresetEntries(mixed, "original", t))).toEqual([
      "officialOnly",
      "officialPrime",
      "primeOnly",
      "restFirst",
      "restLast",
    ]);
  });
});

describe("ProviderPresetSelector", () => {
  it("默认（original 模式）将官方分类置顶，其余保持传入顺序", () => {
    renderSelector();

    expect(getPresetButtonTexts()).toEqual([
      "providerPreset.custom",
      "preset.alpha",
      "preset.gamma",
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
      "preset.alpha",
      "preset.gamma",
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

  it("所有预设按钮填满网格列宽(w-full)实现等宽对齐", () => {
    renderSelector();

    const presetButtons = screen.getAllByRole("button");
    const fullWidthButtons = presetButtons.filter((btn) =>
      btn.className.includes("w-full"),
    );

    // 至少包含 custom + 4 个预设 = 5 个等宽按钮(搜索/排序按钮为 size-8 不计入)
    expect(fullWidthButtons.length).toBeGreaterThanOrEqual(5);
  });

  it("preset.icon 存在时按钮内渲染图标元素(img/svg)", () => {
    const entriesWithIcon = [
      {
        id: "with-icon",
        preset: {
          name: "With Icon",
          websiteUrl: "https://icon.example.com",
          settingsConfig: {},
          category: "official" as ProviderCategory,
          icon: "claude-api",
          iconColor: "#D4915D",
        },
      },
    ];

    renderSelector({ entries: entriesWithIcon });

    const button = screen.getByRole("button", { name: /with icon/i });
    const icon = button.querySelector('[data-testid="provider-icon"]');
    expect(icon).not.toBeNull();
    expect(icon?.getAttribute("data-icon")).toBe("claude-api");
    expect(icon?.getAttribute("data-color")).toBe("#D4915D");
  });

  it("preset 无 icon 且无 theme.icon 时,按钮内仍渲染占位元素保持文字对齐", () => {
    const entriesWithoutIcon = [
      {
        id: "no-icon",
        preset: {
          name: "No Icon",
          websiteUrl: "https://noicon.example.com",
          settingsConfig: {},
          category: "official" as ProviderCategory,
        },
      },
    ];

    renderSelector({ entries: entriesWithoutIcon });

    const button = screen.getByRole("button", { name: /no icon/i });
    // 占位 span(16x16)应该存在,保证文字位置与有图标的按钮对齐
    const placeholder = button.querySelector("span[aria-hidden]");
    expect(placeholder).not.toBeNull();
  });

  it("custom 按钮同样渲染占位元素,文字与带图标的预设按钮对齐", () => {
    renderSelector();

    const customButton = screen.getByRole("button", {
      name: "providerPreset.custom",
    });
    const placeholder = customButton.querySelector("span[aria-hidden]");
    expect(placeholder).not.toBeNull();
  });

  it("点击放大镜 inline 切换搜索输入框可见性,ESC 收起并清空", async () => {
    const user = userEvent.setup();
    renderSelector();

    // 初始没有搜索输入框
    expect(
      screen.queryByRole("textbox", {
        name: /providerPreset\.(searchInput|searchPlaceholder)|搜索预设|search/i,
      }),
    ).not.toBeInTheDocument();

    // 点击放大镜展开输入框
    await user.click(getSearchButton());
    const input = getSearchInput();
    expect(input).toBeInTheDocument();

    // 输入关键字过滤
    await user.type(input, "gateway");
    expect(
      screen.getByRole("button", { name: "Beta Gateway" }),
    ).toBeInTheDocument();

    // ESC 收起输入框并清空
    await user.keyboard("{Escape}");
    expect(
      screen.queryByRole("textbox", {
        name: /providerPreset\.(searchInput|searchPlaceholder)|搜索预设|search/i,
      }),
    ).not.toBeInTheDocument();
    // 收起后所有预设恢复显示
    expect(
      screen.getByRole("button", { name: "preset.gamma" }),
    ).toBeInTheDocument();
  });

  it("按 Ctrl+F 快捷键打开搜索输入框", async () => {
    const user = userEvent.setup();
    renderSelector();

    // 初始没有搜索输入框
    expect(
      screen.queryByRole("textbox", {
        name: /providerPreset\.(searchInput|searchPlaceholder)|搜索预设|search/i,
      }),
    ).not.toBeInTheDocument();

    // 按 Ctrl+F 展开输入框
    await user.keyboard("{Control>}f{/Control}");
    expect(getSearchInput()).toBeInTheDocument();
  });

  it("搜索后点击预设按钮可选中预设且不清空搜索关键词", async () => {
    const user = userEvent.setup();
    const onPresetChange = vi.fn();
    renderSelector({ onPresetChange });

    await user.click(getSearchButton());
    await user.type(getSearchInput(), "gateway");

    await user.click(screen.getByRole("button", { name: "Beta Gateway" }));

    expect(onPresetChange).toHaveBeenCalledWith("beta");
    // 搜索框仍展开、关键词保留
    expect(getSearchInput()).toBeInTheDocument();
    expect(getSearchInput()).toHaveValue("gateway");
  });

  it("搜索已打开、焦点在别处时再次 Ctrl+F 把焦点移回搜索框且保留关键词", async () => {
    const user = userEvent.setup();
    renderSelector();

    await user.click(getSearchButton());
    await user.type(getSearchInput(), "gateway");

    // 选中 preset 后焦点离开搜索框（搜索框仍展开、关键词保留）
    await user.click(screen.getByRole("button", { name: "Beta Gateway" }));
    expect(getSearchInput()).not.toHaveFocus();

    // 再次 Ctrl+F：setSearchOpen(true) 同值不重渲染、autoFocus 不重触发，
    // 需靠快捷键命中时的命令式聚焦把焦点移回搜索框，且不清空关键词
    await user.keyboard("{Control>}f{/Control}");
    await waitFor(() => expect(getSearchInput()).toHaveFocus());
    expect(getSearchInput()).toHaveValue("gateway");
  });

  it("点击组件外区域自动收起并清空", async () => {
    const user = userEvent.setup();
    const Wrapper = () => {
      const form = useForm();
      return (
        <Form {...form}>
          <ProviderPresetSelector
            selectedPresetId="custom"
            presetEntries={presetEntries}
            presetCategoryLabels={presetCategoryLabels}
            onPresetChange={vi.fn()}
          />
          <div data-testid="outside">Outside</div>
        </Form>
      );
    };
    render(<Wrapper />);

    await user.click(getSearchButton());
    await user.type(getSearchInput(), "gateway");
    expect(getSearchInput()).toBeInTheDocument();

    // 点击组件外的元素应收起搜索框
    await user.click(screen.getByTestId("outside"));

    expect(
      screen.queryByRole("textbox", {
        name: /providerPreset\.(searchInput|searchPlaceholder)|搜索预设|search/i,
      }),
    ).not.toBeInTheDocument();
    // 收起后清空 query,所有预设恢复显示
    expect(
      screen.getByRole("button", { name: "preset.gamma" }),
    ).toBeInTheDocument();
  });
});
