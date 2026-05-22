import { describe, expect, it } from "vitest";
import { parseRepoUrl } from "@/lib/utils/parseRepoUrl";

describe("parseRepoUrl", () => {
  // ── github.com 短格式 ────────────────────────────────────────────────────
  it("parses owner/name short format as github.com", () => {
    const result = parseRepoUrl("anthropics/skills");
    expect(result).toEqual({
      host: "github.com",
      owner: "anthropics",
      name: "skills",
    });
  });

  it("trims whitespace from short format", () => {
    const result = parseRepoUrl("  owner/repo  ");
    expect(result).toEqual({ host: "github.com", owner: "owner", name: "repo" });
  });

  // ── github.com 完整 URL ──────────────────────────────────────────────────
  it("parses full github.com URL", () => {
    const result = parseRepoUrl("https://github.com/anthropics/skills");
    expect(result).toEqual({
      host: "github.com",
      owner: "anthropics",
      name: "skills",
    });
  });

  it("strips .git suffix from full URL", () => {
    const result = parseRepoUrl("https://github.com/anthropics/skills.git");
    expect(result).toEqual({
      host: "github.com",
      owner: "anthropics",
      name: "skills",
    });
  });

  it("strips .git suffix from short format", () => {
    const result = parseRepoUrl("anthropics/skills.git");
    expect(result).toEqual({
      host: "github.com",
      owner: "anthropics",
      name: "skills",
    });
  });

  it("handles http:// prefix", () => {
    const result = parseRepoUrl("http://github.com/owner/repo");
    expect(result).toEqual({ host: "github.com", owner: "owner", name: "repo" });
  });

  // ── GHES 完整 URL ────────────────────────────────────────────────────────
  it("parses GHES URL and preserves enterprise host", () => {
    const result = parseRepoUrl("https://ghes.example.com/my-org/my-skills");
    expect(result).toEqual({
      host: "ghes.example.com",
      owner: "my-org",
      name: "my-skills",
    });
  });

  it("parses GHES URL with .git suffix", () => {
    const result = parseRepoUrl("https://ghes.example.com/org/repo.git");
    expect(result).toEqual({
      host: "ghes.example.com",
      owner: "org",
      name: "repo",
    });
  });

  it("parses GHES URL with subdomain", () => {
    const result = parseRepoUrl("https://git.internal.corp/team/skills");
    expect(result).toEqual({
      host: "git.internal.corp",
      owner: "team",
      name: "skills",
    });
  });

  it("parses GHES URL with port", () => {
    const result = parseRepoUrl("https://ghes.example.com:8443/org/repo");
    expect(result).toEqual({
      host: "ghes.example.com:8443",
      owner: "org",
      name: "repo",
    });
  });

  // ── 无效输入 ─────────────────────────────────────────────────────────────
  it("returns null for empty string", () => {
    expect(parseRepoUrl("")).toBeNull();
  });

  it("returns null for bare repo name without owner", () => {
    expect(parseRepoUrl("just-a-repo")).toBeNull();
  });

  it("returns null for URL missing repo name", () => {
    expect(parseRepoUrl("https://github.com/only-owner")).toBeNull();
  });

  it("returns null for three-segment short format", () => {
    expect(parseRepoUrl("a/b/c")).toBeNull();
  });
});
