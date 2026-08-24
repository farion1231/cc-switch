import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AppSwitcher } from "@/components/AppSwitcher";

describe("AppSwitcher", () => {
  it("keeps the Codex Desktop tooltip on the app button", () => {
    render(
      <AppSwitcher
        activeApp="codex-desktop"
        onSwitch={() => {}}
        visibleApps={{
          claude: false,
          "claude-desktop": false,
          codex: false,
          "codex-desktop": true,
          gemini: false,
          grokbuild: false,
          opencode: false,
          openclaw: false,
          hermes: false,
          pi: false,
        }}
      />,
    );

    const button = screen.getByRole("button", { name: "Codex Desktop" });
    expect(button).toHaveAttribute("title", "Codex Desktop");
    expect(button.querySelector("[aria-hidden='true']")).toHaveClass(
      "pointer-events-none",
    );
  });
});
