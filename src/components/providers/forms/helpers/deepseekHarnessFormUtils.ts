import { DEEPSEEK_HARNESS_DEFAULT_CONFIG } from "@/config/deepseekHarnessProviderPresets";

// ── Default configs ──────────────────────────────────────────────────

export const DSH_MODEL_API_OPTIONS = [
  "openai-completions",
  "openai-responses",
  "anthropic-messages",
] as const;

export type DshModelApi = (typeof DSH_MODEL_API_OPTIONS)[number];

export type DshModel = {
  id: string;
  name?: string;
  contextWindow?: number;
  /** Client-only stable React key; stripped before persisting. */
  rowKey?: string;
};

let dshRowKeySeq = 0;

/** Mirrors the stable per-row keys Pi's provider form uses for model rows. */
export function nextDshRowKey(): string {
  dshRowKeySeq += 1;
  return `dsh-row-${dshRowKeySeq}`;
}

export function withDshRowKeys(models: DshModel[]): DshModel[] {
  return models.map((model) => ({
    ...model,
    rowKey: model.rowKey ?? nextDshRowKey(),
  }));
}

/**
 * Drops blank model rows before persisting. The Add button appends empty
 * rows (mirroring Pi/OpenCode), so a half-filled row must never reach the
 * native `models` catalog where it would break model resolution.
 *
 * Bare-id strings are a valid native catalog form (the backend `model_id_of`
 * accepts them), so they survive the prune — trimmed like object ids so they
 * match the trimmed `meta.dshCurrentModel` comparison on the Rust side.
 */
export function pruneDshModelRows(
  config: Record<string, unknown>,
): Record<string, unknown> {
  if (!Array.isArray(config.models)) return config;
  let changed = false;
  const pruned: unknown[] = [];
  for (const model of config.models) {
    if (typeof model === "string") {
      const id = model.trim();
      if (id === "") {
        changed = true;
        continue;
      }
      if (id !== model) changed = true;
      pruned.push(id);
      continue;
    }
    if (
      model !== null &&
      typeof model === "object" &&
      !Array.isArray(model) &&
      typeof (model as Record<string, unknown>).id === "string" &&
      ((model as Record<string, unknown>).id as string).trim() !== ""
    ) {
      const raw = model as Record<string, unknown>;
      const id = (raw.id as string).trim();
      if (id !== raw.id) {
        changed = true;
        pruned.push({ ...raw, id });
        continue;
      }
      pruned.push(model);
      continue;
    }
    // Nulls, arrays, and objects without a usable id are not model entries.
    changed = true;
  }
  if (!changed) return config;
  return { ...config, models: pruned };
}

export const DSH_OFFICIAL_DEFAULT_CONFIG = DEEPSEEK_HARNESS_DEFAULT_CONFIG;

export const DSH_CUSTOM_DEFAULT_CONFIG: Record<string, unknown> = {
  displayName: "Custom DSH",
  api: "openai-completions",
  baseURL: "",
  apiKeyEnv: "CUSTOM_DSH_API_KEY",
  apiKey: "",
  models: [],
};

// ── Pure functions ───────────────────────────────────────────────────

/**
 * Official DSH routes are identified authoritatively by the persisted provider
 * meta `providerType` (`dsh_deepseek` vs `dsh_pi_ai`), with the fixed provider
 * id and official category as fallbacks. A reserved credential reference is NOT
 * a signal: custom routes may legitimately use `DEEPSEEK_API_KEY`.
 */
export function isDshOfficial(
  id: string | undefined,
  category: string | undefined,
  _config: Record<string, unknown> | undefined,
  providerType?: string | null,
): boolean {
  if (providerType === "dsh_deepseek") return true;
  if (providerType === "dsh_pi_ai") return false;
  if (id === "deepseek-official") return true;
  if (category === "official") return true;
  return false;
}

/**
 * Safely parses a DSH model catalog, tolerating `{ id }` objects and bare
 * id strings from imported native configuration.
 */
export function normalizeDshModels(value: unknown): DshModel[] {
  if (!Array.isArray(value)) return [];

  const models: DshModel[] = [];
  for (const entry of value) {
    if (typeof entry === "string") {
      // Bare-id strings are valid native catalog entries; trim so the editor
      // matches the trimmed default-model comparison on the Rust side.
      const id = entry.trim();
      if (id !== "") models.push({ id });
      continue;
    }
    if (entry && typeof entry === "object" && !Array.isArray(entry)) {
      const item = entry as Record<string, unknown>;
      const id = item.id == null ? "" : String(item.id).trim();
      const name =
        typeof item.name === "string" && item.name ? item.name : undefined;
      const contextWindow =
        typeof item.contextWindow === "number" &&
        Number.isFinite(item.contextWindow)
          ? item.contextWindow
          : undefined;
      models.push({ id, name, contextWindow });
    }
  }
  return models;
}

/** DSH credential reference names must match the backend's env-var pattern. */
export function isValidCredentialRef(value: string): boolean {
  return /^[A-Z_][A-Z0-9_]*$/.test(value);
}
