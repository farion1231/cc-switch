import { parse as parseToml, stringify as stringifyToml } from "smol-toml";
import { normalizeTomlText } from "@/utils/textNormalization";

type JsonRecord = Record<string, unknown>;

const isRecord = (value: unknown): value is JsonRecord =>
  !!value && typeof value === "object" && !Array.isArray(value);

const pruneEmptyValues = (value: unknown): unknown => {
  if (Array.isArray(value)) {
    const items = value
      .map(pruneEmptyValues)
      .filter((item) => item !== undefined);
    return items.length > 0 ? items : undefined;
  }

  if (!isRecord(value)) {
    return value;
  }

  const entries = Object.entries(value)
    .map(([key, item]) => [key, pruneEmptyValues(item)] as const)
    .filter(([, item]) => item !== undefined);

  if (entries.length === 0) {
    return undefined;
  }

  return Object.fromEntries(entries);
};

export const KIMI_DEFAULT_EDITOR_CONFIG = JSON.stringify(
  {
    config: {
      default_model: "kimi-code/kimi-for-coding",
      default_thinking: true,
      default_permission_mode: "manual",
      default_plan_mode: false,
      merge_all_available_skills: true,
      telemetry: true,
      providers: {
        "managed:kimi-code": {
          type: "kimi",
          api_key: "",
          base_url: "https://api.kimi.com/coding/v1",
          env: {
            KIMI_API_KEY: "",
            KIMI_BASE_URL: "https://api.kimi.com/coding/v1",
          },
          custom_headers: {},
        },
      },
      models: {
        "kimi-code/kimi-for-coding": {
          provider: "managed:kimi-code",
          model: "kimi-for-coding",
          max_context_size: 262144,
          max_output_size: 32000,
          capabilities: ["thinking", "tool_use"],
          display_name: "Kimi For Coding",
        },
      },
      thinking: {
        mode: "auto",
        effort: "high",
      },
      loop_control: {
        max_retries_per_step: 3,
        reserved_context_size: 50000,
      },
      background: {
        max_running_tasks: 4,
        keep_alive_on_exit: false,
      },
      experimental: {
        micro_compaction: true,
      },
    },
  },
  null,
  2,
);

export const formatKimiSettingsForEditor = (settingsConfig: string): string => {
  const raw = settingsConfig.trim();
  if (!raw) {
    return KIMI_DEFAULT_EDITOR_CONFIG;
  }

  try {
    const settings = JSON.parse(raw) as JsonRecord;
    if (!isRecord(settings)) {
      return raw;
    }

    if (typeof settings.config === "string") {
      return JSON.stringify(
        {
          ...settings,
          config: parseToml(normalizeTomlText(settings.config)),
        },
        null,
        2,
      );
    }

    return JSON.stringify(settings, null, 2);
  } catch {
    try {
      return JSON.stringify(
        { config: parseToml(normalizeTomlText(raw)) },
        null,
        2,
      );
    } catch {
      return raw;
    }
  }
};

export const serializeKimiSettingsForBackend = (
  settingsConfig: string,
): string => {
  const settings = JSON.parse(settingsConfig) as JsonRecord;
  if (!isRecord(settings)) {
    throw new Error("Kimi 配置必须是 JSON 对象");
  }

  if (isRecord(settings.config)) {
    const config = pruneEmptyValues(settings.config);
    return JSON.stringify({
      ...settings,
      config: config ? stringifyToml(config as JsonRecord) : "",
    });
  }

  if (typeof settings.config === "string") {
    return JSON.stringify(settings);
  }

  throw new Error("Kimi config 必须是 JSON 对象或 TOML 字符串");
};
