import type { AppCountBarFilter } from "@/components/common/AppCountBar";
import type { McpServer } from "@/types";

export type McpServerEntry = [string, McpServer];

export function filterMcpServers(
  entries: McpServerEntry[],
  filter: AppCountBarFilter,
): McpServerEntry[] {
  if (filter === "all") return entries;

  return entries.filter(([, server]) => server.apps[filter] === true);
}
