import type { ProviderMeta } from "@/types";

/** 后端 `http_client::build_client_from` 接受的代理协议 */
const PROXY_SCHEMES = ["http:", "https:", "socks5:", "socks5h:"];

/** 代理地址是否可用：能解析、带主机名、协议是 http/https/socks5/socks5h */
export function isValidProxyUrl(value: string): boolean {
  try {
    const parsed = new URL(value.trim());
    return PROXY_SCHEMES.includes(parsed.protocol) && !!parsed.hostname;
  } catch {
    return false;
  }
}

/** 从完整 URL 提取认证信息 */
export function extractProxyAuth(url: string): {
  baseUrl: string;
  username: string;
  password: string;
} {
  if (!url.trim()) return { baseUrl: "", username: "", password: "" };

  try {
    const parsed = new URL(url);
    const username = decodeURIComponent(parsed.username || "");
    const password = decodeURIComponent(parsed.password || "");
    // 移除认证信息，获取基础 URL
    parsed.username = "";
    parsed.password = "";
    return { baseUrl: parsed.toString(), username, password };
  } catch {
    return { baseUrl: url, username: "", password: "" };
  }
}

/** 将认证信息合并到 URL */
export function mergeProxyAuth(
  baseUrl: string,
  username: string,
  password: string,
): string {
  if (!baseUrl.trim()) return "";
  if (!username.trim()) return baseUrl;

  try {
    const parsed = new URL(baseUrl);
    // URL 对象的 username/password setter 会自动进行 percent-encoding
    // 不要使用 encodeURIComponent，否则会导致双重编码
    parsed.username = username.trim();
    if (password) {
      parsed.password = password;
    }
    return parsed.toString();
  } catch {
    // URL 解析失败，尝试手动插入（此时需要手动编码）
    const match = baseUrl.match(/^(\w+:\/\/)(.+)$/);
    if (match) {
      const auth = password
        ? `${encodeURIComponent(username.trim())}:${encodeURIComponent(password)}@`
        : `${encodeURIComponent(username.trim())}@`;
      return `${match[1]}${auth}${match[2]}`;
    }
    return baseUrl;
  }
}

/**
 * 供应商代理表单用的拆分：同 {@link extractProxyAuth}，但用户没写根路径的「/」时不补上，
 * 输入框里看到的就是用户写的样子。
 */
export function splitProxyUrl(url: string): {
  baseUrl: string;
  username: string;
  password: string;
} {
  const parts = extractProxyAuth(url);
  if (!url.trim().endsWith("/")) {
    parts.baseUrl = parts.baseUrl.replace(/\/$/, "");
  }
  return parts;
}

/** {@link splitProxyUrl} 的逆操作：合并账号密码，地址没写结尾「/」时也不补 */
export function joinProxyUrl(
  baseUrl: string,
  username: string,
  password: string,
): string {
  const joined = mergeProxyAuth(baseUrl, username, password);
  return baseUrl.trim().endsWith("/") ? joined : joined.replace(/\/$/, "");
}

/** 把供应商代理写进 meta：空值时去掉字段；meta 原本没有且代理为空时保持 undefined */
export function withProviderProxy(
  meta: ProviderMeta | undefined,
  proxyUrl: string,
): ProviderMeta | undefined {
  const trimmed = proxyUrl.trim();
  if (!meta && !trimmed) return meta;
  const next: ProviderMeta = { ...(meta ?? {}) };
  if (trimmed) next.proxyUrl = trimmed;
  else delete next.proxyUrl;
  return next;
}
