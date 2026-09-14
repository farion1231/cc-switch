export interface CodexModelSelection {
  providerId: string;
  model: string;
}

export interface CodexModelRoutingConfig {
  enabled: boolean;
  providerName: string;
  models: CodexModelSelection[];
}

export interface ModelRoutingSaveResult {
  config: CodexModelRoutingConfig;
  catalogChanged: boolean;
}
