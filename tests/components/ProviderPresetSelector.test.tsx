import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { useForm } from "react-hook-form";
import { Form } from "@/components/ui/form";
import { ProviderPresetSelector } from "@/components/providers/forms/ProviderPresetSelector";

describe("ProviderPresetSelector", () => {
  it("按传入的预设数组顺序渲染，不按分类重新排序", () => {
    const Wrapper = () => {
      const form = useForm();

      return (
        <Form {...form}>
          <ProviderPresetSelector
            selectedPresetId="custom"
            presetEntries={[
              {
                id: "preset-0",
                preset: {
                  name: "First",
                  websiteUrl: "https://first.example.com",
                  settingsConfig: {},
                  category: "third_party",
                },
              },
              {
                id: "preset-1",
                preset: {
                  name: "Second",
                  websiteUrl: "https://second.example.com",
                  settingsConfig: {},
                  category: "official",
                },
              },
              {
                id: "preset-2",
                preset: {
                  name: "Third",
                  websiteUrl: "https://third.example.com",
                  settingsConfig: {},
                  category: "aggregator",
                },
              },
              {
                id: "preset-3",
                preset: {
                  name: "Fourth",
                  websiteUrl: "https://fourth.example.com",
                  settingsConfig: {},
                  category: "official",
                },
              },
            ]}
            presetCategoryLabels={{
              official: "官方",
              aggregator: "聚合服务",
              third_party: "第三方",
            }}
            onPresetChange={vi.fn()}
          />
        </Form>
      );
    };

    render(<Wrapper />);

    expect(
      screen.getAllByRole("button").map((button) => button.textContent),
    ).toEqual(["providerPreset.custom", "First", "Second", "Third", "Fourth"]);
  });

  it("搜索 'max' 能匹配到 'MiniMax'（模糊匹配）", async () => {
    const user = userEvent.setup();
    const Wrapper = () => {
      const form = useForm();
      return (
        <Form {...form}>
          <ProviderPresetSelector
            selectedPresetId="custom"
            presetEntries={[
              {
                id: "minimax",
                preset: {
                  name: "MiniMax",
                  websiteUrl: "https://minimax.example.com",
                  settingsConfig: {},
                  category: "third_party",
                },
              },
              {
                id: "openai",
                preset: {
                  name: "OpenAI",
                  websiteUrl: "https://openai.example.com",
                  settingsConfig: {},
                  category: "official",
                },
              },
            ]}
            presetCategoryLabels={{}}
            onPresetChange={vi.fn()}
          />
        </Form>
      );
    };
    render(<Wrapper />);

    const searchInput = screen.getByRole("searchbox");
    await user.type(searchInput, "max");

    expect(screen.getByText("MiniMax")).toBeInTheDocument();
    expect(screen.queryByText("OpenAI")).not.toBeInTheDocument();
  });

  it("点击 Sort 按钮后，预设按首字母 A-Z 排序", async () => {
    const user = userEvent.setup();
    const Wrapper = () => {
      const form = useForm();
      return (
        <Form {...form}>
          <ProviderPresetSelector
            selectedPresetId="custom"
            presetEntries={[
              {
                id: "zhipu",
                preset: {
                  name: "Zhipu",
                  websiteUrl: "https://zhipu.example.com",
                  settingsConfig: {},
                  category: "third_party",
                },
              },
              {
                id: "minimax",
                preset: {
                  name: "MiniMax",
                  websiteUrl: "https://minimax.example.com",
                  settingsConfig: {},
                  category: "third_party",
                },
              },
              {
                id: "anthropic",
                preset: {
                  name: "Anthropic",
                  websiteUrl: "https://anthropic.example.com",
                  settingsConfig: {},
                  category: "official",
                },
              },
            ]}
            presetCategoryLabels={{}}
            onPresetChange={vi.fn()}
          />
        </Form>
      );
    };
    render(<Wrapper />);

    // 默认保持原顺序: Zhipu, MiniMax, Anthropic
    const buttons = screen.getAllByRole("button");
    expect(buttons[1].textContent).toMatch(/Zhipu|MiniMax|Anthropic/);

    // 点击 Sort 排序按钮
    const sortButton = screen.getByRole("button", { name: /sort/i });
    await user.click(sortButton);

    // 排序后: Anthropic, MiniMax, Zhipu
    const sortedButtons = screen.getAllByRole("button");
    expect(sortedButtons[1].textContent).toMatch(/Anthropic/);
    expect(sortedButtons[2].textContent).toMatch(/MiniMax/);
    expect(sortedButtons[3].textContent).toMatch(/Zhipu/);
  });

  it("'自定义'按钮始终排在第一位，不受排序影响", async () => {
    const user = userEvent.setup();
    const Wrapper = () => {
      const form = useForm();
      return (
        <Form {...form}>
          <ProviderPresetSelector
            selectedPresetId="custom"
            presetEntries={[
              {
                id: "zhipu",
                preset: {
                  name: "Zhipu",
                  websiteUrl: "https://zhipu.example.com",
                  settingsConfig: {},
                  category: "third_party",
                },
              },
              {
                id: "anthropic",
                preset: {
                  name: "Anthropic",
                  websiteUrl: "https://anthropic.example.com",
                  settingsConfig: {},
                  category: "official",
                },
              },
            ]}
            presetCategoryLabels={{}}
            onPresetChange={vi.fn()}
          />
        </Form>
      );
    };
    render(<Wrapper />);

    const sortButton = screen.getByRole("button", { name: /sort/i });
    await user.click(sortButton); // 开启 A-Z 排序

    const buttons = screen.getAllByRole("button");
    expect(buttons[0].textContent).toBe("providerPreset.custom"); // 自定义始终第一
    expect(buttons[1].textContent).toMatch(/Anthropic/); // A-Z 排序
  });

  it("多词 OR 搜索 'mini max' 能匹配到 'MiniMax'", async () => {
    const user = userEvent.setup();
    const Wrapper = () => {
      const form = useForm();
      return (
        <Form {...form}>
          <ProviderPresetSelector
            selectedPresetId="custom"
            presetEntries={[
              {
                id: "minimax",
                preset: {
                  name: "MiniMax",
                  websiteUrl: "https://minimax.example.com",
                  settingsConfig: {},
                  category: "third_party",
                },
              },
              {
                id: "openai",
                preset: {
                  name: "OpenAI",
                  websiteUrl: "https://openai.example.com",
                  settingsConfig: {},
                  category: "official",
                },
              },
            ]}
            presetCategoryLabels={{}}
            onPresetChange={vi.fn()}
          />
        </Form>
      );
    };
    render(<Wrapper />);

    const searchInput = screen.getByRole("searchbox");
    await user.type(searchInput, "mini max");

    expect(screen.getByText("MiniMax")).toBeInTheDocument();
    expect(screen.queryByText("OpenAI")).not.toBeInTheDocument();
  });

  it("大小写不敏感 'MINI' 能匹配到 'MiniMax'", async () => {
    const user = userEvent.setup();
    const Wrapper = () => {
      const form = useForm();
      return (
        <Form {...form}>
          <ProviderPresetSelector
            selectedPresetId="custom"
            presetEntries={[
              {
                id: "minimax",
                preset: {
                  name: "MiniMax",
                  websiteUrl: "https://minimax.example.com",
                  settingsConfig: {},
                  category: "third_party",
                },
              },
              {
                id: "openai",
                preset: {
                  name: "OpenAI",
                  websiteUrl: "https://openai.example.com",
                  settingsConfig: {},
                  category: "official",
                },
              },
            ]}
            presetCategoryLabels={{}}
            onPresetChange={vi.fn()}
          />
        </Form>
      );
    };
    render(<Wrapper />);

    const searchInput = screen.getByRole("searchbox");
    await user.type(searchInput, "MINI");

    expect(screen.getByText("MiniMax")).toBeInTheDocument();
    expect(screen.queryByText("OpenAI")).not.toBeInTheDocument();
  });

  it("取消排序恢复原顺序", async () => {
    const user = userEvent.setup();
    const Wrapper = () => {
      const form = useForm();
      return (
        <Form {...form}>
          <ProviderPresetSelector
            selectedPresetId="custom"
            presetEntries={[
              {
                id: "zhipu",
                preset: {
                  name: "Zhipu",
                  websiteUrl: "https://zhipu.example.com",
                  settingsConfig: {},
                  category: "third_party",
                },
              },
              {
                id: "anthropic",
                preset: {
                  name: "Anthropic",
                  websiteUrl: "https://anthropic.example.com",
                  settingsConfig: {},
                  category: "official",
                },
              },
            ]}
            presetCategoryLabels={{}}
            onPresetChange={vi.fn()}
          />
        </Form>
      );
    };
    render(<Wrapper />);

    const sortButton = screen.getByRole("button", { name: /sort/i });

    // 开启排序
    await user.click(sortButton);
    let buttons = screen.getAllByRole("button");
    expect(buttons[1].textContent).toMatch(/Anthropic/); // 排序后

    // 取消排序
    await user.click(sortButton);
    buttons = screen.getAllByRole("button");
    expect(buttons[1].textContent).toMatch(/Zhipu/); // 恢复原顺序
  });
});