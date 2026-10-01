// 从拉取列表选中模型后，把查到的已知参数换成各应用的字段格式补进这一行。
// 统一规则：只补空着的字段，用户填过的值一律不动；布尔和模态只往「支持」方向补。

import type { KnownModelMetadata } from "@/lib/modelMetadata";
import type { CodexCatalogModel, OpenClawModel, OpenCodeModel } from "@/types";
import type { HermesModel } from "@/config/hermesProviderPresets";

/** 补全前后是否不同：没补上任何字段时不提示用户。 */
export const metadataFilledAnything = (before: unknown, after: unknown) =>
  JSON.stringify(before) !== JSON.stringify(after);

const isBlank = (value: unknown) =>
  value === undefined || value === null || String(value).trim() === "";

/** 只认文字和图片两种输入（Codex / OpenClaw / Pi 的模态字段只有这两种）。 */
function textImageModalities(
  modalities: string[] | undefined,
): string[] | undefined {
  if (!modalities) return undefined;
  return modalities.includes("image") ? ["text", "image"] : ["text"];
}

export function fillCodexCatalogModel<T extends CodexCatalogModel>(
  row: T,
  metadata: KnownModelMetadata,
  knownLevels: readonly string[],
): T {
  const next = { ...row };
  if (isBlank(row.contextWindow) && metadata.contextWindow) {
    next.contextWindow = String(metadata.contextWindow);
  }
  if (!row.reasoningLevels?.length && metadata.reasoningEfforts) {
    // 按 Codex 的档位顺序排，丢掉它不认识的值。
    const levels = knownLevels.filter((level) =>
      metadata.reasoningEfforts?.includes(level),
    );
    if (levels.length > 0) next.reasoningLevels = levels;
  }
  if (!row.inputModalities && metadata.inputModalities) {
    next.inputModalities = textImageModalities(metadata.inputModalities);
  }
  return next;
}

export function fillOpenClawModel(
  model: OpenClawModel,
  metadata: KnownModelMetadata,
): OpenClawModel {
  const next = { ...model };
  if (isBlank(model.contextWindow) && metadata.contextWindow) {
    next.contextWindow = metadata.contextWindow;
  }
  if (isBlank(model.maxTokens) && metadata.maxOutputTokens) {
    next.maxTokens = metadata.maxOutputTokens;
  }
  if (model.reasoning === undefined && metadata.reasoning !== undefined) {
    next.reasoning = metadata.reasoning;
  } else if (metadata.reasoning === true) {
    next.reasoning = true;
  }
  if (!model.input?.length) {
    const input = textImageModalities(metadata.inputModalities);
    if (input) next.input = input;
  } else if (
    metadata.inputModalities?.includes("image") &&
    !model.input.includes("image")
  ) {
    // 新建的行默认只有 text，模型支持图片就补上。
    next.input = [...model.input, "image"];
  }
  if (!model.cost && metadata.cost) {
    next.cost = { ...metadata.cost };
  }
  return next;
}

export function fillHermesModel(
  model: HermesModel,
  metadata: KnownModelMetadata,
): HermesModel {
  return isBlank(model.context_length) && metadata.contextWindow
    ? { ...model, context_length: metadata.contextWindow }
    : model;
}

export function fillOpenCodeModel(
  model: OpenCodeModel,
  metadata: KnownModelMetadata,
): OpenCodeModel {
  const next = { ...model };
  const limit = { ...(model.limit ?? {}) };
  if (isBlank(limit.context) && metadata.contextWindow) {
    limit.context = metadata.contextWindow;
  }
  if (isBlank(limit.output) && metadata.maxOutputTokens) {
    limit.output = metadata.maxOutputTokens;
  }
  if (Object.keys(limit).length > 0) next.limit = limit;
  if (model.modalities === undefined && metadata.inputModalities) {
    next.modalities = {
      input: metadata.inputModalities,
      output: metadata.outputModalities ?? ["text"],
    };
  }
  return next;
}
