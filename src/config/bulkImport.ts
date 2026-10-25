import type { AppId } from "@/lib/api/types";

const all = ["anthropic", "openai_chat", "openai_responses", "gemini_native"];
const three = ["anthropic", "openai_chat", "openai_responses"];
const gemini = ["gemini_native"];
// Mirrors bulk_import/formats.rs; model declarations are required for coexist apps.
export const BULK_IMPORT_FORMATS: Record<
  AppId,
  { defaultFormat: string; formats: string[]; modelRequired: boolean }
> = {
  claude: { defaultFormat: "anthropic", formats: all, modelRequired: false },
  "claude-desktop": {
    defaultFormat: "anthropic",
    formats: all,
    modelRequired: false,
  },
  codex: {
    defaultFormat: "openai_responses",
    formats: three,
    modelRequired: false,
  },
  gemini: {
    defaultFormat: "gemini_native",
    formats: gemini,
    modelRequired: false,
  },
  grokbuild: {
    defaultFormat: "openai_responses",
    formats: three,
    modelRequired: false,
  },
  agy: {
    defaultFormat: "gemini_native",
    formats: gemini,
    modelRequired: false,
  },
  opencode: { defaultFormat: "openai_chat", formats: all, modelRequired: true },
  openclaw: { defaultFormat: "openai_chat", formats: all, modelRequired: true },
  hermes: { defaultFormat: "openai_chat", formats: three, modelRequired: true },
  pi: { defaultFormat: "openai_chat", formats: all, modelRequired: true },
  omp: { defaultFormat: "openai_chat", formats: all, modelRequired: true },
  mcode: { defaultFormat: "anthropic", formats: three, modelRequired: true },
  kimi: { defaultFormat: "openai_chat", formats: all, modelRequired: true },
};
