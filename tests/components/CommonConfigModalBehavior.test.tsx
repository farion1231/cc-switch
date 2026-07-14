import type { ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import CodexConfigEditor from "@/components/providers/forms/CodexConfigEditor";
import GeminiConfigEditor from "@/components/providers/forms/GeminiConfigEditor";
import { isCodexGoalModeEnabled } from "@/utils/providerConfigUtils";

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    title,
    onClose,
    children,
    footer,
  }: {
    isOpen: boolean;
    title: string;
    onClose: () => void;
    children: ReactNode;
    footer?: ReactNode;
  }) =>
    isOpen ? (
      <div data-testid="common-config-panel">
        <button type="button" onClick={onClose}>
          panel-close
        </button>
        <h2>{title}</h2>
        <div>{children}</div>
        <div>{footer}</div>
      </div>
    ) : null,
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

describe("Common config modals", () => {
  it("keeps the Codex common config modal closed after user closes it with an error present", async () => {
    render(
      <CodexConfigEditor
        authValue="{}"
        configValue=""
        onAuthChange={() => {}}
        onConfigChange={() => {}}
        useCommonConfig={false}
        onCommonConfigToggle={() => {}}
        commonConfigSnippet={`base_url = "https://example.com"`}
        onCommonConfigSnippetChange={() => false}
        onCommonConfigErrorClear={() => {}}
        commonConfigError="Invalid TOML"
        authError=""
        configError=""
      />,
    );

    expect(screen.queryByTestId("common-config-panel")).not.toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", {
        name: /codexConfig.editCommonConfig|编辑通用配置/,
      }),
    );

    expect(screen.getByTestId("common-config-panel")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));

    await waitFor(() =>
      expect(
        screen.queryByTestId("common-config-panel"),
      ).not.toBeInTheDocument(),
    );
  });

  it("toggles Codex Goal mode in config.toml from the provider editor", () => {
    const onConfigChange = vi.fn();
    const configValue = [
      'model_provider = "custom"',
      'model = "gpt-5.4"',
      "",
      "[model_providers.custom]",
      'name = "custom"',
      "",
    ].join("\n");

    render(
      <CodexConfigEditor
        authValue="{}"
        configValue={configValue}
        onAuthChange={() => {}}
        onConfigChange={onConfigChange}
        useCommonConfig={false}
        onCommonConfigToggle={() => {}}
        commonConfigSnippet=""
        onCommonConfigSnippetChange={() => true}
        onCommonConfigErrorClear={() => {}}
        commonConfigError=""
        authError=""
        configError=""
      />,
    );

    const goalToggle = screen.getByRole("checkbox", {
      name: "codexConfig.enableGoalMode",
    });

    expect(goalToggle).not.toBeChecked();

    fireEvent.click(goalToggle);

    const enabledConfig = onConfigChange.mock.lastCall?.[0] ?? "";
    expect(isCodexGoalModeEnabled(enabledConfig)).toBe(true);

    fireEvent.click(goalToggle);

    const disabledConfig = onConfigChange.mock.lastCall?.[0] ?? "";
    expect(isCodexGoalModeEnabled(disabledConfig)).toBe(false);
    expect(disabledConfig).not.toContain("goals = true");
  });

  it("separates Grok global config from Codex-only controls", () => {
    render(
      <CodexConfigEditor
        appId="grok"
        showCodexFeatures={false}
        authValue="{}"
        configValue={'[model.ccswitch]\nmodel = "grok-4.5"'}
        onAuthChange={() => {}}
        onConfigChange={() => {}}
        useCommonConfig={false}
        onCommonConfigToggle={() => {}}
        commonConfigSnippet=""
        onCommonConfigSnippetChange={() => true}
        onCommonConfigErrorClear={() => {}}
        commonConfigError=""
        authError=""
        configError=""
      />,
    );

    expect(
      screen.queryByRole("button", {
        name: /codexConfig.editCommonConfig|编辑通用配置/,
      }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: /grokConfig.editGlobalConfig|编辑 Grok 全局配置/,
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: /grokConfig.addToGlobalConfig|加入全局配置/,
      }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("checkbox", {
        name: "codexConfig.enableGoalMode",
      }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  });

  it("keeps the Gemini common config modal closed after user closes it with an error present", async () => {
    render(
      <GeminiConfigEditor
        envValue="{}"
        configValue="{}"
        onEnvChange={() => {}}
        onConfigChange={() => {}}
        useCommonConfig={false}
        onCommonConfigToggle={() => {}}
        commonConfigSnippet={`{"GEMINI_MODEL":"gemini-2.5-pro"}`}
        onCommonConfigSnippetChange={() => false}
        onCommonConfigErrorClear={() => {}}
        commonConfigError="Invalid JSON"
        envError=""
        configError=""
      />,
    );

    expect(screen.queryByTestId("common-config-panel")).not.toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", {
        name: /geminiConfig.editCommonConfig|编辑通用配置/,
      }),
    );

    expect(screen.getByTestId("common-config-panel")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));

    await waitFor(() =>
      expect(
        screen.queryByTestId("common-config-panel"),
      ).not.toBeInTheDocument(),
    );
  });
});
