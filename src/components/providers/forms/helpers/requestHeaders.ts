export const REQUEST_HEADER_DRAFT_PREFIX = "draft-header:";

export function normalizeRequestHeaders(
  headers: Record<string, string>,
): Record<string, string> {
  const normalized: Record<string, string> = {};
  for (const [key, value] of Object.entries(headers)) {
    const trimmedKey = key.trim();
    if (trimmedKey && !key.startsWith(REQUEST_HEADER_DRAFT_PREFIX)) {
      normalized[trimmedKey] = value;
    }
  }
  return normalized;
}

export function findRequestHeaderValue(
  headers: Record<string, string>,
  name: string,
): string | undefined {
  const normalizedName = name.toLowerCase();
  return Object.entries(headers).find(
    ([key]) => key.toLowerCase() === normalizedName,
  )?.[1];
}

/**
 * 请求头的值指纹，顺序无关，用于判断「端点 / 凭据是否真的变了」。
 *
 * 直接拿 headers 对象当依赖不可靠：调用方常写
 * `headers={config.options?.headers ?? {}}`，父组件每渲染一次就是一个新对象，
 * 内容没变也会触发依赖。
 */
export function requestHeadersKey(headers: Record<string, string>): string {
  return JSON.stringify(
    Object.entries(normalizeRequestHeaders(headers))
      .map(([key, value]) => [key.toLowerCase(), value])
      .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)),
  );
}
