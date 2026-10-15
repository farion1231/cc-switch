import { useEffect, useState } from "react";
import { MCP_APP_IDS } from "@/config/appConfig";
import { mcpApi } from "@/lib/api/mcp";
import type { McpServersMap } from "@/types";

export type McpConnectionState = "checking" | "connected" | "disconnected";

/** Page-local results: no polling, focus refresh, retries, or persisted sessions. */
export function useMcpConnectivity(servers: McpServersMap | undefined) {
  // Metadata, filtering and toggling an additional app do not reconnect servers.
  const configKey = JSON.stringify(
    Object.entries(servers ?? {})
      .filter(([, server]) => MCP_APP_IDS.some((app) => server.apps[app]))
      .map(([id, server]) => [id, server.server]),
  );
  const [results, setResults] = useState<{
    configKey: string;
    states: Record<string, McpConnectionState>;
  }>({ configKey: "", states: {} });

  useEffect(() => {
    const entries = JSON.parse(configKey) as [string, unknown][];
    if (entries.length === 0) return;
    let disposed = false;
    setResults({ configKey, states: {} });
    const batch = mcpApi.startConnectivityChecks(
      entries.map(([id]) => id),
      ({ serverId, connected }) => {
        if (disposed) return;
        setResults((previous) => ({
          configKey,
          states: {
            ...previous.states,
            [serverId]: connected ? "connected" : "disconnected",
          },
        }));
      },
    );
    void batch.catch(() => {
      if (disposed) return;
      setResults({
        configKey,
        states: Object.fromEntries(entries.map(([id]) => [id, "disconnected"])),
      });
    });
    return () => {
      disposed = true;
      // Wait for registration even when navigation wins the start-command race.
      void batch
        .then((id) => mcpApi.cancelConnectivityChecks(id))
        .catch(() => {});
    };
  }, [configKey]);

  return results.configKey === configKey ? results.states : {};
}
