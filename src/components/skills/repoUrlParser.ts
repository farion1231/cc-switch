/**
 * 私有仓库 URL 解析器
 * 
 * 用于解析私有仓库的完整 URL，提取 baseUrl、owner 和 name
 */

/**
 * 解析后的仓库 URL 结构
 */
export interface ParsedRepoUrl {
  /** 协议 + 域名，如 https://gitlab.company.com */
  baseUrl: string;
  /** 仓库所有者/组织 */
  owner: string;
  /** 仓库名称 */
  name: string;
}

/**
 * 解析私有仓库 URL
 * 
 * 支持格式：https://host/owner/name 或 https://host/owner/name.git
 * 
 * @param url - 完整的仓库 URL
 * @returns 解析结果，如果格式无效则返回 null
 * 
 * @example
 * parsePrivateRepoUrl("https://gitlab.company.com/team/project")
 * // => { baseUrl: "https://gitlab.company.com", owner: "team", name: "project" }
 * 
 * parsePrivateRepoUrl("https://github.com/owner/repo.git")
 * // => { baseUrl: "https://github.com", owner: "owner", name: "repo" }
 */
export function parsePrivateRepoUrl(url: string): ParsedRepoUrl | null {
  if (!url || typeof url !== "string") {
    return null;
  }

  const trimmed = url.trim();
  
  // 必须是 https:// 开头
  if (!trimmed.startsWith("https://")) {
    return null;
  }

  try {
    const urlObj = new URL(trimmed);
    
    // 获取路径部分，去除开头的 /
    let pathname = urlObj.pathname;
    if (pathname.startsWith("/")) {
      pathname = pathname.slice(1);
    }
    
    // 去除 .git 后缀
    if (pathname.endsWith(".git")) {
      pathname = pathname.slice(0, -4);
    }
    
    // 去除末尾的 /
    if (pathname.endsWith("/")) {
      pathname = pathname.slice(0, -1);
    }

    // 分割路径，获取 owner 和 name
    const parts = pathname.split("/").filter(Boolean);
    
    // 至少需要两个部分：owner 和 name
    if (parts.length < 2) {
      return null;
    }

    // 取前两个部分作为 owner 和 name
    const owner = parts[0];
    const name = parts[1];

    // owner 和 name 不能为空
    if (!owner || !name) {
      return null;
    }

    // 构建 baseUrl（协议 + 域名）
    const baseUrl = `${urlObj.protocol}//${urlObj.host}`;

    return {
      baseUrl,
      owner,
      name,
    };
  } catch {
    // URL 解析失败
    return null;
  }
}

/**
 * 遮蔽 Token 显示
 * 
 * 将 access token 转换为遮蔽显示格式，保护敏感信息
 * 
 * @param token - 原始 token
 * @param visibleChars - 可见字符数（前后各显示的字符数），默认为 0（完全遮蔽）
 * @returns 遮蔽后的字符串
 * 
 * @example
 * maskToken("ghp_xxxxxxxxxxxxxxxxxxxx")
 * // => "••••••••"
 * 
 * maskToken("ghp_xxxxxxxxxxxxxxxxxxxx", 4)
 * // => "ghp_••••••••xxxx"
 * 
 * maskToken("")
 * // => ""
 */
export function maskToken(token: string, visibleChars: number = 0): string {
  if (!token || typeof token !== "string") {
    return "";
  }

  const trimmed = token.trim();
  if (trimmed.length === 0) {
    return "";
  }

  // 固定长度的遮蔽字符
  const maskedPart = "••••••••";

  // 完全遮蔽模式
  if (visibleChars <= 0) {
    return maskedPart;
  }

  // 如果 token 太短，直接完全遮蔽
  if (trimmed.length <= visibleChars * 2) {
    return maskedPart;
  }

  // 显示前后各 visibleChars 个字符
  const prefix = trimmed.slice(0, visibleChars);
  const suffix = trimmed.slice(-visibleChars);

  return `${prefix}${maskedPart}${suffix}`;
}

/**
 * 判断是否为私有仓库
 * 
 * 通过检查 access_token 是否存在来判断
 * 
 * @param repo - 仓库配置对象
 * @returns 是否为私有仓库
 */
export function isPrivateRepo(repo: { access_token?: string }): boolean {
  return Boolean(repo.access_token && repo.access_token.trim().length > 0);
}
