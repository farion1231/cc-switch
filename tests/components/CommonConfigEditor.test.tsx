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
) {
  render(
    <CommonConfigEditor
      value={value}
      onChange={onChange}
      inactiveFields={inactiveFields}
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
