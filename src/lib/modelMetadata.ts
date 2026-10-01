import {
  normalizeModelsDevModelId,
  type ModelsDevModel,
  type ModelsDevResponse,
} from "./modelsDev";

/**
 * 一个模型的已知参数，供表单在用户从拉取列表里选中模型时补全空着的字段。
 * 各应用把它换成自己的字段格式（Codex `contextWindow`、Hermes `context_length`、
 * OpenCode `limit.context`……）。
 */
export interface KnownModelMetadata {
  contextWindow?: number;
  maxOutputTokens?: number;
  reasoning?: boolean;
  /** 模型接受的思考档位（models.dev `reasoning_options` 里 effort 类的取值）。 */
  reasoningEfforts?: string[];
  inputModalities?: string[];
  outputModalities?: string[];
  /** 美元 / 百万 token。只取自同一家供应商，原厂兜底不带价格。 */
  cost?: {
    input: number;
    output: number;
    cacheRead?: number;
    cacheWrite?: number;
  };
}

/** 参数来自哪里：同地址预设，还是 models.dev。补全提示里告诉用户。 */
export type ModelMetadataSource = "preset" | "models-dev";

export interface ResolvedModelMetadata extends KnownModelMetadata {
  /** 实际贡献了字段的来源，按优先级排序。 */
  sources: ModelMetadataSource[];
}

/** 一个预设的地址和它手工核实过的模型参数。 */
export interface PresetModelSource {
  baseUrl: string;
  models: ReadonlyMap<string, KnownModelMetadata>;
}

const VERSION_SEGMENT = /^v\d+(?:(?:alpha|beta)\d*)?$/;
const REQUEST_SEGMENTS = new Set(["responses", "messages"]);

/**
 * 把地址归一成「域名 + 路径」，去掉末尾的版本号（`v1`、`v4`、`v1beta`）和请求
 * 路径（`chat/completions`、`responses`、`messages`）：这些写不写都是同一个接口。
 * 其余路径必须完全一致才算同一个接口——`opencode.ai/zen/v1` 和 `/zen/go/v1`、
 * `api.z.ai/api/paas/v4` 和 `/api/coding/paas/v4` 都是同域名下的不同套餐，
 * 窗口和价格不同，不能因为一方是另一方的上级路径就混用。
 */
function endpointKey(url: string): string | null {
  const trimmed = url.trim();
  if (!trimmed) return null;
  let parsed: URL;
  try {
    parsed = new URL(
      /^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed) ? trimmed : `https://${trimmed}`,
    );
  } catch {
    return null;
  }
  const segments = parsed.pathname
    .split("/")
    .filter(Boolean)
    .map((segment) => segment.toLowerCase());
  for (;;) {
    const last = segments[segments.length - 1];
    if (last === "completions" && segments[segments.length - 2] === "chat") {
      segments.length -= 2;
    } else if (REQUEST_SEGMENTS.has(last) || VERSION_SEGMENT.test(last)) {
      segments.length -= 1;
    } else {
      break;
    }
  }
  return [parsed.hostname.toLowerCase(), ...segments].join("/");
}

/** 挑出与地址是同一个接口的候选，保持原有顺序。 */
function matchEndpoint<T>(
  baseUrl: string,
  candidates: readonly T[],
  endpointOf: (candidate: T) => string | null,
): T[] {
  const target = endpointKey(baseUrl);
  if (!target) return [];
  return candidates.filter((candidate) => endpointOf(candidate) === target);
}

function findModelInMap<T>(
  models: ReadonlyMap<string, T>,
  modelId: string,
): T | undefined {
  const exact = models.get(modelId);
  if (exact !== undefined) return exact;
  const lower = modelId.toLowerCase();
  const normalized = normalizeModelsDevModelId(modelId);
  let normalizedMatch: T | undefined;
  for (const [id, value] of models) {
    if (id.toLowerCase() === lower) return value;
    if (
      normalizedMatch === undefined &&
      normalizeModelsDevModelId(id) === normalized
    ) {
      normalizedMatch = value;
    }
  }
  return normalizedMatch;
}

