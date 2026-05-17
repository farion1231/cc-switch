import { render, screen } from "@testing-library/react";
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
});
