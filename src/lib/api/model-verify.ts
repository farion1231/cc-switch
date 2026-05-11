import { invoke } from "@tauri-apps/api/core";

export type ModelVerifyProtocol =
  | "openAiChat"
  | "anthropicMessages"
  | "geminiGenerateContent";

export type ProbeStatus = "passed" | "warning" | "failed";

export type EvidenceLevel = "weak" | "medium" | "strong";

export interface ModelVerifyRequest {
  protocol: ModelVerifyProtocol;
  baseUrl: string;
  apiKey: string;
  model: string;
  organization?: string;
  apiVersion?: string;
  timeoutSecs: number;
}

export interface ModelVerifyProbe {
  id: string;
  label: string;
  group: string;
  weight: number;
  status: ProbeStatus;
  latencyMs?: number;
  message: string;
  excerpt?: string;
}

export interface ModelVerifyProbeGroup {
  id: string;
  label: string;
  score: number;
  maxScore: number;
  probes: ModelVerifyProbe[];
}

export interface ModelVerifyScores {
  knowledgeQaScore: number;
  modelFeatureScore: number;
  protocolConsistencyScore: number;
  responseStructureScore: number;
}

export interface ModelVerifyMetrics {
  latencyMs?: number;
  latencySeconds?: number;
  tokensPerSecond?: number;
  inputTokens?: number;
  outputTokens?: number;
  cachedInputTokens?: number;
}

export interface ModelVerifyResult {
  success: boolean;
  testedAt: number;
  modelRequested: string;
  protocol: ModelVerifyProtocol;
  confidenceScore: number;
  mismatchRisk: number;
  overallConfidence: number;
  dilutionRisk: number;
  evidenceLevel: EvidenceLevel;
  scores: ModelVerifyScores;
  metrics: ModelVerifyMetrics;
  summary: string;
  totalLatencyMs?: number;
  probes: ModelVerifyProbe[];
  probeGroups: ModelVerifyProbeGroup[];
  diagnostics: ModelVerifyProbe[];
}

export async function verifyModelAuthenticity(
  request: ModelVerifyRequest,
): Promise<ModelVerifyResult> {
  return invoke("verify_model_authenticity", { request });
}
