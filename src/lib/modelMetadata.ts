import {
  normalizeModelsDevModelId,
  type ModelsDevCost,
  type ModelsDevModel,
  type ModelsDevResponse,
} from "./modelsDev";

/**
 * 一个模型的已知参数，供表单在用户从拉取列表里选中模型时补全空着的字段。
 * 各应用把它换成自己的字段格式（Codex `contextWindow`、Hermes `context_length`、
 * OpenCode `limit.context`……）。
 */
export interface KnownModelMetadata {
  /** 显示名称（models.dev `name`）；只用来补表单里还没写名字的行。 */
  displayName?: string;
  contextWindow?: number;
  maxInputTokens?: number;
  maxOutputTokens?: number;
  family?: string;
  releaseDate?: string;
  attachment?: boolean;
  temperature?: boolean;
  toolCall?: boolean;
  /** models.dev `interleaved`：思考内容与正文的交错字段。 */
  interleaved?: unknown;
  status?: "alpha" | "beta" | "deprecated" | "active";
  reasoning?: boolean;
  /** 模型接受的思考档位（models.dev `reasoning_options` 里 effort 类的取值）。 */
  reasoningEfforts?: string[];
  /**
   * 同地址 Pi 预设里人工核对过的思考档位映射。映射只在预设的协议和 compat 下
   * 成立（例如 Moonshot 要靠 compat 打开 `reasoning_effort`），三者一起带出。
   */
  piThinking?: {
    thinkingLevelMap: Readonly<Record<string, string | null>>;
    api: string;
    baseUrl: string;
    compat?: Readonly<Record<string, unknown>>;
  };
  inputModalities?: string[];
  outputModalities?: string[];
  /** 美元 / 百万 token。来自网关、同地址预设或同一家供应商；原厂兜底不带价格。 */
  cost?: {
    input: number;
    output: number;
    cacheRead?: number;
    cacheWrite?: number;
  };
  /** models.dev `cost.context_over_200k`：超过 200K 上下文后的价格。 */
  costOver200k?: {
    input: number;
    output: number;
    cacheRead?: number;
    cacheWrite?: number;
  };
}

/** 参数来自哪里：同地址预设，还是 models.dev，还是网关随列表返回的元数据。 */
export type ModelMetadataSource = "preset" | "models-dev" | "gateway";

export interface ResolvedModelMetadata extends KnownModelMetadata {
  /** 实际贡献了字段的来源，按优先级排序。 */
  sources: ModelMetadataSource[];
}

/** 一个预设的地址和它手工核实过的模型参数。 */
export interface PresetModelSource {
  baseUrl: string;
  models: ReadonlyMap<string, KnownModelMetadata>;
}

/**
 * GPT 模型的窗口上限：Codex 官方目录给 GPT-5.5 以后各款的默认窗口。OpenAI API
 * 写的是 1.05M，但很多中转站的 GPT 是从 ChatGPT 订阅分发的，按订阅的窗口卡。
 * 窗口写大了客户端不压缩，直到上游拒收、这一轮失败；写小只是提前压缩。所以
 * 认不出地址、只能借原厂条目的窗口时按订阅的窗口封顶；同地址预设和 models.dev
 * 按地址认出的那家照用它们自己的值，用户手填的不经过这里。
 */
export const GPT_CONTEXT_WINDOW_CAP = 272_000;

/** 是不是 OpenAI 的 GPT 模型（`gpt-6-astra`、`openai/gpt-5.6-sol`、`openai-gpt-6-1-sol`）。 */
export function isGptModelId(modelId: string): boolean {
  return /(?:^|[^a-z0-9])gpt-/.test(normalizeModelsDevModelId(modelId));
}

/** GPT 模型的窗口按 {@link GPT_CONTEXT_WINDOW_CAP} 封顶，本来就小的不动。 */
export function capGptContextWindow(
  modelId: string,
  contextWindow: number,
): number {
  return isGptModelId(modelId)
    ? Math.min(contextWindow, GPT_CONTEXT_WINDOW_CAP)
    : contextWindow;
}

const VERSION_SEGMENT = /^v\d+(?:(?:alpha|beta)\d*)?$/;
const REQUEST_SEGMENTS = new Set(["responses", "messages"]);

/**
 * 同一个接口的新旧地址（归一后的写法）。OpenCode 的 Zen / Go 在 models.dev
 * 里改成了 `/inference/...`（2026-10），预设和用户手里的仍是 `/zen/...`，两者
 * 都能用、计费相同。
 */
const ENDPOINT_ALIASES: Record<string, string> = {
  "opencode.ai/zen": "opencode.ai/inference/openai",
  "opencode.ai/zen/go": "opencode.ai/inference/go/openai",
};

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
  const key = [parsed.hostname.toLowerCase(), ...segments].join("/");
  return ENDPOINT_ALIASES[key] ?? key;
}

