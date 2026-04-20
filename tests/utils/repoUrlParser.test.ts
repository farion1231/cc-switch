import { describe, it, expect } from "vitest";
import { parseRepoUrl } from "@/lib/utils/repoUrlParser";

describe("parseRepoUrl", () => {
  it("supports GitHub shorthand", () => {
    expect(parseRepoUrl("owner/repo")).toEqual({
      owner: "owner",
      name: "repo",
      url: "https://github.com/owner/repo",
    });
  });

  it("supports GitHub root repository URL", () => {
    expect(parseRepoUrl("https://github.com/owner/repo")).toEqual({
      owner: "owner",
      name: "repo",
      url: "https://github.com/owner/repo",
    });
  });

  it("supports GitLab root repository URL", () => {
    expect(parseRepoUrl("https://git.example.com/group/project")).toEqual({
      owner: "git.example.com/group",
      name: "project",
      url: "https://git.example.com/group/project",
    });
  });

  it("supports GitLab root repository URL with subgroup", () => {
    expect(
      parseRepoUrl("https://git.example.com/group/subgroup/project"),
    ).toEqual({
      owner: "git.example.com/group/subgroup",
      name: "project",
      url: "https://git.example.com/group/subgroup/project",
    });
  });

  it("trims whitespace around supported URLs", () => {
    expect(parseRepoUrl("  https://github.com/owner/repo/  ")).toEqual({
      owner: "owner",
      name: "repo",
      url: "https://github.com/owner/repo",
    });
  });

  it("rejects GitHub tree URLs", () => {
    expect(parseRepoUrl("https://github.com/owner/repo/tree/main")).toBeNull();
  });

  it("rejects GitHub enterprise URLs", () => {
    expect(parseRepoUrl("https://github.mycorp.com/org/repo")).toBeNull();
  });

  it("rejects GitLab tree URLs", () => {
    expect(
      parseRepoUrl("https://git.example.com/group/project/-/tree/main"),
    ).toBeNull();
  });

  it("rejects GitHub URLs with extra path", () => {
    expect(parseRepoUrl("https://github.com/owner/repo/issues")).toBeNull();
  });

  it("rejects invalid shorthand", () => {
    expect(parseRepoUrl("owner")).toBeNull();
    expect(parseRepoUrl("owner/repo/extra")).toBeNull();
  });
});
