import { parse as parseToml, stringify as stringifyToml } from "smol-toml";
import {
  kimiConfigSchema,
  type KimiProviderConfig,
} from "@/config/kimiProviderPresets";
import { normalizeTomlText } from "@/utils/textNormalization";

type Table = Record<string, unknown>;

const isTable = (value: unknown): value is Table =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/**
 * 按 Kimi Code 自己 config.toml 的排版显示这一家：`[providers.<键>]` 加上它的
 * `[models."<模型键>"]`，每个模型表带 `provider = "<键>"`，和写进文件的样子一致。
 */
export function kimiConfigToToml(
  config: KimiProviderConfig,
  key: string,
): string {
  const models = Object.fromEntries(
    Object.entries(config.models).map(([id, model]) => {
      const rest: Table = { ...model };
      delete rest.provider;
      return [id, { provider: key, ...rest }];
    }),
  );
  return `${stringifyToml({ providers: { [key]: config.provider }, models }).trim()}\n`;
}

export type KimiTomlError =
  | { kind: "syntax"; detail: string }
  | { kind: "scope" }
  | { kind: "schema" };

/**
 * 把编辑器里的 TOML 读回 `{ provider, models }`。只接受这一家的表：
 * 供应商键要和上方的「供应商标识」一致，模型表的 `provider` 不能指向别家，
 * 默认模型等全局设置不归这张卡管。
 */
export function kimiConfigFromToml(
  text: string,
  key: string,
):
  | {
      ok: true;
      provider: KimiProviderConfig["provider"];
      models: KimiProviderConfig["models"];
    }
  | { ok: false; error: KimiTomlError } {
  let document: Table;
  try {
    document = parseToml(normalizeTomlText(text));
  } catch (error) {
    return {
      ok: false,
      error: {
        kind: "syntax",
        detail: error instanceof Error ? error.message : String(error),
      },
    };
  }
  const { providers, models = {}, ...extra } = document;
  if (
    Object.keys(extra).length > 0 ||
    !isTable(providers) ||
    Object.keys(providers).length !== 1 ||
    !Object.prototype.hasOwnProperty.call(providers, key) ||
    !isTable(models)
  ) {
    return { ok: false, error: { kind: "scope" } };
  }
  const ownModels: Table = {};
  for (const [id, model] of Object.entries(models)) {
    if (!isTable(model)) return { ok: false, error: { kind: "schema" } };
    const { provider, ...rest } = model;
    if (provider !== undefined && provider !== key) {
      return { ok: false, error: { kind: "scope" } };
    }
    ownModels[id] = rest;
  }
  const parsed = kimiConfigSchema.safeParse({
    provider: providers[key],
    models: ownModels,
  });
  return parsed.success
    ? { ok: true, provider: parsed.data.provider, models: parsed.data.models }
    : { ok: false, error: { kind: "schema" } };
}
