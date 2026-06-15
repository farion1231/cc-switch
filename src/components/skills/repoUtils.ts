import type { DiscoverableSkill, SkillRepo } from "@/lib/api/skills";

export function normalizeRepoSourceType(sourceType?: string): string {
  return (sourceType || "github").trim().toLowerCase() || "github";
}

export function normalizeRepoSourceHost(
  sourceHost?: string,
  sourceType?: string,
): string {
  const cleaned = (sourceHost || "")
    .trim()
    .replace(/^https?:\/\//, "")
    .replace(/\/$/, "")
    .toLowerCase();

  if (cleaned) return cleaned;
  return normalizeRepoSourceType(sourceType) === "gitlab"
    ? "gitlab.com"
    : "github.com";
}

export function repoDisplayName(repo: SkillRepo): string {
  const sourceHost = normalizeRepoSourceHost(repo.sourceHost, repo.sourceType);
  if (
    normalizeRepoSourceType(repo.sourceType) === "github" &&
    sourceHost === "github.com"
  ) {
    return `${repo.owner}/${repo.name}`;
  }
  return `${sourceHost}/${repo.owner}/${repo.name}`;
}

export function repoExternalUrl(repo: SkillRepo): string {
  const sourceHost = normalizeRepoSourceHost(repo.sourceHost, repo.sourceType);
  return `https://${sourceHost}/${repo.owner}/${repo.name}`;
}

export function skillMatchesRepo(
  skill: DiscoverableSkill,
  repo: SkillRepo,
): boolean {
  return (
    normalizeRepoSourceType(skill.repoSourceType) ===
      normalizeRepoSourceType(repo.sourceType) &&
    normalizeRepoSourceHost(skill.repoSourceHost, skill.repoSourceType) ===
      normalizeRepoSourceHost(repo.sourceHost, repo.sourceType) &&
    skill.repoOwner === repo.owner &&
    skill.repoName === repo.name &&
    (skill.repoBranch || "main") === (repo.branch || "main")
  );
}

export function parseRepoUrl(input: string): SkillRepo | null {
  const trimmed = input.trim();
  if (!trimmed) return null;

  const sshMatch = trimmed.match(/^(?:ssh:\/\/)?git@([^:/]+)[:/](.+)$/);
  if (sshMatch) {
    return parsePathRepo(sshMatch[2], sshMatch[1]);
  }

  if (/^https?:\/\//i.test(trimmed)) {
    try {
      const url = new URL(trimmed);
      return parsePathRepo(url.pathname, url.hostname);
    } catch {
      return null;
    }
  }

  const cleaned = trimmed.replace(/\.git$/, "").replace(/^\/+|\/+$/g, "");
  const parts = cleaned.split("/").filter(Boolean);
  if (parts.length === 2) {
    return {
      sourceType: "github",
      sourceHost: "github.com",
      owner: parts[0],
      name: parts[1],
      branch: "main",
      enabled: true,
    };
  }

  return null;
}

function parsePathRepo(path: string, host: string): SkillRepo | null {
  const sourceHost = normalizeRepoSourceHost(host);
  const sourceType = sourceHost === "github.com" ? "github" : "gitlab";
  const markerIndex = path.indexOf("/-/");
  const repoPath = (markerIndex >= 0 ? path.slice(0, markerIndex) : path)
    .replace(/\.git$/, "")
    .replace(/^\/+|\/+$/g, "");
  const parts = repoPath.split("/").filter(Boolean);

  if (sourceType === "github" && parts.length !== 2) {
    return null;
  }
  if (sourceType === "gitlab" && parts.length < 2) {
    return null;
  }

  const name = parts[parts.length - 1];
  const owner = parts.slice(0, -1).join("/");
  if (!owner || !name) return null;

  return {
    sourceType,
    sourceHost,
    owner,
    name,
    branch: "main",
    enabled: true,
  };
}
