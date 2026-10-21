import type { ReactNode } from "react";

/** Keep the page instance stable when only its application or settings section changes. */
export function PageFrame({
  children,
  transitionKey: _transitionKey,
}: {
  children: ReactNode;
  transitionKey: string;
}) {
  return <div className="flex min-h-0 flex-1 flex-col">{children}</div>;
}
