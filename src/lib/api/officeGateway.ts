import { invoke } from "@tauri-apps/api/core";

export type OfficeGatewayProviderKind =
  | "auto"
  | "deep_seek"
  | "kimi"
  | "mimo"
  | "mini_max";

export interface OfficeGatewayConfig {
  listenHost: string;
  listenPort: number;
  activeProvider: OfficeGatewayProviderKind;
  defaultMaxTokens: number;
  minCompatMaxTokens: number;
  passthroughMetadata: boolean;
  enableWebSearchTool: boolean;
  modelPrimary: string;
  modelMid: string;
  modelFast: string;
  deepseekApiKey: string;
  deepseekBaseUrl: string;
  deepseekModelPrimary: string;
  deepseekModelMid: string;
  deepseekModelFast: string;
  kimiApiKey: string;
  kimiPaygBaseUrl: string;
  kimiCodingBaseUrl: string;
  kimiCodingModel: string;
  kimiModelPrimary: string;
  kimiModelMid: string;
  kimiModelFast: string;
  mimoApiKey: string;
  mimoPaygBaseUrl: string;
  mimoTpRegion: string;
  mimoTpBaseUrlCn: string;
  mimoTpBaseUrlSgp: string;
  mimoTpBaseUrlAms: string;
  mimoModelPrimary: string;
  mimoModelMid: string;
  mimoModelFast: string;
  minimaxApiKey: string;
  minimaxRegion: string;
  minimaxBaseUrlCn: string;
  minimaxBaseUrlGlobal: string;
  minimaxModelPrimary: string;
  minimaxModelMid: string;
  minimaxModelFast: string;
}

export interface OfficeGatewayStatus {
  running: boolean;
  host: string;
  port: number;
  baseUrl: string;
  activeProvider: OfficeGatewayProviderKind;
  logFile: string;
  startedAt: string | null;
}

export interface OfficeGatewayLogEntry {
  ts: string;
  level: string;
  category: string;
  message: string;
}

export interface OfficeGatewayLogSnapshot {
  entries: OfficeGatewayLogEntry[];
  logFile: string;
}

export interface OfficeGatewayUpstreamTestResult {
  ok: boolean;
  provider: OfficeGatewayProviderKind;
  routeKind: string;
  upstreamUrl: string;
  model: string;
  status: number;
  message: string;
  bodyPreview: string;
}

export const officeGatewayApi = {
  getConfig: () =>
    invoke<OfficeGatewayConfig>("get_office_gateway_config"),
  saveConfig: (config: OfficeGatewayConfig) =>
    invoke<OfficeGatewayConfig>("save_office_gateway_config", { config }),
  start: () => invoke<OfficeGatewayStatus>("start_office_gateway"),
  stop: () => invoke<void>("stop_office_gateway"),
  restart: () => invoke<OfficeGatewayStatus>("restart_office_gateway"),
  getStatus: () => invoke<OfficeGatewayStatus>("get_office_gateway_status"),
  getLogs: () =>
    invoke<OfficeGatewayLogSnapshot>("get_office_gateway_logs"),
  testUpstream: () =>
    invoke<OfficeGatewayUpstreamTestResult>("test_office_gateway_upstream"),
  clearLogs: () => invoke<void>("clear_office_gateway_logs"),
  openLogFile: () => invoke<void>("open_office_gateway_log_file"),
};
