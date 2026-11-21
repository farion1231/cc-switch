import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import {
  ClineCliModelNotice,
  hasClineCustomProvider,
} from "@/components/providers/ClineCliModelNotice";
import type { Provider } from "@/types";

const custom = {
  id: "ccs-relay",
  name: "Relay",
  settingsConfig: {
    settings: { model: "m1" },
    provider: { name: "Relay", baseUrl: "https://relay.example.com/v1" },
    models: { m1: { id: "m1" } },
  },
} as Provider;
const builtIn = {
  id: "anthropic",
  name: "Anthropic",
  settingsConfig: { settings: { model: "claude-sonnet-5-5" } },
} as Provider;

describe("ClineCliModelNotice", () => {
  beforeEach(() => window.localStorage.clear());

  it("only applies to custom providers", () => {
    expect(hasClineCustomProvider({ anthropic: builtIn })).toBe(false);
    expect(hasClineCustomProvider({ anthropic: builtIn, relay: custom })).toBe(
      true,
    );
    const { container } = render(
      <ClineCliModelNotice providers={{ anthropic: builtIn }} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("stays hidden after it is dismissed", () => {
    const { unmount } = render(
      <ClineCliModelNotice providers={{ relay: custom }} />,
    );
    expect(screen.getByText("cline.cliModel.title")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "common.close" }));
    expect(screen.queryByText("cline.cliModel.title")).not.toBeInTheDocument();
    unmount();

    render(<ClineCliModelNotice providers={{ relay: custom }} />);
    expect(screen.queryByText("cline.cliModel.title")).not.toBeInTheDocument();
  });
});