interface IndexedProvider {
  id: string;
  endpoint: string | null;
  models: Map<string, ModelsDevModel>;
}

interface ModelsDevIndex {
  providers: IndexedProvider[];
  byId: Map<string, IndexedProvider>;
  /** 规范化 ID → 各家同名条目，给原厂兜底用。 */
  byNormalizedId: Map<string, { providerId: string; model: ModelsDevModel }[]>;
  /** 被 `canonical_model_id` 指向过的供应商，即原厂。 */
  vendors: Set<string>;
}

const indexCache = new WeakMap<ModelsDevResponse, ModelsDevIndex>();

function indexModelsDev(data: ModelsDevResponse): ModelsDevIndex {
  const cached = indexCache.get(data);
  if (cached) return cached;

  const providers: IndexedProvider[] = [];
  const byId = new Map<string, IndexedProvider>();
  const byNormalizedId = new Map<
    string,
    { providerId: string; model: ModelsDevModel }[]
  >();
  const vendors = new Set<string>();

  for (const [providerId, provider] of Object.entries(data)) {
    if (!provider || typeof provider !== "object") continue;
    const models = new Map<string, ModelsDevModel>();
    for (const [modelId, model] of Object.entries(provider.models ?? {})) {
      if (!model || typeof model !== "object") continue;
      models.set(modelId, model);
      const normalized = normalizeModelsDevModelId(modelId);
      if (normalized) {
        const list = byNormalizedId.get(normalized) ?? [];
        list.push({ providerId, model });
        byNormalizedId.set(normalized, list);
      }
      const canonical = model.canonical_model_id;
      if (typeof canonical === "string" && canonical.includes("/")) {
        vendors.add(canonical.slice(0, canonical.indexOf("/")));
      }
    }
    const indexed: IndexedProvider = {
      id: providerId,
      endpoint:
        typeof provider.api === "string" ? endpointKey(provider.api) : null,
      models,
    };
    providers.push(indexed);
    byId.set(providerId, indexed);
  }

  const index = { providers, byId, byNormalizedId, vendors };
  indexCache.set(data, index);
  return index;
}

/** 正整数（接受数字字符串，表单里的窗口常存成字符串）；其余返回 undefined。 */
export function positiveInteger(value: unknown): number | undefined {
  const parsed =
    typeof value === "string" && value.trim() ? Number(value) : value;
  return typeof parsed === "number" && Number.isFinite(parsed) && parsed > 0
    ? Math.trunc(parsed)
    : undefined;
}

/** 去空、小写、去重后的字符串列表；空列表返回 undefined。 */
export function stringList(value: unknown): string[] | undefined {
  if (!Array.isArray(value)) return undefined;
  const list = value
    .filter((item): item is string => typeof item === "string")
    .map((item) => item.trim().toLowerCase())
    .filter(Boolean);
  return list.length > 0 ? [...new Set(list)] : undefined;
}

function metadataFromModelsDev(
  model: ModelsDevModel,
  withCost: boolean,
): KnownModelMetadata {
  const efforts = stringList(
    (model.reasoning_options ?? [])
      .filter((option) => option?.type === "effort")
      .flatMap((option) => option.values ?? []),
  );
  const cost = model.cost;
  const hasCost =
    withCost &&
    typeof cost?.input === "number" &&
    typeof cost?.output === "number";
  return {
    contextWindow: positiveInteger(model.limit?.context),
    maxOutputTokens: positiveInteger(model.limit?.output),
    reasoning:
      typeof model.reasoning === "boolean" ? model.reasoning : undefined,
    reasoningEfforts: efforts,
    inputModalities: stringList(model.modalities?.input),
    outputModalities: stringList(model.modalities?.output),
    cost: hasCost
      ? {
          input: cost.input as number,
          output: cost.output as number,
          ...(typeof cost.cache_read === "number"
            ? { cacheRead: cost.cache_read }
            : {}),
          ...(typeof cost.cache_write === "number"
            ? { cacheWrite: cost.cache_write }
            : {}),
        }
      : undefined,
  };
}

