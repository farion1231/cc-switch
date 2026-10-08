import { Fragment, type ReactNode } from "react";

/**
 * 账号第二行的登录日期（「9月12日」；不是今年的带上年份）。
 * authenticated_at 是秒；0 / 缺失时不写。
 */
export function signedInDate(
  authenticatedAt: number | null | undefined,
  locale: string,
): string | null {
  if (!authenticatedAt || authenticatedAt <= 0) return null;
  const date = new Date(authenticatedAt * 1000);
  if (Number.isNaN(date.getTime())) return null;
  const sameYear = date.getFullYear() === new Date().getFullYear();
  try {
    return new Intl.DateTimeFormat(
      locale,
      sameYear
        ? { month: "short", day: "numeric" }
        : { year: "numeric", month: "short", day: "numeric" },
    ).format(date);
  } catch {
    return date.toLocaleDateString();
  }
}

/** 文案里的标识符（id_token）用等宽字体 */
export function withMonoToken(text: string, token: string): ReactNode {
  const parts = text.split(token);
  if (parts.length === 1) return text;
  return parts.map((part, index) => (
    <Fragment key={index}>
      {index > 0 && <code className="font-mono text-caption">{token}</code>}
      {part}
    </Fragment>
  ));
}
