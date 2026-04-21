/**
 * Parse repository URL to extract owner, name, branch, and normalized URL.
 * Supports GitHub, GitHub Enterprise, and GitLab URL formats.
 */
export interface ParsedRepoUrl {
  owner: string;
  name: string;
  url: string;
}

function stripDotGit(value: string): string {
  return value.replace(/\.git$/i, "");
}

export function parseRepoUrl(url: string): ParsedRepoUrl | null {
  const cleaned = url.trim();
  if (!cleaned) {
    return null;
  }

  if (!/^https?:\/\//i.test(cleaned)) {
    const parts = cleaned.split("/");
    if (parts.length !== 2 || parts.some((part) => !part)) {
      return null;
    }
    const owner = parts[0];
    const name = stripDotGit(parts[1]);
    if (!name) {
      return null;
    }
    return {
      owner,
      name,
      url: `https://github.com/${owner}/${name}`,
    };
  }

  try {
    const parsed = new URL(cleaned);
    if (parsed.search || parsed.hash) {
      return null;
    }
    const hostname = parsed.hostname.toLowerCase();

    const pathParts = parsed.pathname.split("/").filter(Boolean);
    if (pathParts.length < 2) {
      return null;
    }

    if (hostname.includes("github") && hostname !== "github.com") {
      return null;
    }

    if (hostname === "github.com") {
      if (pathParts.length !== 2) {
        return null;
      }
      const owner = pathParts[0];
      const name = stripDotGit(pathParts[1]);
      if (!owner || !name) {
        return null;
      }
      return {
        owner,
        name,
        url: `${parsed.origin}/${owner}/${name}`,
      };
    }

    if (pathParts.some((part) => !part) || parsed.pathname.includes("/-/")) {
      return null;
    }

    const name = stripDotGit(pathParts[pathParts.length - 1]);
    const namespaces = pathParts.slice(0, -1);
    if (!name || namespaces.length === 0) {
      return null;
    }
    const normalizedPath = [...namespaces, name].join("/");

    return {
      owner: `${parsed.host}/${namespaces.join("/")}`,
      name,
      url: `${parsed.origin}/${normalizedPath}`,
    };
  } catch {
    return null;
  }
}
