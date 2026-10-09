import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  skillsApi,
  type DiscoverableSkill,
  type SkillInstallProgress,
} from "@/lib/api/skills";

const m = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: m.invoke,
  Channel: class {
    onmessage: (event: SkillInstallProgress) => void = () => {};
  },
}));
const skill: DiscoverableSkill = {
  key: "owner/repo:example",
  name: "example",
  description: "",
  directory: "example",
  repoOwner: "owner",
  repoName: "repo",
  repoBranch: "main",
};
const event: SkillInstallProgress = {
  phase: "downloading",
  downloadedBytes: 1024,
  totalBytes: 2048,
};

describe("Skill installation progress IPC", () => {
  beforeEach(() => m.invoke.mockReset());

  it("binds a dedicated channel before invocation and ignores late messages after success", async () => {
    let channel: { onmessage: (progress: SkillInstallProgress) => void };
    m.invoke.mockImplementation(async (command, args) => {
      if (command !== "install_skill_unified") return "windows";
      channel = args.onProgress;
      channel.onmessage(event);
      return { id: skill.key };
    });
    const onProgress = vi.fn();
    await skillsApi.installUnified(skill, "codex", onProgress);
    expect(onProgress).toHaveBeenCalledWith(event);
    expect(m.invoke).toHaveBeenCalledWith("install_skill_unified", {
      skill,
      currentApp: "codex",
      onProgress: expect.any(Object),
    });
    channel!.onmessage(event);
    expect(onProgress).toHaveBeenCalledTimes(1);
  });

  it("ignores late messages after failure so a retry cannot receive stale progress", async () => {
    let channel: { onmessage: (progress: SkillInstallProgress) => void };
    m.invoke.mockImplementation(async (command, args) => {
      if (command !== "install_skill_unified") return "windows";
      channel = args.onProgress;
      throw new Error("timeout");
    });
    const onProgress = vi.fn();
    await expect(
      skillsApi.installUnified(skill, "codex", onProgress),
    ).rejects.toThrow("timeout");
    channel!.onmessage(event);
    expect(onProgress).not.toHaveBeenCalled();
  });

  it("preserves compatibility with installation calls without progress", async () => {
    m.invoke.mockResolvedValue({ id: skill.key });
    await skillsApi.installUnified(skill, "codex");
    expect(m.invoke).toHaveBeenCalledWith("install_skill_unified", {
      skill,
      currentApp: "codex",
    });
  });
});
