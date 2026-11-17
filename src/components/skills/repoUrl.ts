/** GitHub 的主机名，也是仓库没写 `host` 时的取值（与后端 `GITHUB_HOST` 一致） */
export const GITHUB_HOST = "github.com";

export interface ParsedRepoUrl {
  /** 只有 GitLab 仓库才带；GitHub 仓库不写，后端按 github.com 处理 */
  host?: string;
  owner: string;
  name: string;
}

/** 仓库首页：GitHub 与 GitLab 都是 `https://<主机>/<owner>/<name>` */
export function repoWebUrl(repo: {
  host?: string;
  owner: string;
  name: string;
}): string {
  return `https://${repo.host || GITHUB_HOST}/${repo.owner}/${repo.name}`;
}

const stripRepoSuffix = (path: string) =>
  path.replace(/\/+$/, "").replace(/\.git$/, "");

function fromHostAndPath(host: string, path: string): ParsedRepoUrl | null {
  const normalizedHost = host
    .toLowerCase()
    .replace(/^www\.(?=github\.com$)/, "");
  // GitLab 页面地址里 `/-/` 之后是界面路径（`/-/tree/main`…），不属于项目路径
  const marker = path.indexOf("/-/");
  const parts = stripRepoSuffix(marker >= 0 ? path.slice(0, marker) : path)
    .split("/")
    .filter(Boolean);

  if (normalizedHost === GITHUB_HOST) {
    return parts.length === 2 ? { owner: parts[0], name: parts[1] } : null;
  }
  // GitLab 的分组可以嵌套：最后一段是项目名，前面整段是分组路径
  if (parts.length < 2) return null;
  return {
    host: normalizedHost,
    owner: parts.slice(0, -1).join("/"),
    name: parts[parts.length - 1],
  };
}

/**
 * 支持的写法：
 * - `owner/name`（GitHub）
 * - `https://github.com/owner/name[.git]`
 * - `https://<GitLab 主机>[:端口]/group[/subgroup…]/project[.git]`
 * - `git@<主机>:group/project.git`、`ssh://git@<主机>[:端口]/group/project.git`
 *   （只取坐标，下载仍走 https；SSH 端口与 https 端口无关，丢掉）
 */
export function parseRepoUrl(url: string): ParsedRepoUrl | null {
  const trimmed = url.trim();
  if (!trimmed) return null;

  // scp 写法没有端口：冒号后面整段都是路径
  const scp = trimmed.match(/^git@([^:/]+):(.+)$/);
  if (scp) return fromHostAndPath(scp[1], scp[2]);

  if (/^ssh:\/\//i.test(trimmed)) {
    try {
      const parsed = new URL(trimmed);
      return fromHostAndPath(parsed.hostname, parsed.pathname);
    } catch {
      return null;
    }
  }

  if (/^https?:\/\//i.test(trimmed)) {
    try {
      const parsed = new URL(trimmed);
      // `host` 带端口，`hostname` 不带：自建实例常用非默认端口
      return fromHostAndPath(parsed.host, parsed.pathname);
    } catch {
      return null;
    }
  }

  const parts = stripRepoSuffix(trimmed).split("/");
  if (parts.length === 2 && parts[0] && parts[1]) {
    return { owner: parts[0], name: parts[1] };
  }
  return null;
}

/**
 * 明文 http 的 GitLab 地址：技能内容会被 AI 工具当指令执行，下载只走 https，
 * 所以这类地址要单独提示，而不是存进去再报一个看不懂的网络错误。
 */
export function isInsecureRepoUrl(url: string): boolean {
  const trimmed = url.trim();
  if (!/^http:\/\//i.test(trimmed)) return false;
  return Boolean(parseRepoUrl(trimmed)?.host);
}
