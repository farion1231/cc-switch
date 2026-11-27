import { invoke } from "@tauri-apps/api/core";

export type QoderUpstreamFormat =
  | "openai_responses"
  | "openai_chat"
  | "anthropic";

interface QoderImportSource {
  id: string;
  name: string;
  model: string;
}

interface QoderGatewayStatus {
  enabled: boolean;
  running: boolean;
  baseUrl: string;
  model: string;
  gatewayToken: string;
  importableProviders: QoderImportSource[];
}

export const qoderCnGatewayQueryKey = ["qoderCnDesktop", "gateway"] as const;

export const qoderCnDesktopApi = {
  status: () => invoke<QoderGatewayStatus>("qoder_cn_desktop_status"),
  setGateway: (enabled: boolean) =>
    invoke<void>("qoder_cn_desktop_set_gateway", { enabled }),
  import: (sourceId: string) =>
    invoke<void>("qoder_cn_desktop_import_provider", { sourceId }),
};
