import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useForm } from "react-hook-form";
import { Form } from "@/components/ui/form";
import { ProviderPresetSelector } from "@/components/providers/forms/ProviderPresetSelector";

describe("ProviderPresetSelector", () => {
  afterEach(() => {
    window.localStorage.removeItem("provider-preset-sort-mode");
  });

  const renderSelector = () => {
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
                  name: "Zoo",
                  websiteUrl: "https://first.example.com",
                  settingsConfig: {},
                  category: "third_party",
                },
              },
              {
                id: "preset-1",
                preset: {
                  name: "Alpha",
                  websiteUrl: "https://second.example.com",
                  settingsConfig: {},
                  category: "official",
                },
              },
              {
                id: "preset-2",
                preset: {
                  name: "Moon",
                  websiteUrl: "https://third.example.com",
                  settingsConfig: {},
                  category: "aggregator",
                },
              },
              {
                id: "preset-3",
                preset: {
                  name: "Beta",
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

    return render(<Wrapper />);
  };

  const presetButtonTexts = () =>
    within(screen.getByTestId("provider-preset-options"))
      .getAllByRole("button")
      .map((button) => button.textContent);

  it("Default 模式按传入的预设数组顺序渲染，不按分类重新排序", () => {
    window.localStorage.removeItem("provider-preset-sort-mode");
    renderSelector();

    expect(presetButtonTexts()).toEqual([
      "providerPreset.custom",
      "Zoo",
      "Alpha",
      "Moon",
      "Beta",
    ]);
  });

  it("按字母模式基于显示名称排序，并保持 Custom Configuration 固定在最前", async () => {
    window.localStorage.removeItem("provider-preset-sort-mode");
    renderSelector();

    await userEvent.click(
      screen.getByRole("tab", { name: "providerPreset.sortAlphabetical" }),
    );

    expect(presetButtonTexts()).toEqual([
      "providerPreset.custom",
      "Alpha",
      "Beta",
      "Moon",
      "Zoo",
    ]);
  });

  it("会从 localStorage 恢复上次选择的排序模式", () => {
    window.localStorage.setItem("provider-preset-sort-mode", "alphabetical");
    renderSelector();

    expect(
      screen.getByRole("tab", { name: "providerPreset.sortAlphabetical" }),
    ).toHaveAttribute("data-state", "active");
    expect(presetButtonTexts()).toEqual([
      "providerPreset.custom",
      "Alpha",
      "Beta",
      "Moon",
      "Zoo",
    ]);
  });
});
