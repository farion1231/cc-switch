import type {
  OpenClawDefaultModel,
  OpenClawProviderConfig,
  Provider,
} from "@/types";

export function buildOpenClawDefaultModel(
  provider: Provider,
): OpenClawDefaultModel | null {
  const config = provider.settingsConfig as OpenClawProviderConfig;
  const models = config.models ?? [];
  if (!models[0]?.id) return null;

  return {
    primary: `${provider.id}/${models[0].id}`,
    fallbacks: models.slice(1).map((model) => `${provider.id}/${model.id}`),
  };
}

export function matchesOpenClawPrimaryModel(
  current: OpenClawDefaultModel | null | undefined,
  provider: Provider,
): boolean {
  if (!current?.primary) return false;

  const config = provider.settingsConfig as OpenClawProviderConfig;
  const models = config.models ?? [];

  return models.some(
    (model) =>
      Boolean(model.id) && current.primary === `${provider.id}/${model.id}`,
  );
}
