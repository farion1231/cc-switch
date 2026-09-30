import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { ClaudeDesktopDisplaySettings } from "@/components/settings/ClaudeDesktopDisplaySettings";
import type { ClaudeDesktopDisplay } from "@/types";

/** The component is controlled, so the test owns the state round trip. */
function Harness({ initial }: { initial: ClaudeDesktopDisplay | null }) {
  const [value, setValue] = useState<ClaudeDesktopDisplay | null>(initial);
  return <ClaudeDesktopDisplaySettings value={value} onChange={setValue} />;
}

const ENABLED: ClaudeDesktopDisplay = {
  name: "Chris",
  subtitle: "Gateway",
  attribution: false,
};

/** Component tests run against empty i18n resources, so a label's accessible
 *  name is its key — querying that way also pins the label/switch wiring. */
const enableSwitch = () =>
  screen.getByRole("switch", { name: "settings.claudeDesktopDisplayEnable" });

describe("ClaudeDesktopDisplaySettings", () => {
  it("shows the stored values while enabled", () => {
    render(<Harness initial={ENABLED} />);
    expect(screen.getByDisplayValue("Chris")).toBeInTheDocument();
    expect(screen.getByDisplayValue("Gateway")).toBeInTheDocument();
    expect(enableSwitch()).toBeChecked();
  });

  it("hides the fields and clears the setting when switched off", async () => {
    const user = userEvent.setup();
    render(<Harness initial={ENABLED} />);

    await user.click(enableSwitch());

    expect(screen.queryByDisplayValue("Chris")).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue("Gateway")).not.toBeInTheDocument();
  });

  it("restores what was typed when switched off and back on", async () => {
    const user = userEvent.setup();
    render(<Harness initial={ENABLED} />);

    await user.click(enableSwitch()); // off - the setting is dropped
    await user.click(enableSwitch()); // on again

    expect(screen.getByDisplayValue("Chris")).toBeInTheDocument();
    expect(screen.getByDisplayValue("Gateway")).toBeInTheDocument();
  });

  it("writes edits back through onChange", async () => {
    const user = userEvent.setup();
    render(<Harness initial={{ ...ENABLED, name: "" }} />);

    await user.type(screen.getByDisplayValue(""), "New Name");

    expect(screen.getByDisplayValue("New Name")).toBeInTheDocument();
  });
});
