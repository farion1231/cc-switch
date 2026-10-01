import { useRef, useState } from "react";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { ClaudeDesktopDisplaySettings } from "@/components/settings/ClaudeDesktopDisplaySettings";
import type { ClaudeDesktopDisplay } from "@/types";

/** The component is controlled, so the test owns the state round trip. */
function Harness({ initial }: { initial: ClaudeDesktopDisplay | null }) {
  const [value, setValue] = useState<ClaudeDesktopDisplay | null>(initial);
  return <ClaudeDesktopDisplaySettings value={value} onChange={setValue} />;
}

/** How long the fixture takes to refetch after a save is issued. */
const REFETCH_DELAY_MS = 150;
/** The debounce an edit waits out before it is saved. */
const AUTOSAVE_DELAY_MS = 400;
/** Long enough for that debounce and the refetch it triggers to both settle. */
const settle = async () => {
  await act(async () => {
    await new Promise((resolve) =>
      setTimeout(resolve, AUTOSAVE_DELAY_MS + REFETCH_DELAY_MS + 100),
    );
  });
};

/**
 * Stands in for SettingsPage plus the settings query. The first save of a
 * burst snapshots the value as it stands, and when the query refetches it
 * pushes that snapshot back as the new prop: the response was already in
 * flight when the save went out, so it still describes the state before it.
 * The refetch right behind it reports the value that was saved.
 */
function SavingHarness({
  initial,
  onSaved,
  onReady,
}: {
  initial: ClaudeDesktopDisplay;
  onSaved?: (value: ClaudeDesktopDisplay | null) => void;
  /** Hands the test the setter an external change to the setting would use. */
  onReady?: (external: (next: ClaudeDesktopDisplay) => void) => void;
}) {
  const [value, setValue] = useState<ClaudeDesktopDisplay | null>(initial);
  const savedRef = useRef<ClaudeDesktopDisplay | null>(initial);
  const snapshotRef = useRef<ClaudeDesktopDisplay | null>(null);
  const savingRef = useRef(false);

  onReady?.(setValue);

  return (
    <ClaudeDesktopDisplaySettings
      value={value}
      onChange={(next) => {
        const first = !savingRef.current;
        if (first) snapshotRef.current = savedRef.current;
        savedRef.current = next;
        savingRef.current = true;
        onSaved?.(next);
        setTimeout(() => {
          if (first && snapshotRef.current) setValue(snapshotRef.current);
          setValue(savedRef.current);
          savingRef.current = false;
        }, REFETCH_DELAY_MS);
      }}
    />
  );
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

const nameInput = () =>
  screen.getByLabelText("settings.claudeDesktopDisplayName");

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

describe("ClaudeDesktopDisplaySettings against a refetching settings query", () => {
  it("keeps every keystroke when a stale refetch lands mid-typing", async () => {
    const user = userEvent.setup();
    const saved: (ClaudeDesktopDisplay | null)[] = [];
    render(
      <SavingHarness
        initial={{ ...ENABLED, name: "" }}
        onSaved={(v) => saved.push(v)}
      />,
    );

    await user.type(nameInput(), "AB");
    await user.type(nameInput(), "C");
    expect(nameInput()).toHaveValue("ABC");

    // Typing is debounced: nothing is saved until the user pauses.
    expect(saved).toHaveLength(0);
    await waitFor(() => expect(saved.at(-1)?.name).toBe("ABC"));

    // The save's refetch answers with the pre-save snapshot first and the saved
    // value second; neither may disturb what is on screen.
    await settle();
    expect(nameInput()).toHaveValue("ABC");
    expect(saved.at(-1)?.name).toBe("ABC");
  });

  it("ignores an external update while an edit is pending, and takes it once the save settles", async () => {
    const user = userEvent.setup();
    let external!: (next: ClaudeDesktopDisplay) => void;
    render(
      <SavingHarness
        initial={{ ...ENABLED, name: "" }}
        onReady={(drive) => {
          external = drive;
        }}
      />,
    );

    await user.type(nameInput(), "AB");
    act(() => external({ ...ENABLED, name: "External" }));
    expect(nameInput()).toHaveValue("AB");

    // The save's own stale snapshot must not clobber the draft either.
    await settle();
    expect(nameInput()).toHaveValue("AB");

    // The draft is clean again, so later external updates flow through.
    act(() => external({ ...ENABLED, name: "External" }));
    expect(nameInput()).toHaveValue("External");
  });
});
