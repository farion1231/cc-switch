import { fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";
import { AggregateProviderFields } from "@/components/providers/forms/AggregateProviderFields";
import type { AggregateRoutes, Provider } from "@/types";

type FieldsProps = ComponentProps<typeof AggregateProviderFields>;

function targetProvider(id: string, name: string): Provider {
  return {
    id,
    name,
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.example.com",
        ANTHROPIC_API_KEY: "sk-test",
      },
    },
  };
}

function renderFields(routes: AggregateRoutes) {
  const onRoutesChange = vi.fn();
  const props: FieldsProps = {
    enabled: true,
    onEnabledChange: vi.fn(),
    routes,
    onRoutesChange,
    providers: [targetProvider("kimi", "Kimi")],
  };
  render(<AggregateProviderFields {...props} />);
  return { onRoutesChange };
}

// 四个档位行各有一个 1M checkbox，顺序与 AGGREGATE_ROUTE_TIERS 一致
const OPUS_CHECKBOX_INDEX = 2;

describe("AggregateProviderFields 1M marker", () => {
  it("reflects the [1M] marker: checkbox checked, input shows the base id", () => {
    renderFields({ opus: { providerId: "kimi", model: "k3[1M]" } });

    const input = document.getElementById(
      "aggregate-opus-model",
    ) as HTMLInputElement;
    expect(input.value).toBe("k3");

    const checkboxes = screen.getAllByRole("checkbox");
    expect(checkboxes[OPUS_CHECKBOX_INDEX]).toHaveAttribute(
      "data-state",
      "checked",
    );
    expect(checkboxes[0]).toHaveAttribute("data-state", "unchecked");
  });

  it("toggling the checkbox adds and removes the [1M] marker", () => {
    const { onRoutesChange } = renderFields({
      opus: { providerId: "kimi", model: "k3[1M]" },
    });

    fireEvent.click(screen.getAllByRole("checkbox")[OPUS_CHECKBOX_INDEX]);
    expect(onRoutesChange).toHaveBeenCalledWith({
      opus: { providerId: "kimi", model: "k3" },
    });
  });

  it("checking the box on an unmarked model appends [1M]", () => {
    const { onRoutesChange } = renderFields({
      opus: { providerId: "kimi", model: "k3" },
    });

    fireEvent.click(screen.getAllByRole("checkbox")[OPUS_CHECKBOX_INDEX]);
    expect(onRoutesChange).toHaveBeenCalledWith({
      opus: { providerId: "kimi", model: "k3[1M]" },
    });
  });

  it("typing in the model input preserves the current marker state", () => {
    const { onRoutesChange } = renderFields({
      opus: { providerId: "kimi", model: "k3[1M]" },
    });

    fireEvent.change(document.getElementById("aggregate-opus-model")!, {
      target: { value: "k3.5" },
    });
    expect(onRoutesChange).toHaveBeenCalledWith({
      opus: { providerId: "kimi", model: "k3.5[1M]" },
    });
  });

  it("ignores checkbox toggles while the model is empty", () => {
    const { onRoutesChange } = renderFields({
      opus: { providerId: "kimi", model: "" },
    });

    fireEvent.click(screen.getAllByRole("checkbox")[OPUS_CHECKBOX_INDEX]);
    expect(onRoutesChange).not.toHaveBeenCalled();
  });
});
