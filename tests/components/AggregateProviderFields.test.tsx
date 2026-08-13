import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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

function renderFields(
  routes: AggregateRoutes,
  overrides: Partial<FieldsProps> = {},
) {
  const onRoutesChange = vi.fn();
  const props: FieldsProps = {
    appId: "claude",
    enabled: true,
    onEnabledChange: vi.fn(),
    routes,
    onRoutesChange,
    providers: [targetProvider("kimi", "Kimi")],
    ...overrides,
  };
  render(<AggregateProviderFields {...props} />);
  return { onRoutesChange };
}

// 四个档位行各有一个 1M checkbox，顺序与 AGGREGATE_ROUTE_TIERS 一致
const OPUS_CHECKBOX_INDEX = 2;

describe("AggregateProviderFields enable switch", () => {
  it("renders the switch when onEnabledChange is provided", () => {
    renderFields({});

    expect(screen.getByRole("switch")).toBeInTheDocument();
  });

  it("hides the switch (always enabled) when onEnabledChange is omitted", () => {
    renderFields(
      { opus: { providerId: "kimi", model: "k3" } },
      { enabled: undefined, onEnabledChange: undefined },
    );

    expect(screen.queryByRole("switch")).toBeNull();
    // 路由 UI 仍然渲染
    expect(document.getElementById("aggregate-opus-model")).not.toBeNull();
  });
});

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

describe("AggregateProviderFields codex custom routes", () => {
  function renderCodexFields(
    customRows: Array<{ key: string; providerId: string; model: string }> = [],
  ) {
    const onRoutesChange = vi.fn();
    const onCustomRowsChange = vi.fn();
    renderFields(
      {},
      {
        appId: "codex",
        routes: {},
        onRoutesChange,
        customRows,
        onCustomRowsChange,
      },
    );
    return { onRoutesChange, onCustomRowsChange };
  }

  it("uses the codex-specific hint", () => {
    renderCodexFields();

    expect(screen.getByText(/exact request model name/i)).toBeInTheDocument();
  });

  it("renders existing rows with request/upstream model inputs", () => {
    renderCodexFields([{ key: "gpt-5.5", providerId: "kimi", model: "k2" }]);

    const requestInput = document.getElementById(
      "aggregate-custom-0-key",
    ) as HTMLInputElement;
    const upstreamInput = document.getElementById(
      "aggregate-custom-0-model",
    ) as HTMLInputElement;
    expect(requestInput.value).toBe("gpt-5.5");
    expect(upstreamInput.value).toBe("k2");
  });

  it("always offers official model suggestions for the request model input", () => {
    renderCodexFields([{ key: "", providerId: "", model: "" }]);

    // 候选非空时 ModelInputWithFetch 渲染 Input + 下拉触发按钮
    const requestInput = document.getElementById("aggregate-custom-0-key")!;
    const trigger = requestInput.parentElement?.querySelector("button");
    expect(trigger).not.toBeNull();
  });

  it("picking a suggestion from the dropdown fills the request model", async () => {
    const user = userEvent.setup();
    const { onCustomRowsChange } = renderCodexFields([
      { key: "", providerId: "kimi", model: "k2" },
    ]);

    const requestInput = document.getElementById("aggregate-custom-0-key")!;
    await user.click(
      requestInput.parentElement!.querySelector("button") as HTMLElement,
    );
    await user.click(await screen.findByText("gpt-5.5"));

    expect(onCustomRowsChange).toHaveBeenCalledWith([
      { key: "gpt-5.5", providerId: "kimi", model: "k2" },
    ]);
  });

  it("shows a fetch button for the upstream model once a provider is selected", () => {
    renderCodexFields([{ key: "gpt-5.5", providerId: "kimi", model: "" }]);

    expect(screen.getByTitle("providerForm.fetchModels")).toBeInTheDocument();
  });

  it("keeps the upstream model a plain input without a selected provider", () => {
    renderCodexFields([{ key: "gpt-5.5", providerId: "", model: "" }]);

    expect(document.getElementById("aggregate-custom-0-model")).not.toBeNull();
    expect(screen.queryByTitle("providerForm.fetchModels")).toBeNull();
  });

  it("adds an empty row and writes it back as a custom Record entry", () => {
    const { onRoutesChange, onCustomRowsChange } = renderCodexFields();

    fireEvent.click(screen.getByRole("button", { name: /add route/i }));
    const nextRows = [{ key: "", providerId: "", model: "" }];
    expect(onCustomRowsChange).toHaveBeenCalledWith(nextRows);
    expect(onRoutesChange).toHaveBeenCalledWith({
      custom: { "": { providerId: "", model: "" } },
    });
  });

  it("typing a request model name updates the row and the custom Record", () => {
    const { onRoutesChange, onCustomRowsChange } = renderCodexFields([
      { key: "", providerId: "kimi", model: "k2" },
    ]);

    fireEvent.change(document.getElementById("aggregate-custom-0-key")!, {
      target: { value: "gpt-5.5" },
    });
    const nextRows = [{ key: "gpt-5.5", providerId: "kimi", model: "k2" }];
    expect(onCustomRowsChange).toHaveBeenCalledWith(nextRows);
    expect(onRoutesChange).toHaveBeenCalledWith({
      custom: { "gpt-5.5": { providerId: "kimi", model: "k2" } },
    });
  });

  it("deletes a row via the trash button", () => {
    const { onCustomRowsChange } = renderCodexFields([
      { key: "gpt-5.5", providerId: "kimi", model: "k2" },
    ]);

    fireEvent.click(screen.getByRole("button", { name: /delete/i }));
    expect(onCustomRowsChange).toHaveBeenCalledWith([]);
  });
});
