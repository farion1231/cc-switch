import { invoke } from "@tauri-apps/api/core";
import type { SwitchResult } from "./providers";

export interface CodexSelection {
  revision: string;
  selected: string[];
  model: string | null;
  effort: string | null;
  exitToDirect?: boolean;
}
export interface CodexSwitchPreview {
  revision: string;
  proxy: boolean;
  native?: boolean;
  routing?: boolean;
  exitToDirect?: boolean;
  fields: {
    id: string;
    label: string;
    before: string;
    after: string;
    selected: boolean;
  }[];
  models: { model: string; reasoningLevels: string[] }[];
  currentModels: { model: string; reasoningLevels: string[] }[];
  defaultModel: string | null;
  currentModel: string | null;
  currentEffort: string | null;
}
export const codexSelectionApi = {
  preview: (id: string, exitToDirect = false) =>
    invoke<CodexSwitchPreview>("preview_codex_switch", { id, exitToDirect }),
  apply: (id: string, selection: CodexSelection) =>
    invoke<SwitchResult>("apply_codex_switch", { id, selection }),
};
