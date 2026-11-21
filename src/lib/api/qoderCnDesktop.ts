import { invoke } from "@tauri-apps/api/core";

export type QoderUpstreamFormat =
  | "openai_responses"
  | "openai_chat"
  | "anthropic";

interface QoderProvider {
  id: string;
  name: string;
  baseUrl: string;
  model: string;
  format: QoderUpstreamFormat;
  anthropicApiKey: boolean;
}

export type QoderProviderInput = Omit<QoderProvider, "id"> & {
  id?: string;
  apiKey: string;
};

interface QoderGatewayStatus {
  enabled: boolean;
  running: boolean;
  baseUrl: string;
  model: string;
  gatewayToken: string;
  currentProviderId: string | null;
  providers: QoderProvider[];
  importableProviders: QoderProvider[];
}

export const qoderCnDesktopApi = {
  status: () => invoke<QoderGatewayStatus>("qoder_cn_desktop_status"),
  setGateway: (enabled: boolean) =>
    invoke<void>("qoder_cn_desktop_set_gateway", { enabled }),
  save: (input: QoderProviderInput) =>
    invoke<void>("qoder_cn_desktop_save_provider", { input }),
  import: (sourceId: string) =>
    invoke<void>("qoder_cn_desktop_import_provider", { sourceId }),
  select: (id: string) =>
    invoke<void>("qoder_cn_desktop_select_provider", { id }),
  delete: (id: string) =>
    invoke<void>("qoder_cn_desktop_delete_provider", { id }),
};
