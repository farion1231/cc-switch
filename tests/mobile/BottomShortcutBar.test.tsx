import { useState } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { BottomShortcutBar } from "@/mobile/components/BottomShortcutBar";
import { CustomizeBarSheet } from "@/mobile/components/CustomizeBarSheet";
import { MoreSheet } from "@/mobile/components/MoreSheet";
import { DEFAULT_PINNED_VIEWS, type MobileViewId } from "@/mobile/shortcuts";

function ShortcutBarHarness() {
  const [pinned, setPinned] = useState<MobileViewId[]>([
    ...DEFAULT_PINNED_VIEWS,
  ]);
  const [activeView, setActiveView] = useState<MobileViewId>("overview");
  const [moreOpen, setMoreOpen] = useState(false);
  const [customizeOpen, setCustomizeOpen] = useState(false);

  return (
    <div>
      <div data-testid="active-view">{activeView}</div>
      <BottomShortcutBar
        pinned={pinned}
        activeView={activeView}
        lang="en"
        moreOpen={moreOpen}
        onSelectView={(v) => {
          setActiveView(v);
          setMoreOpen(false);
        }}
        onToggleMore={() => setMoreOpen((v) => !v)}
      />
      <MoreSheet
        open={moreOpen}
        pinned={pinned}
        activeView={activeView}
        lang="en"
        onSelectView={(v) => setActiveView(v)}
        onOpenCustomize={() => setCustomizeOpen(true)}
        onClose={() => setMoreOpen(false)}
      />
      <CustomizeBarSheet
        open={customizeOpen}
        pinned={pinned}
        lang="en"
        onChange={setPinned}
        onClose={() => setCustomizeOpen(false)}
      />
    </div>
  );
}

describe("BottomShortcutBar & CustomizeBarSheet", () => {
  it("opens More sheet, enters Customize modal, reorders, swaps a pinned tab, and resets", () => {
    render(<ShortcutBarHarness />);

    const nav = screen.getByRole("navigation", { name: "Mobile navigation" });
    expect(within(nav).getByText("Overview")).toBeInTheDocument();
    expect(within(nav).getByText("Providers")).toBeInTheDocument();
    expect(within(nav).getByText("Logs")).toBeInTheDocument();
    expect(within(nav).getByText("Proxy")).toBeInTheDocument();
    expect(within(nav).getByText("More")).toBeInTheDocument();

    // Open More sheet -> navigate to Remote Control / Models
    fireEvent.click(within(nav).getByText("More"));
    const moreDialog = screen.getByRole("dialog", { name: "More Pages" });
    expect(within(moreDialog).getByText("Remote Control")).toBeInTheDocument();
    expect(
      within(moreDialog).getByText("Models & Breakdown"),
    ).toBeInTheDocument();
    fireEvent.click(within(moreDialog).getByText("Customize Shortcut Bar"));

    // Customize modal opens with live preview
    const customDialog = screen.getByRole("dialog", {
      name: "Customize Shortcut Bar",
    });
    const preview = within(customDialog).getByTestId("shortcut-bar-preview");
    expect(preview.textContent).toContain("Overview");
    expect(preview.textContent).toContain("More");

    // Unpin "Proxy" (4th pinned item) -> unpinned order is [Proxy, Remote, Models, Preferences]
    const removeButtons = within(customDialog).getAllByRole("button", {
      name: "Remove",
    });
    fireEvent.click(removeButtons[3]);
    expect(preview.textContent).not.toContain("Proxy");

    const pinButtons = within(customDialog).getAllByRole("button", {
      name: "Pin",
    });
    fireEvent.click(pinButtons[1]); // pins Remote Control
    expect(preview.textContent).toContain("Remote");

    // Reorder: move 2nd item earlier
    const moveEarlierBtns = within(customDialog).getAllByRole("button", {
      name: "Move earlier",
    });
    fireEvent.click(moveEarlierBtns[1]);
    expect(preview.textContent?.indexOf("Providers")).toBeLessThan(
      preview.textContent?.indexOf("Overview") ?? 0,
    );

    // Reset restores defaults
    fireEvent.click(
      within(customDialog).getByRole("button", { name: "Reset to Default" }),
    );
    expect(preview.textContent).toContain("Proxy");
    expect(preview.textContent?.indexOf("Overview")).toBeLessThan(
      preview.textContent?.indexOf("Providers") ?? 0,
    );
  });
});
