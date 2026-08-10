import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ModelDropdown } from "@/components/providers/forms/shared/ModelDropdown";
import type { FetchedModel } from "@/lib/api/model-fetch";

const models: FetchedModel[] = [
  { id: "gateway-proxy", ownedBy: null },
  { id: "openrouter/anthropic/claude-3-5-sonnet", ownedBy: "openrouter" },
  { id: "kimi-k2-instruct", ownedBy: null },
];

function renderDropdown(onSelect = vi.fn()) {
  return {
    onSelect,
    ...render(<ModelDropdown models={models} onSelect={onSelect} />),
  };
}

function getTrigger() {
  return screen.getByRole("button", {
    name: /Choose a fetched model|providerForm\.modelPickerAriaLabel/,
  });
}

describe("ModelDropdown", () => {
  it("preserves owner groups and returns the complete model id", async () => {
    const user = userEvent.setup();
    const { onSelect } = renderDropdown();

    await user.click(getTrigger());

    expect(
      screen.getByRole("option", {
        name: "openrouter/anthropic/claude-3-5-sonnet",
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("openrouter", { exact: true })).toBeInTheDocument();
    expect(screen.getByText("Other", { exact: true })).toBeInTheDocument();

    await user.click(
      screen.getByRole("option", {
        name: "openrouter/anthropic/claude-3-5-sonnet",
      }),
    );
    expect(onSelect).toHaveBeenCalledWith(
      "openrouter/anthropic/claude-3-5-sonnet",
    );
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("matches full ids containing slashes and hyphens", async () => {
    const user = userEvent.setup();
    renderDropdown();

    await user.click(getTrigger());
    const input = screen.getByRole("combobox", {
      name: /Search model IDs|providerForm\.modelSearchAriaLabel/,
    });
    await user.type(input, "anthropic/claude-3-5");

    expect(
      screen.getByRole("option", {
        name: "openrouter/anthropic/claude-3-5-sonnet",
      }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("option", { name: "gateway-proxy" }),
    ).not.toBeInTheDocument();
  });

  it("deduplicates empty ids and resets the query after closing", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(
      <ModelDropdown
        models={[...models, { id: "", ownedBy: null }, models[0]]}
        onSelect={onSelect}
      />,
    );

    await user.click(getTrigger());
    expect(
      screen.getAllByRole("option", { name: "gateway-proxy" }),
    ).toHaveLength(1);

    const input = screen.getByRole("combobox", {
      name: /Search model IDs|providerForm\.modelSearchAriaLabel/,
    });
    await user.type(input, "kimi");
    await user.keyboard("{Escape}");

    await user.click(getTrigger());
    expect(input).toHaveValue("");
    expect(
      screen.getByRole("option", { name: "gateway-proxy" }),
    ).toBeInTheDocument();
  });
});
