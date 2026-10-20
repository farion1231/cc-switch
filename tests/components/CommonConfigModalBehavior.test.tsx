import type { ReactNode } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
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
  it("shows no Codex common config snippet and lists fields that do not follow the provider", () => {
    render(
      <CodexConfigEditor
        authValue="{}"
        configValue=""
        onAuthChange={() => {}}
        onConfigChange={() => {}}
        authError=""
        configError=""
        inactiveFields={[
          {
            path: ["mcp_servers", "legacy"],
            value: '[mcp_servers.legacy]\ncommand = "x"\n',
          },
        ]}
      />,
    );

    expect(
      screen.queryByRole("button", {
        name: /codexConfig.editCommonConfig|编辑通用配置/,
      }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "mcp_servers.legacy" }),
    ).toBeInTheDocument();
  });

  it("shows no Gemini common config snippet and adds inactive fields to the global settings", () => {
    const onEnvChange = vi.fn();
    const onConfigChange = vi.fn();
    render(
      <GeminiConfigEditor
        envValue={"GEMINI_API_KEY=k\nDEBUG=0"}
        configValue={'{\n  "ui": {}\n}'}
        onEnvChange={onEnvChange}
        onConfigChange={onConfigChange}
        envError=""
        configError=""
        inactiveFields={[
          { path: ["env", "DEBUG"], value: "1" },
          { path: ["config", "general"], value: { vimMode: true } },
          { path: ["env", "HTTPS_PROXY"], value: "http://p" },
        ]}
      />,
    );

    expect(
      screen.queryByRole("button", {
        name: /geminiConfig.editCommonConfig|编辑通用配置/,
      }),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "+ env.DEBUG" }));
    expect(onEnvChange).toHaveBeenLastCalledWith("GEMINI_API_KEY=k\nDEBUG=1");
    fireEvent.click(screen.getByRole("button", { name: "+ env.HTTPS_PROXY" }));
    expect(onEnvChange).toHaveBeenLastCalledWith(
      "GEMINI_API_KEY=k\nDEBUG=0\nHTTPS_PROXY=http://p",
    );
    fireEvent.click(screen.getByRole("button", { name: "+ config.general" }));
    expect(JSON.parse(onConfigChange.mock.calls.at(-1)?.[0])).toEqual({
      ui: {},
      general: { vimMode: true },
    });
  });
});
