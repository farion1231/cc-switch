import type { FetchedModel } from "@/lib/api/model-fetch";
import type { CopilotModel, CopilotModelOptions } from "@/lib/api/copilot";
import type { CodexCatalogModel } from "@/types";

export const COPILOT_AUTO_MODEL_ID = "copilot-fusion-auto";

export function copilotFetchedModels(
  options: CopilotModelOptions,
  autoDisplayName: string,
): FetchedModel[] {
  return [
    ...(options.autoRoutingSupported
      ? [
          {
            id: COPILOT_AUTO_MODEL_ID,
            ownedBy: autoDisplayName,
            isCopilotAuto: true,
          },
        ]
      : []),
    ...options.models.map((model: CopilotModel) => ({
      id: model.id,
      ownedBy: model.vendor || null,
      billing: model.billing,
    })),
  ];
}

export function copilotAutoCatalogModel(
  displayName: string,
): CodexCatalogModel {
  return {
    model: COPILOT_AUTO_MODEL_ID,
    displayName,
    supportsParallelToolCalls: false,
    inputModalities: ["text"],
  };
}
