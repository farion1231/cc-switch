import type { ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import CodexConfigEditor from "@/components/providers/forms/CodexConfigEditor";
import GeminiConfigEditor from "@/components/providers/forms/GeminiConfigEditor";

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
      screen.getByRole("button", { name: /codexConfig.editCommonConfig|编辑通用配置/ }),
    );

    expect(screen.getByTestId("common-config-panel")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));

    await waitFor(() =>
      expect(
        screen.queryByTestId("common-config-panel"),
      ).not.toBeInTheDocument(),
    );
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
