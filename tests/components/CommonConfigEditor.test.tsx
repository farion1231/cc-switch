import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CommonConfigEditor } from "@/components/providers/forms/CommonConfigEditor";
import type { ProviderEditorInactiveField } from "@/lib/api/providers";

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      aria-label="settings-json-editor"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

function renderEditor(
  value: string,
  onChange = vi.fn(),
  inactiveFields: ProviderEditorInactiveField[] = [],
  showAdvisorModel = true,
) {
  render(
    <CommonConfigEditor
      value={value}
      onChange={onChange}
      inactiveFields={inactiveFields}
      showAdvisorModel={showAdvisorModel}
    />,
  );
  return onChange;
}

const hideAttributionCheckbox = () =>
  screen.getByRole("checkbox", { name: "claudeConfig.hideAttribution" });

describe("CommonConfigEditor hide attribution toggle", () => {
  it("requires sessionUrl=false to treat attribution as hidden", () => {
    renderEditor(
      JSON.stringify({ attribution: { commit: "", pr: "" } }, null, 2),
    );

    expect(hideAttributionCheckbox()).not.toBeChecked();
  });

  it("disables commit, PR, and session URL attribution", () => {
    const onChange = renderEditor("{}");

    fireEvent.click(hideAttributionCheckbox());

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(JSON.parse(onChange.mock.calls[0][0])).toEqual({
      attribution: {
        commit: "",
        pr: "",
        sessionUrl: false,
      },
    });
  });
});

describe("CommonConfigEditor inactive row fields", () => {
  const timeout: ProviderEditorInactiveField = {
    path: ["env", "API_TIMEOUT_MS"],
    value: "3000000",
  };

  it("offers row fields that never reach live and adds one on click", () => {
    const onChange = renderEditor(
      JSON.stringify({ env: { ANTHROPIC_BASE_URL: "https://a.example" } }),
      vi.fn(),
      [timeout],
    );

    fireEvent.click(
      screen.getByRole("button", { name: /env\.API_TIMEOUT_MS/ }),
    );

    expect(JSON.parse(onChange.mock.calls[0][0])).toEqual({
      env: {
        ANTHROPIC_BASE_URL: "https://a.example",
        API_TIMEOUT_MS: "3000000",
      },
    });
  });

  it("hides a field once the JSON already carries its value", () => {
    renderEditor(
      JSON.stringify({ env: { API_TIMEOUT_MS: "3000000" } }),
      vi.fn(),
      [timeout],
    );

    expect(
      screen.queryByRole("button", { name: /env\.API_TIMEOUT_MS/ }),
    ).not.toBeInTheDocument();
  });
});

describe("CommonConfigEditor auto mode server toggle", () => {
  const autoModeServerCheckbox = () =>
    screen.getByRole("checkbox", {
      name: "claudeConfig.disableAutoModeServer",
    });

  it("reads CLAUDE_CODE_AUTO_MODE_SERVER=0 as checked", () => {
    renderEditor(
      JSON.stringify({ env: { CLAUDE_CODE_AUTO_MODE_SERVER: "0" } }),
    );

    expect(autoModeServerCheckbox()).toBeChecked();
  });

  it("writes and removes the env key", () => {
    const onChange = renderEditor(
      JSON.stringify({ env: { ANTHROPIC_BASE_URL: "https://gw.example" } }),
    );

    fireEvent.click(autoModeServerCheckbox());

    expect(JSON.parse(onChange.mock.calls[0][0])).toEqual({
      env: {
        ANTHROPIC_BASE_URL: "https://gw.example",
        CLAUDE_CODE_AUTO_MODE_SERVER: "0",
      },
    });
  });

  it("drops an emptied env when unchecked", () => {
    const onChange = renderEditor(
      JSON.stringify({ env: { CLAUDE_CODE_AUTO_MODE_SERVER: "0" } }),
    );

    fireEvent.click(autoModeServerCheckbox());

    expect(JSON.parse(onChange.mock.calls[0][0])).toEqual({});
  });
});

describe("CommonConfigEditor quick toggle help", () => {
  it("gives every toggle a help button that does not toggle it", () => {
    const onChange = renderEditor("{}");

    for (const key of [
      "hideAttribution",
      "enableToolSearch",
      "disableAutoUpgrade",
      "disableArtifact",
      "disableAutoModeServer",
    ]) {
      const help = screen.getByRole("button", { name: `claudeConfig.${key}` });
      expect(help).toHaveAccessibleDescription(`claudeConfig.${key}Help`);
      fireEvent.click(help);
      expect(
        screen.getByRole("checkbox", { name: `claudeConfig.${key}` }),
      ).not.toBeChecked();
    }
    expect(onChange).not.toHaveBeenCalled();
  });
});

describe("CommonConfigEditor advisor model", () => {
  const advisorCheckbox = () =>
    screen.getByRole("checkbox", { name: "claudeConfig.advisorModel" });
  const advisorInput = () =>
    screen.getByRole("textbox", { name: "claudeConfig.advisorModelInput" });

  it("reads advisorModel as checked and shows its value", () => {
    renderEditor(JSON.stringify({ advisorModel: "sonnet" }));

    expect(advisorCheckbox()).toBeChecked();
    expect(advisorInput()).toHaveValue("sonnet");
  });

  it("enables the advisor with opus by default", () => {
    const onChange = renderEditor(JSON.stringify({ model: "opus" }));

    fireEvent.click(advisorCheckbox());

    expect(JSON.parse(onChange.mock.calls[0][0])).toEqual({
      model: "opus",
      advisorModel: "opus",
    });
  });

  it("removes advisorModel when unchecked", () => {
    const onChange = renderEditor(
      JSON.stringify({ model: "opus", advisorModel: "claude-opus-5-5" }),
    );

    fireEvent.click(advisorCheckbox());

    expect(JSON.parse(onChange.mock.calls[0][0])).toEqual({ model: "opus" });
  });

  it("writes a typed model and drops the key when cleared", () => {
    const onChange = renderEditor("{}");

    fireEvent.change(advisorInput(), {
      target: { value: " claude-opus-5-5 " },
    });
    expect(JSON.parse(onChange.mock.calls[0][0])).toEqual({
      advisorModel: "claude-opus-5-5",
    });

    fireEvent.change(advisorInput(), { target: { value: "" } });
    expect(JSON.parse(onChange.mock.calls[1][0])).toEqual({});
  });
});

describe("CommonConfigEditor advisor model visibility", () => {
  it("hides the advisor option for non-official providers", () => {
    renderEditor(JSON.stringify({ advisorModel: "opus" }), vi.fn(), [], false);

    expect(
      screen.queryByRole("checkbox", { name: "claudeConfig.advisorModel" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("textbox", { name: "claudeConfig.advisorModelInput" }),
    ).not.toBeInTheDocument();
  });
});

describe("CommonConfigEditor advisor model presets", () => {
  it("lists every alias even when one is already filled in", () => {
    const onChange = renderEditor(JSON.stringify({ advisorModel: "opus" }));

    fireEvent.keyDown(
      screen.getByRole("button", { name: "claudeConfig.advisorModelPresets" }),
      { key: "Enter" },
    );

    const items = screen.getAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual([
      "opus",
      "sonnet",
      "fable",
    ]);

    fireEvent.click(screen.getByRole("menuitem", { name: "sonnet" }));
    expect(JSON.parse(onChange.mock.calls[0][0])).toEqual({
      advisorModel: "sonnet",
    });
  });
});
