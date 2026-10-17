import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { AppId } from "@/lib/api";
import { AppGlyph, type AppGlyphBadge } from "@/components/shell/AppGlyph";

function badgeOf(app: AppId, badge?: AppGlyphBadge) {
  const { container } = render(<AppGlyph app={app} badge={badge} />);
  const terminal = container.querySelector(".lucide-terminal") !== null;
  const desktop = container.querySelector(".lucide-monitor") !== null;
  if (terminal) return "terminal";
  if (desktop) return "desktop";
  return null;
}

describe("AppGlyph badge", () => {
  it("tells Claude Code from Claude Desktop by default", () => {
    expect(badgeOf("claude")).toBe("terminal");
    expect(badgeOf("claude-desktop")).toBe("desktop");
  });

  it("leaves Codex unbadged by default, where the sidebar has a single entry", () => {
    expect(badgeOf("codex")).toBeNull();
  });

  it("lets a row choose the badge, as the Apps page does for the two Codex rows", () => {
    expect(badgeOf("codex", "terminal")).toBe("terminal");
    expect(badgeOf("codex", "desktop")).toBe("desktop");
  });
});