/** 两个地址是否指向同一个接口（忽略版本段和请求路径）。 */
export function endpointsMatch(left: string, right: string): boolean {
  const leftKey = endpointKey(left);
  return leftKey !== null && leftKey === endpointKey(right);
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

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function booleanValue(value: unknown): boolean | undefined {
  return typeof value === "boolean" ? value : undefined;
}

function numberValue(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value)
    ? value
    : undefined;
}

function stringValue(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

const MODEL_STATUSES = new Set<NonNullable<KnownModelMetadata["status"]>>([
  "alpha",
  "beta",
  "deprecated",
  "active",
]);

function statusValue(value: unknown): KnownModelMetadata["status"] | undefined {
  return typeof value === "string" &&
    MODEL_STATUSES.has(value as NonNullable<KnownModelMetadata["status"]>)
    ? (value as NonNullable<KnownModelMetadata["status"]>)
    : undefined;
}

function costFromRecord(
  record: Record<string, unknown> | ModelsDevCost | undefined,
): KnownModelMetadata["cost"] | undefined {
  const input = numberValue(record?.input);
  const output = numberValue(record?.output);
  if (input === undefined || output === undefined) return undefined;
  const cacheRead = numberValue(record?.cache_read);
  const cacheWrite = numberValue(record?.cache_write);
  return {
    input,
    output,
    ...(cacheRead !== undefined ? { cacheRead } : {}),
    ...(cacheWrite !== undefined ? { cacheWrite } : {}),
  };
}

/** 只保留 OpenCode / models.dev 认识的交错字段形状。 */
function interleavedValue(value: unknown): unknown {
  return typeof value === "string" ||
    typeof value === "boolean" ||
    asRecord(value) !== undefined
    ? value
    : undefined;
}

/** models.dev `reasoning_options` 里 effort 类的取值；其他类型忽略。 */
function effortValues(value: unknown): string[] | undefined {
  if (!Array.isArray(value)) return undefined;
  return stringList(
    value.flatMap((option) => {
      const record = asRecord(option);
      return record?.type === "effort" && Array.isArray(record.values)
        ? record.values
        : [];
    }),
  );
}

/**
 * 解析网关在 /v1/models 条目里附带的参数。认 `info.meta` 信封：
 * `capabilities.vision` 加上 `models_dev.*`（limit、cost、modalities、
 * reasoning 等）；也接受把 `meta` 直接放在条目上的实现。
 *
 * 这些值是网关按自己的配置算好的，比 cc-switch 按地址猜 models.dev 更准，
 * 所以补全时优先级最高。
 */
export function metadataFromGateway(
  raw: unknown,
): KnownModelMetadata | undefined {
  const envelope = asRecord(raw);
  if (!envelope) return undefined;
  const meta = asRecord(envelope.meta) ?? envelope;
  const dev = asRecord(meta.models_dev) ?? asRecord(meta.modelsDev);
  const capabilities = asRecord(meta.capabilities);
  const limit = asRecord(dev?.limit);
  const modalities = asRecord(dev?.modalities);
  const cost = asRecord(dev?.cost);

  const vision = booleanValue(capabilities?.vision);
  // `vision_source: "default"`（或没写来源）只是网关的保守默认值，不能据此
  // 断言模型只收文字，否则会把图片入口关掉。
  const visionKnown =
    typeof capabilities?.vision_source === "string" &&
    capabilities.vision_source !== "default";
  const metadata: KnownModelMetadata = {
    displayName:
      stringValue(dev?.name) ??
      stringValue(meta.name) ??
      stringValue(meta.display_name),
    contextWindow:
      positiveInteger(limit?.context) ??
      positiveInteger(meta.context_window) ??
      positiveInteger(meta.context_length) ??
      positiveInteger(meta.n_ctx),
    maxInputTokens: positiveInteger(limit?.input),
    maxOutputTokens:
      positiveInteger(limit?.output) ??
      positiveInteger(meta.max_output_tokens) ??
      positiveInteger(meta.max_tokens),
    family: stringValue(dev?.family) ?? stringValue(meta.family),
    releaseDate:
      stringValue(dev?.release_date) ?? stringValue(meta.release_date),
    attachment: booleanValue(dev?.attachment) ?? booleanValue(meta.attachment),
    temperature:
      booleanValue(dev?.temperature) ?? booleanValue(meta.temperature),
    toolCall: booleanValue(dev?.tool_call) ?? booleanValue(meta.tool_call),
    interleaved: interleavedValue(dev?.interleaved ?? meta.interleaved),
    status: statusValue(dev?.status) ?? statusValue(meta.status),
    reasoning: booleanValue(dev?.reasoning) ?? booleanValue(meta.reasoning),
    reasoningEfforts:
      effortValues(dev?.reasoning_options) ??
      effortValues(meta.reasoning_options),
    inputModalities:
      stringList(modalities?.input) ??
      stringList(meta.input_modalities) ??
      (vision === true
        ? ["text", "image"]
        : vision === false && visionKnown
          ? ["text"]
          : undefined),
    outputModalities:
      stringList(modalities?.output) ?? stringList(meta.output_modalities),
    cost: costFromRecord(cost),
    costOver200k: costFromRecord(asRecord(cost?.context_over_200k)),
  };
  return Object.values(metadata).some((value) => value !== undefined)
    ? metadata
    : undefined;
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
  return {
    displayName: stringValue(model.name),
    contextWindow: positiveInteger(model.limit?.context),
    maxInputTokens: positiveInteger(model.limit?.input),
    maxOutputTokens: positiveInteger(model.limit?.output),
    family: stringValue(model.family),
    releaseDate: stringValue(model.release_date),
    attachment: model.attachment,
    temperature: model.temperature,
    toolCall: model.tool_call,
    interleaved: interleavedValue(model.interleaved),
    status: statusValue(model.status),
    reasoning: booleanValue(model.reasoning),
    reasoningEfforts: efforts,
    inputModalities: stringList(model.modalities?.input),
    outputModalities: stringList(model.modalities?.output),
    cost: withCost ? costFromRecord(cost) : undefined,
    costOver200k: withCost
      ? costFromRecord(cost?.context_over_200k)
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
 * 原厂兜底：同名条目指向的原厂条目；指向都落空（没写，或原厂名下没有这一条）
 * 时，才用原厂自己名下的同名条目。指向不止一家原厂时说不清是哪个模型，放弃。
 *
 * 「原厂名下」只是退路：原厂也会托管别家的模型（NVIDIA 名下有
 * `minimaxai/minimax-m2.7`），和指向结果并列就会被当成第二家原厂。
 * 自己指向别家的条目也不算自有：指向的条目缺失时（models.dev 的 deepseek 名下
 * 没有 `deepseek-v4.1-flash-1017`），thinkingmachines 名下指回 deepseek 的 LoRA
 * 微调条目只收文字、窗口 262144，不能冒充原厂资料（#8765）。
 */
function findByVendor(
  index: ModelsDevIndex,
  modelId: string,
): ModelsDevModel | undefined {
  const normalized = normalizeModelsDevModelId(modelId);
  const pointed = new Map<string, ModelsDevModel>();
  const owned = new Map<string, ModelsDevModel>();
  for (const { providerId, model } of index.byNormalizedId.get(normalized) ??
    []) {
    const canonical = model.canonical_model_id;
    if (typeof canonical === "string" && canonical.includes("/")) {
      const vendorId = canonical.slice(0, canonical.indexOf("/"));
      const vendorModel = index.byId
        .get(vendorId)
        ?.models.get(canonical.slice(vendorId.length + 1));
      if (vendorModel) {
        pointed.set(canonical, vendorModel);
        continue;
      }
      if (vendorId !== providerId) continue;
    }
    if (index.vendors.has(providerId)) {
      owned.set(`${providerId}/${model.id ?? normalized}`, model);
    }
  }
  const found = pointed.size > 0 ? pointed : owned;
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
 * 1. 网关随 `/v1/models` 返回的参数（`info.meta`；网关已按自己的配置和
 *    models.dev 算好，最贴近这一家路由的真实值）；
 * 2. 同地址预设里手工核实过的那一行（同一个模型各家部署的窗口差很多，
 *    比如 glm-5.2 从 200K 到 1M 都有，预设写的就是这一家的真实值）；
 * 3. models.dev 里按地址认出的同一家供应商；
 * 4. models.dev 里的原厂条目（不带价格：转售价不等于原厂价；思考档位照用，
 *    中转站绝大多数与原厂一致），
 *    认出了供应商也照样用它补供应商条目里没写的字段。
 * GPT 模型的窗口只在取自原厂条目时按 {@link GPT_CONTEXT_WINDOW_CAP} 封顶。
 * 都查不到返回 null。
 */
export function resolveModelMetadata(
  modelId: string,
  options: {
    baseUrl: string;
    presets?: readonly PresetModelSource[];
    modelsDev?: ModelsDevResponse;
    /** 网关随模型列表返回的参数（见 {@link metadataFromGateway}）。 */
    gatewayMetadata?: KnownModelMetadata;
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

  if (options.gatewayMetadata) {
    add("gateway", options.gatewayMetadata);
  }

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
      const vendor = metadataFromModelsDev(vendorModel, false);
      if (vendor.contextWindow !== undefined) {
        vendor.contextWindow = capGptContextWindow(id, vendor.contextWindow);
      }
      add("models-dev", vendor);
    }
  }

  if (sources.length === 0) return null;
  return { ...metadata, sources };
}