/** 按地址认出 models.dev 里的那家供应商，再查它名下的这个模型。 */
function findByProvider(
  index: ModelsDevIndex,
  baseUrl: string,
  modelId: string,
): ModelsDevModel | undefined {
  for (const provider of matchEndpoint(
    baseUrl,
    index.providers,
    (p) => p.endpoint,
  )) {
    const model = findModelInMap(provider.models, modelId);
    if (model) return model;
  }
  return undefined;
}

/**
 * 原厂兜底：同名条目指向的原厂条目，或者原厂自己名下的同名条目。
 * 指向不止一家原厂时说不清是哪个模型，放弃。
 */
function findByVendor(
  index: ModelsDevIndex,
  modelId: string,
): ModelsDevModel | undefined {
  const normalized = normalizeModelsDevModelId(modelId);
  const found = new Map<string, ModelsDevModel>();
  for (const { providerId, model } of index.byNormalizedId.get(normalized) ??
    []) {
    const canonical = model.canonical_model_id;
    if (typeof canonical === "string" && canonical.includes("/")) {
      const vendorId = canonical.slice(0, canonical.indexOf("/"));
      const vendorModel = index.byId
        .get(vendorId)
        ?.models.get(canonical.slice(vendorId.length + 1));
      if (vendorModel) {
        found.set(canonical, vendorModel);
        continue;
      }
    }
    if (index.vendors.has(providerId)) {
      found.set(`${providerId}/${model.id ?? normalized}`, model);
    }
  }
  return found.size === 1 ? found.values().next().value : undefined;
}

function mergeMissing(
  primary: KnownModelMetadata,
  fallback: KnownModelMetadata,
): KnownModelMetadata {
  const merged: KnownModelMetadata = { ...primary };
  for (const [key, value] of Object.entries(fallback) as [
    keyof KnownModelMetadata,
    KnownModelMetadata[keyof KnownModelMetadata],
  ][]) {
    if (merged[key] === undefined && value !== undefined) {
      (merged as Record<string, unknown>)[key] = value;
    }
  }
  return merged;
}

function fieldCount(metadata: KnownModelMetadata): number {
  return Object.values(metadata).filter((value) => value !== undefined).length;
}

/**
 * 查一个模型的已知参数。逐字段按优先级取值：
 * 1. 同地址预设里手工核实过的那一行（同一个模型各家部署的窗口差很多，
 *    比如 glm-5.2 从 200K 到 1M 都有，预设写的就是这一家的真实值）；
 * 2. models.dev 里按地址认出的同一家供应商；
 * 3. models.dev 里的原厂条目（不带价格：转售价不等于原厂价），
 *    认出了供应商也照样用它补供应商条目里没写的字段。
 * 都查不到返回 null。
 */
export function resolveModelMetadata(
  modelId: string,
  options: {
    baseUrl: string;
    presets?: readonly PresetModelSource[];
    modelsDev?: ModelsDevResponse;
  },
): ResolvedModelMetadata | null {
  const id = modelId.trim();
  if (!id) return null;

  let metadata: KnownModelMetadata = {};
  const sources: ModelMetadataSource[] = [];
  const add = (source: ModelMetadataSource, found: KnownModelMetadata) => {
    const before = fieldCount(metadata);
    metadata = mergeMissing(metadata, found);
    if (fieldCount(metadata) > before && !sources.includes(source)) {
      sources.push(source);
    }
  };

  for (const preset of matchEndpoint(
    options.baseUrl,
    options.presets ?? [],
    (source) => endpointKey(source.baseUrl),
  )) {
    const presetModel = findModelInMap(preset.models, id);
    if (presetModel) {
      add("preset", presetModel);
      break;
    }
  }

  if (options.modelsDev) {
    const index = indexModelsDev(options.modelsDev);
    const providerModel = findByProvider(index, options.baseUrl, id);
    if (providerModel) {
      add("models-dev", metadataFromModelsDev(providerModel, true));
    }
    const vendorModel = findByVendor(index, id);
    if (vendorModel) {
      add("models-dev", metadataFromModelsDev(vendorModel, false));
    }
  }

  return sources.length > 0 ? { ...metadata, sources } : null;
}
