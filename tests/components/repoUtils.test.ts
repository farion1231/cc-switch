import { describe, expect, it } from "vitest";

import { parseRepoUrl } from "@/components/skills/repoUtils";

describe("parseRepoUrl", () => {
  it("parses GitLab HTTPS and SSH URLs, including nested groups", () => {
    expect(
      parseRepoUrl("https://gitlab.com/group/subgroup/skills.git"),
    ).toMatchObject({
      sourceType: "gitlab",
      sourceHost: "gitlab.com",
      owner: "group/subgroup",
      name: "skills",
    });
    expect(
      parseRepoUrl("git@gitlab.example.com:group/skills.git"),
    ).toMatchObject({
      sourceType: "gitlab",
      sourceHost: "gitlab.example.com",
      owner: "group",
      name: "skills",
    });
  });

  it("does not infer a token from repository URLs", () => {
    const parsed = parseRepoUrl(
      "https://git.ushareit.me/it/engineering-ai-skills?private_token=glpat-secret",
    );
    expect(parsed).toMatchObject({
      sourceType: "gitlab",
      sourceHost: "git.ushareit.me",
      owner: "it",
      name: "engineering-ai-skills",
      accessToken: undefined,
    });
  });

  it("parses a self-hosted GitLab host without a URL scheme", () => {
    expect(
      parseRepoUrl("git.ushareit.me/it/engineering-ai-skills"),
    ).toMatchObject({
      sourceType: "gitlab",
      sourceHost: "git.ushareit.me",
      owner: "it",
      name: "engineering-ai-skills",
    });
  });
});
