import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSkillInstallTargets } from "@/components/skills/useSkillInstallTargets";

const toggle = vi.hoisted(() => vi.fn());
vi.mock("@/hooks/useSkills", () => ({
  useToggleSkillApp: () => ({ mutateAsync: toggle }),
}));

beforeEach(() => {
  localStorage.clear();
  toggle.mockReset();
});

describe("Unified Skill install targets", () => {
  it("excludes locked default targets and does not toggle explicitly supplied locked apps", async () => {
    const { result } = renderHook(() =>
      useSkillInstallTargets(
        ["claude", "codex", "gemini"],
        ["codex", "gemini"],
      ),
    );
    expect(result.current.targets).toEqual(["claude"]);
    const install = vi.fn().mockResolvedValue([{ id: "skill" }]);
    await act(async () => {
      const outcome = await result.current.installTo(install, [
        "claude",
        "codex",
        "gemini",
      ]);
      expect(outcome.failures).toEqual([]);
    });
    expect(install).toHaveBeenCalledWith("claude");
    expect(toggle).not.toHaveBeenCalled();
  });

  it("can install to SSOT when only locked applications are visible", async () => {
    const { result } = renderHook(() =>
      useSkillInstallTargets(["codex"], ["codex"]),
    );
    const install = vi.fn().mockResolvedValue([{ id: "skill" }]);
    await act(async () => {
      await result.current.installTo(install);
    });
    expect(install).toHaveBeenCalledWith("codex");
    expect(toggle).not.toHaveBeenCalled();
  });

  it("reapplies remembered targets after leaving Unified mode", () => {
    const { result, rerender } = renderHook(
      ({ locked }) => useSkillInstallTargets(["claude", "codex"], locked),
      { initialProps: { locked: ["codex"] as "codex"[] } },
    );
    expect(result.current.targets).toEqual(["claude"]);
    rerender({ locked: [] });
    expect(result.current.targets).toEqual(["claude", "codex"]);
  });
});
