/**
 * Parse repository URL to extract owner, name, branch, and normalized URL.
 * Supports GitHub, GitHub Enterprise, and GitLab URL formats.
 */
export interface ParsedRepoUrl {
  owner: string;
  name: string;
  url: string;
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
    return {
      owner: parts[0],
      name: parts[1],
      url: `https://github.com/${parts[0]}/${parts[1]}`,
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
      const [owner, name] = pathParts;
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

    const name = pathParts[pathParts.length - 1];
    const namespaces = pathParts.slice(0, -1);
    if (!name || namespaces.length === 0) {
      return null;
    }

    return {
      owner: `${parsed.host}/${namespaces.join("/")}`,
      name,
      url: `${parsed.origin}/${pathParts.join("/")}`,
    };
  } catch {
    return null;
  }
}
