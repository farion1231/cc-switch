import { invoke } from "@tauri-apps/api/core";

export interface StrategyRule {
  name: string;
  description: string;
  type: "route" | "cascade" | "debate" | "moa";
  complexityRange: [number, number];
  riskLevels: string[];
  models: string[];
  qualityThreshold: number;
  judge?: string;
  aggregator?: string;
}

export interface ModelDef {
  provider: string;
  model: string;
  api_key_env: string;
  base_url?: string | null;
  max_tokens: number;
}

export interface OrchestrationConfig {
  enabled: boolean;
  models: Record<string, ModelDef>;
  strategies: Record<
    string,
    {
      description: string;
      when: {
        complexity?: [number, number] | null;
        risk?: string[] | null;
        task_type?: string[] | null;
        has_image?: boolean | null;
      };
      action:
        | { type: "route"; use_model: string; verify?: boolean }
        | {
            type: "cascade";
            models: string[];
            verify_each?: boolean;
            escalate_on_fail?: boolean;
            quality_threshold?: number;
          }
        | {
            type: "debate";
            debaters: string[];
            judge: string;
            quality_threshold?: number;
          }
        | {
            type: "moa";
            proposers: string[];
            aggregator: string;
            verify_each?: boolean;
            quality_threshold?: number;
          };
    }
  >;
}

export function configToStrategyRules(config: OrchestrationConfig): StrategyRule[] {
  return Object.entries(config.strategies).map(
    ([name, def]): StrategyRule => {
      const action = def.action;
      const type = action.type;
      const models =
        type === "route"
          ? [(action as { use_model: string }).use_model]
          : type === "debate"
            ? (action as { debaters: string[] }).debaters
            : type === "moa"
              ? (action as { proposers: string[] }).proposers
              : (action as { models: string[] }).models;
      const qualityThreshold =
        type !== "route"
          ? (action as { quality_threshold?: number }).quality_threshold ?? 0.65
          : 0.65;

      return {
        name,
        description: def.description ?? "",
        type,
        complexityRange: def.when?.complexity ?? [0, 1],
        riskLevels: def.when?.risk ?? [],
        models,
        qualityThreshold,
        judge:
          type === "debate"
            ? (action as { judge: string }).judge
            : undefined,
        aggregator:
          type === "moa"
            ? (action as { aggregator: string }).aggregator
            : undefined,
      };
    },
  );
}

export async function getConfig(): Promise<OrchestrationConfig> {
  return invoke<OrchestrationConfig>("get_strategies_config");
}

export async function saveConfig(config: OrchestrationConfig): Promise<void> {
  return invoke("save_strategies_config", { configJson: config });
}

export async function getConfigPath(): Promise<string> {
  return invoke<string>("get_strategies_config_path");
}
