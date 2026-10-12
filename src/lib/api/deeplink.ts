import { invoke } from "@tauri-apps/api/core";

export type ResourceType = "provider" | "prompt" | "mcp" | "skill";

export interface DeepLinkImportRequest {
  version: string;
  resource: ResourceType;

  // Common fields
  app?:
    | "claude"
    | "codex"
    | "gemini"
    | "grokbuild"
    | "opencode"
    | "openclaw"
    | "hermes"
    | "pi";
  name?: string;
  enabled?: boolean;

  // Provider fields
  homepage?: string;
  endpoint?: string;
  apiKey?: string;
  icon?: string;
  /** 图标下载地址；确认框里经 `fetchDeeplinkIcon` 下载、重新编码后放进 `icon` */
  iconUrl?: string;
  model?: string;
  notes?: string;
  haikuModel?: string;
  sonnetModel?: string;
  opusModel?: string;
  /** 接口格式，后端按应用换算成各自的字段 */
  apiFormat?:
    | "anthropic"
    | "openai_chat"
    | "openai_responses"
    | "gemini_native";

  // Prompt fields
  content?: string;
  description?: string;

  // MCP fields
  apps?: string; // Comma-separated application IDs

  // Skill fields
  repo?: string;
  directory?: string;
  branch?: string;

  // Config file fields
  config?: string;
  configFormat?: string;
  configUrl?: string;

  // Usage script fields (v3.9+)
  usageEnabled?: boolean;
  usageScript?: string;
  usageApiKey?: string;
  usageBaseUrl?: string;
  usageAccessToken?: string;
  usageUserId?: string;
  usageAutoInterval?: number;
}

export interface McpImportResult {
  importedCount: number;
  importedIds: string[];
  failed: Array<{
    id: string;
    error: string;
  }>;
}

export type ImportResult =
  | { type: "provider"; id: string }
  | { type: "prompt"; id: string }
  | {
      type: "mcp";
      importedCount: number;
      importedIds: string[];
      failed: Array<{ id: string; error: string }>;
    }
  | { type: "skill"; key: string };

/** 后端暂存、等前端处理的深链接（解析结果或解析错误） */
export type PendingDeepLink =
  | { kind: "import"; request: DeepLinkImportRequest }
  | { kind: "error"; url: string; error: string };

/** 带编号的待处理深链接：编号用于确认清除和去重 */
export interface PendingDeepLinkEntry {
  id: number;
  link: PendingDeepLink;
}

export const deeplinkApi = {
  /**
   * Read the deep link waiting for the frontend; the backend keeps it until acked.
   * Called on mount (cold start / rebuilt window) and on every `deeplink-pending` event.
   */
  peekPendingDeeplink: async (): Promise<PendingDeepLinkEntry | null> => {
    return invoke("peek_pending_deeplink");
  },

  /** Clear the pending deep link after it has been shown; a stale id is ignored */
  ackPendingDeeplink: async (id: number): Promise<void> => {
    return invoke("ack_pending_deeplink", { id });
  },

  /**
   * Parse a deep link URL
   * @param url The ccswitch:// URL to parse
   * @returns Parsed deep link request
   */
  parseDeeplink: async (url: string): Promise<DeepLinkImportRequest> => {
    return invoke("parse_deeplink", { url });
  },

  /**
   * Merge configuration from Base64/URL into a deep link request
   * This is used to show the complete configuration in the confirmation dialog
   * @param request The deep link import request
   * @returns Merged deep link request with config fields populated
   */
  mergeDeeplinkConfig: async (
    request: DeepLinkImportRequest,
  ): Promise<DeepLinkImportRequest> => {
    return invoke("merge_deeplink_config", { request });
  },

  /**
   * 下载链接里 `iconUrl` 指向的图标，返回核对过类型的 data URI；
   * 任何失败（地址不合规、超时、类型不符等）都返回 null，原因只进后端日志
   */
  fetchDeeplinkIcon: async (url: string): Promise<string | null> => {
    return invoke("fetch_deeplink_icon", { url });
  },

  /**
   * Import a resource from a deep link request (unified handler)
   * @param request The deep link import request
   * @returns Import result based on resource type
   */
  importFromDeeplink: async (
    request: DeepLinkImportRequest,
  ): Promise<ImportResult> => {
    return invoke("import_from_deeplink_unified", { request });
  },
};
