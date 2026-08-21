import { extractErrorMessage } from "@/utils/errorUtils";

export const COPILOT_ERROR_KEYS = {
  load: "copilotByok.errors.load",
  targets: "copilotByok.errors.targets",
  addTarget: "copilotByok.errors.addTarget",
  removeTarget: "copilotByok.errors.removeTarget",
  save: "copilotByok.errors.save",
  stateChange: "copilotByok.errors.stateChange",
  apply: "copilotByok.errors.apply",
  preview: "copilotByok.errors.preview",
  delete: "copilotByok.errors.delete",
  duplicate: "copilotByok.errors.duplicate",
  import: "copilotByok.errors.import",
  sync: "copilotByok.errors.sync",
  stop: "copilotByok.errors.stop",
  restore: "copilotByok.errors.restore",
  connection: "copilotByok.errors.connection",
  invalidModelOptions: "copilotByok.errors.invalidModelOptions",
  duplicateModelId: "copilotByok.errors.duplicateModelId",
} as const;

export type CopilotErrorOperation = keyof typeof COPILOT_ERROR_KEYS;
export type CopilotErrorKey =
  | (typeof COPILOT_ERROR_KEYS)[CopilotErrorOperation]
  | "copilotByok.cli.previewStale";

export interface CopilotUiError {
  messageKey: CopilotErrorKey;
  details?: string;
}

export function copilotUiError(
  operation: CopilotErrorOperation,
  error: unknown,
): CopilotUiError {
  return {
    messageKey: COPILOT_ERROR_KEYS[operation],
    details: extractErrorMessage(error) || undefined,
  };
}

export class CopilotFormValidationError extends Error {
  constructor(
    readonly messageKey:
      | typeof COPILOT_ERROR_KEYS.invalidModelOptions
      | typeof COPILOT_ERROR_KEYS.duplicateModelId,
  ) {
    super(messageKey);
    this.name = "CopilotFormValidationError";
  }
}
