import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { MCP_APP_IDS } from "@/config/appConfig";
import { mcpApi, type McpToolsCheck } from "@/lib/api/mcp";
import type { McpServer, McpServersMap } from "@/types";

export type McpConnectionState =
  | "unchecked"
  | "checking"
  | "connected"
  | "authRequired"
  | "disconnected";

type CheckResult = {
  /** The connection config the result was produced for. */
  config: string;
  state: McpConnectionState;
  tools?: McpToolsCheck;
};

const isEnabled = (server: McpServer) =>
  MCP_APP_IDS.some((app) => server.apps[app]);
const configOf = (server: McpServer) => JSON.stringify(server.server);

/**
 * Page-local results, started only by the user: a check runs the configured
 * command (npx downloads, OAuth browser pops, containers), so opening the page
 * must not do it. No polling, focus refresh, retries, or persisted sessions.
 */
export function useMcpConnectivity(servers: McpServersMap | undefined) {
  const [results, setResults] = useState<Record<string, CheckResult>>({});
  // Only the latest request for a server may write its result.
  const generationRef = useRef(0);
  const latestRef = useRef<Record<string, number>>({});
  // Config of each server's latest unfinished check.
  const runningRef = useRef(new Map<string, string>());
  const cancelsRef = useRef(new Set<() => void>());

  const cancelAll = useCallback(() => {
    for (const cancel of cancelsRef.current) cancel();
    cancelsRef.current.clear();
  }, []);

  useEffect(() => cancelAll, [cancelAll]);

  const check = useCallback(
    (ids: string[]) => {
      const configs: Record<string, string> = {};
      for (const id of ids) {
        const server = servers?.[id];
        if (!server || !isEnabled(server)) continue;
        const config = configOf(server);
        // The running check will answer for this config; don't probe it twice.
        if (runningRef.current.get(id) !== config) configs[id] = config;
      }
      const targets = Object.keys(configs);
      if (targets.length === 0) return;

      const generation = ++generationRef.current;
      for (const id of targets) {
        latestRef.current[id] = generation;
        runningRef.current.set(id, configs[id]);
      }
      const pending = new Set(targets);
      const settle = (
        id: string,
        state: McpConnectionState,
        tools?: McpToolsCheck,
      ) => {
        if (latestRef.current[id] !== generation) return;
        if (state !== "checking") runningRef.current.delete(id);
        setResults((previous) => ({
          ...previous,
          [id]: { config: configs[id], state, tools },
        }));
      };
      targets.forEach((id) => settle(id, "checking"));

      let disposed = false;
      let cancel = () => {};
      const finish = () => cancelsRef.current.delete(cancel);
      const batch = mcpApi.startConnectivityChecks(
        targets,
        ({ serverId, status, tools }) => {
          if (disposed || !pending.delete(serverId)) return;
          settle(
            serverId,
            status === "failed" ? "disconnected" : status,
            tools,
          );
          if (pending.size === 0) finish();
        },
      );
      cancel = () => {
        disposed = true;
        // Their results will never arrive, and a replacing batch may skip them.
        pending.forEach((id) => settle(id, "unchecked"));
        // Wait for registration even when cancelling wins the start-command race.
        void batch
          .then((id) => mcpApi.cancelConnectivityChecks(id))
          .catch(() => {});
      };
      cancelsRef.current.add(cancel);
      void batch.catch(() => {
        if (disposed) return;
        finish();
        pending.forEach((id) => settle(id, "disconnected"));
      });
    },
    [servers],
  );

  /** Check every enabled server, replacing any checks still running. */
  const checkAll = useCallback(() => {
    cancelAll();
    check(Object.keys(servers ?? {}));
  }, [cancelAll, check, servers]);

  const checkServer = useCallback((id: string) => check([id]), [check]);

  /**
   * Accept a server's changed tools. Throws when the backend fails; resolves
   * false when nothing was pending (e.g. another window already trusted them).
   */
  const trustTools = useCallback(async (id: string) => {
    const trusted = await mcpApi.trustTools(id);
    setResults((previous) => {
      const result = previous[id];
      if (!result?.tools || result.tools.status !== "changed") return previous;
      return {
        ...previous,
        [id]: {
          ...result,
          tools: {
            ...result.tools,
            status: "unchanged",
            added: [],
            removed: [],
            modified: [],
          },
        },
      };
    });
    return trusted;
  }, []);

  // Metadata, filtering and toggling another app keep a result; editing the
  // connection config makes it unchecked again. Disabled servers have no state.
  const { states, tools } = useMemo(() => {
    const states: Record<string, McpConnectionState> = {};
    const tools: Record<string, McpToolsCheck> = {};
    for (const [id, server] of Object.entries(servers ?? {})) {
      if (!isEnabled(server)) continue;
      const result = results[id];
      const current = result && result.config === configOf(server);
      states[id] = current ? result.state : "unchecked";
      if (current && result.tools) tools[id] = result.tools;
    }
    return { states, tools };
  }, [servers, results]);

  const checking = Object.values(states).includes("checking");
  const canCheck = Object.keys(states).length > 0;

  return {
    states,
    tools,
    checking,
    canCheck,
    checkAll,
    checkServer,
    trustTools,
  };
}
