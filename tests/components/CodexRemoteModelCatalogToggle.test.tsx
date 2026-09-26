import type { ReactNode } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { parse as parseToml } from "smol-toml";
import { describe, expect, it, vi } from "vitest";
import CodexConfigEditor from "@/components/providers/forms/CodexConfigEditor";

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    children,
  }: {
    isOpen: boolean;
    children: ReactNode;
  }) => (isOpen ? <div>{children}</div> : null),
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      value={value}
      onChange={(event) => onChange(event.target.value)}
      aria-label="mock-editor"
    />
  ),
}));

const TOGGLE_LABEL =
  /codexConfig\.enableRemoteModelCatalog|从供应商拉取模型列表|Fetch model list from provider/;

const relayConfig = `model_provider = "custom"
model = "gpt-5.5"

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example.com/v1/"
wire_api = "responses"
`;

const renderEditor = (
  configValue: string,
  onConfigChange: (value: string) => void = () => {},
  showRemoteModelCatalog?: boolean,
) =>
  render(
    <CodexConfigEditor
      authValue="{}"
      configValue={configValue}
      showRemoteModelCatalog={showRemoteModelCatalog}
      onAuthChange={() => {}}
      onConfigChange={onConfigChange}
      useCommonConfig={false}
      onCommonConfigToggle={() => {}}
      commonConfigSnippet=""
      onCommonConfigSnippetChange={() => false}
      onCommonConfigErrorClear={() => {}}
      commonConfigError=""
      authError=""
      configError=""
    />,
  );

describe("Codex remote model catalog toggle", () => {
  it("writes <base_url>/models and the discovery flag when checked", () => {
    const onConfigChange = vi.fn();
    renderEditor(relayConfig, onConfigChange);

    const checkbox = screen.getByRole("checkbox", { name: TOGGLE_LABEL });
    expect(checkbox).not.toBeChecked();
    expect(checkbox).toBeEnabled();

    fireEvent.click(checkbox);

    expect(onConfigChange).toHaveBeenCalledTimes(1);
    const written = parseToml(onConfigChange.mock.calls[0][0]) as Record<
      string,
      any
    >;
    expect(written.features).toEqual({ api_key_model_discovery: true });
    // Trailing slash on base_url must not produce "//models".
    expect(written.model_providers.custom.model_catalog_url).toBe(
      "https://relay.example.com/v1/models",
    );
  });

  it("shows as checked and removes both keys when unchecked", () => {
    const enabledConfig = `model_provider = "custom"

[features]
api_key_model_discovery = true

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example.com/v1"
model_catalog_url = "https://catalog.example.com/models"
`;
    const onConfigChange = vi.fn();
    renderEditor(enabledConfig, onConfigChange);

    const checkbox = screen.getByRole("checkbox", { name: TOGGLE_LABEL });
    expect(checkbox).toBeChecked();

    fireEvent.click(checkbox);

    expect(onConfigChange).toHaveBeenCalledTimes(1);
    const written = onConfigChange.mock.calls[0][0] as string;
    expect(written).not.toContain("model_catalog_url");
    expect(written).not.toContain("[features]");
    expect(
      (parseToml(written) as Record<string, any>).model_providers.custom,
    ).toEqual({ name: "Relay", base_url: "https://relay.example.com/v1" });
  });

  it("is disabled while there is no base_url to derive the URL from", () => {
    const onConfigChange = vi.fn();
    renderEditor(
      `model_provider = "custom"

[model_providers.custom]
name = "Relay"
`,
      onConfigChange,
    );

    const checkbox = screen.getByRole("checkbox", { name: TOGGLE_LABEL });
    expect(checkbox).toBeDisabled();
    fireEvent.click(checkbox);
    expect(onConfigChange).not.toHaveBeenCalled();
  });

  it("is hidden when the form opts out (official providers)", () => {
    renderEditor(relayConfig, () => {}, false);

    expect(
      screen.queryByRole("checkbox", { name: TOGGLE_LABEL }),
    ).not.toBeInTheDocument();
  });
});
