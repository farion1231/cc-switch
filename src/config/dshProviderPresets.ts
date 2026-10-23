import { piProviderPresets } from "./piProviderPresets";
import type { McodeProviderPreset } from "./mcodeProviderPresets";

/** DSH's pi-ai adapter accepts these three wire protocols and native compatibility options. */
export const dshProviderPresets: McodeProviderPreset[] =
  piProviderPresets.flatMap((preset) => {
    const config = preset.settingsConfig;
    if (
      ![
        "openai-completions",
        "openai-responses",
        "anthropic-messages",
      ].includes(config.api)
    ) {
      return [];
    }
    return [
      {
        ...preset,
        settingsConfig: {
          name: preset.name,
          kind: "custom",
          enabled: true,
          api: config.api,
          ...(config.compat ? { compat: config.compat } : {}),
          options: {
            baseURL: config.baseUrl,
            apiKey: config.apiKey,
            ...(config.headers ? { headers: config.headers } : {}),
          },
          models: Object.fromEntries(
            config.models.map((model) => [
              model.id,
              {
                name: model.name,
                modalities: { input: model.input, output: ["text"] },
                limit: {
                  context: model.contextWindow,
                  output: model.maxTokens,
                },
                ...(model.compat ? { compat: model.compat } : {}),
                ...(model.reasoning
                  ? {
                      reasoning: true,
                      reasoningEfforts: Object.fromEntries(
                        Object.entries(
                          model.thinkingLevelMap ?? {
                            off: null,
                            low: "low",
                            medium: "medium",
                            high: "high",
                          },
                        ).filter(
                          ([, value]) =>
                            !model.thinkingLevelMap || value !== null,
                        ),
                      ),
                    }
                  : { reasoningEfforts: false }),
              },
            ]),
          ),
        },
      },
    ];
  });
