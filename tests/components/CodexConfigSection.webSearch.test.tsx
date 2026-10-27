import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { parse } from "smol-toml";
import { CodexConfigSection } from "@/components/providers/forms/CodexConfigSections";

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      aria-label="TOML"
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  ),
}));

const config = `model_provider = "relay"
web_search = "live"
[model_providers.relay]
name = "Relay"
wire_api = "responses"
`;
const toggle = () =>
  screen.getByRole("checkbox", {
    name: "codexConfig.enableStandaloneWebSearch",
  });

describe("standalone web search configuration toggle", () => {
  it("writes both true and false through the existing TOML save callback", () => {
    const onChange = vi.fn();
    render(<CodexConfigSection value={config} onChange={onChange} />);
    expect(toggle()).not.toBeChecked();
    fireEvent.click(toggle());
    expect(toggle()).toBeChecked();
    expect(
      (parse(onChange.mock.lastCall![0]) as any).model_providers.relay
        .supports_standalone_web_search,
    ).toBe(true);
    fireEvent.click(toggle());
    expect(toggle()).not.toBeChecked();
    expect(
      (parse(onChange.mock.lastCall![0]) as any).model_providers.relay
        .supports_standalone_web_search,
    ).toBe(false);
  });

  it("reflects manual edits and switching between provider configurations", () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <CodexConfigSection value={config} onChange={onChange} />,
    );
    fireEvent.change(screen.getByLabelText("TOML"), {
      target: { value: `${config}supports_standalone_web_search = true\n` },
    });
    expect(toggle()).toBeChecked();
    rerender(
      <CodexConfigSection
        value={config.replace('"relay"', '"another"')}
        onChange={onChange}
      />,
    );
    expect(toggle()).not.toBeChecked();
  });

  it("hides the capability control when disabled by the official provider form", () => {
    render(
      <CodexConfigSection
        value={config}
        onChange={vi.fn()}
        showStandaloneWebSearch={false}
      />,
    );
    expect(
      screen.queryByRole("checkbox", {
        name: "codexConfig.enableStandaloneWebSearch",
      }),
    ).not.toBeInTheDocument();
  });

  it("disables the toggle when TOML is invalid or no custom provider is selected", () => {
    const { rerender } = render(
      <CodexConfigSection
        value={config}
        onChange={vi.fn()}
        configError="Invalid TOML"
      />,
    );
    expect(toggle()).toBeDisabled();
    rerender(<CodexConfigSection value="" onChange={vi.fn()} />);
    expect(toggle()).toBeDisabled();
  });
});

describe("live web search configuration toggle", () => {
  const liveToggle = () =>
    screen.getByRole("checkbox", {
      name: "codexConfig.enableLiveWebSearch",
    });

  it("sets live then cached without changing the standalone capability", () => {
    const onChange = vi.fn();
    render(
      <CodexConfigSection
        value={config.replace('web_search = "live"\n', "")}
        onChange={onChange}
      />,
    );
    expect(liveToggle()).not.toBeChecked();
    fireEvent.click(liveToggle());
    expect(liveToggle()).toBeChecked();
    expect(parse(onChange.mock.lastCall![0]).web_search).toBe("live");
    expect(toggle()).not.toBeChecked();
    fireEvent.click(liveToggle());
    expect(parse(onChange.mock.lastCall![0]).web_search).toBe("cached");
    expect(liveToggle()).not.toBeChecked();
  });

  it("is available for official providers and reflects manual TOML edits", () => {
    render(
      <CodexConfigSection
        value=""
        onChange={vi.fn()}
        showStandaloneWebSearch={false}
      />,
    );
    expect(liveToggle()).toBeEnabled();
    fireEvent.change(screen.getByLabelText("TOML"), {
      target: { value: 'web_search = "live"\n' },
    });
    expect(liveToggle()).toBeChecked();
    fireEvent.change(screen.getByLabelText("TOML"), {
      target: { value: 'web_search = "indexed"\n' },
    });
    expect(liveToggle()).not.toBeChecked();
  });

  it("disables editing when the configuration has an error", () => {
    render(
      <CodexConfigSection
        value={config}
        onChange={vi.fn()}
        configError="Invalid TOML"
      />,
    );
    expect(liveToggle()).toBeDisabled();
  });
});
