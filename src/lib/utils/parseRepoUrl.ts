export interface ParsedRepo {
  host: string;
  owner: string;
  name: string;
}

/**
 * Parses a GitHub repository URL or short "owner/name" string.
 * Supports:
 *   - https://github.com/owner/name
 *   - https://ghes.example.com/owner/name
 *   - https://github.com/owner/name.git
 *   - owner/name  (defaults to github.com)
 */
export function parseRepoUrl(url: string): ParsedRepo | null {
  const cleaned = url.trim().replace(/\.git$/, "");

  const urlMatch = cleaned.match(/^https?:\/\/([^/]+)\/([^/]+)\/([^/]+)/);
  if (urlMatch) {
    return { host: urlMatch[1], owner: urlMatch[2], name: urlMatch[3] };
  }

  const parts = cleaned.split("/");
  if (parts.length === 2 && parts[0] && parts[1]) {
    return { host: "github.com", owner: parts[0], name: parts[1] };
  }

  return null;
}
