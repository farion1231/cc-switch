/** GitHub 的主机名，也是仓库没写 `host` 时的取值（与后端 `GITHUB_HOST` 一致） */
export const GITHUB_HOST = "github.com";

export interface ParsedRepoUrl {
  /** GitLab / Gitea 仓库才带；GitHub 仓库不写，后端按 github.com 处理 */
  host?: string;
  owner: string;
  name: string;
}

/** 仓库首页：GitHub、GitLab、Gitea 都是 `https://<主机>/<owner>/<name>` */
export function repoWebUrl(repo: {
  host?: string;
  owner: string;
  name: string;
}): string {
  return `https://${repo.host || GITHUB_HOST}/${repo.owner}/${repo.name}`;
}

const GITEA_REF_KINDS = ["branch", "tag", "commit"];

const stripRepoSuffix = (path: string) =>
  path.replace(/\/+$/, "").replace(/\.git$/, "");

function fromHostAndPath(
  host: string,
  path: string,
  isWebUrl = false,
): ParsedRepoUrl | null {
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
  // Gitea / Forgejo 的页面地址是 `owner/repo/src/branch/main/…`（还有 tag、commit），
  // 仓库只有前两段。这里分不清主机是哪种平台，只能认形状，所以收得很紧：
  // 带 `/-/` 的是 GitLab 页面，以 `.git` 结尾的和 SSH 写法是克隆地址，都不是它。
  // 剩下认错的只有 GitLab 上叫 `group/x/src/branch/…` 的嵌套分组的网页地址，
  // 那种改填克隆地址（带 `.git`）即可。
  const isGiteaPage =
    isWebUrl &&
    marker < 0 &&
    !/\.git\/*$/.test(path) &&
    parts[2] === "src" &&
    GITEA_REF_KINDS.includes(parts[3]);
  if (isGiteaPage) {
    return { host: normalizedHost, owner: parts[0], name: parts[1] };
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
 * - `https://<Gitea 主机>[:端口]/owner/repo[.git]`（Forgejo / Codeberg 同）
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
      return fromHostAndPath(parsed.host, parsed.pathname, true);
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
 * 明文 http 的 GitLab / Gitea 地址：技能内容会被 AI 工具当指令执行，下载只走 https，
 * 所以这类地址要单独提示，而不是存进去再报一个看不懂的网络错误。
 */
export function isInsecureRepoUrl(url: string): boolean {
  const trimmed = url.trim();
  if (!/^http:\/\//i.test(trimmed)) return false;
  return Boolean(parseRepoUrl(trimmed)?.host);
}

/**
 * 访问令牌按主机存，输入框却常被贴进整条仓库地址：从中取出主机（带端口），
 * 小写。`gitlab.example.com/group/x`、`https://…`、`git@host:…` 都认；
 * 取不出就原样（去空白、小写）交给后端校验，由它报「站点无效」。
 */
export function tokenHostFromInput(input: string): string {
  const trimmed = input.trim();
  let host = trimmed.split("/")[0] ?? "";
  const scp = trimmed.match(/^git@([^:/]+):/);
  if (scp) {
    host = scp[1];
  } else if (/^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed)) {
    try {
      const parsed = new URL(trimmed);
      // 与 parseRepoUrl 一致：ssh:// 的端口是 SSH 的，下载走 https 默认端口，不算进主机
      host = /^ssh:/i.test(trimmed) ? parsed.hostname : parsed.host;
    } catch {
      host = trimmed;
    }
  }
  return host.toLowerCase().replace(/^www\.(?=github\.com$)/, "");
}
