import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Check, ChevronDown, RefreshCw } from "lucide-react";
import { cn } from "@/lib/utils";
import { providersApi, type RemoteModelInfo } from "@/lib/api/providers";
import type { ProviderProxyConfig } from "@/types";

interface RemoteModelSelectorProps {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  baseUrl: string;
  apiKey: string;
  apiFormat: "anthropic" | "openai_chat" | "openai_responses";
  proxyConfig?: ProviderProxyConfig;
  placeholder?: string;
  hint?: string;
  className?: string;
}

const REMOTE_MODELS_CACHE_TTL_MS = 5 * 60 * 1000;
const remoteModelsCache = new Map<
  string,
  { expiresAt: number; models: RemoteModelInfo[] }
>();
const remoteModelsInflight = new Map<string, Promise<RemoteModelInfo[]>>();

function normalizeBaseUrl(baseUrl: string): string {
  return baseUrl.trim().replace(/\/+$/, "");
}

function apiKeyFingerprint(apiKey: string): string {
  let hash = 0;
  for (let i = 0; i < apiKey.length; i += 1) {
    hash = (hash * 31 + apiKey.charCodeAt(i)) >>> 0;
  }
  return hash.toString(16);
}

function normalizeProviderName(model: RemoteModelInfo): string | undefined {
  const provider =
    typeof model.provider === "string" ? model.provider.trim() : "";
  return provider || undefined;
}

function normalizeRemoteModels(models: RemoteModelInfo[]): RemoteModelInfo[] {
  const seen = new Set<string>();
  const normalized: RemoteModelInfo[] = [];

  for (const model of models) {
    const id = typeof model.id === "string" ? model.id.trim() : "";
    if (!id || seen.has(id)) continue;
    seen.add(id);
    normalized.push({
      id,
      provider:
        typeof model.provider === "string" ? model.provider.trim() : undefined,
      displayName:
        typeof model.displayName === "string"
          ? model.displayName.trim()
          : undefined,
    });
  }

  return normalized.sort((a, b) =>
    a.id.localeCompare(b.id, "en", { numeric: true }),
  );
}

export function RemoteModelSelector({
  id,
  label,
  value,
  onChange,
  baseUrl,
  apiKey,
  apiFormat,
  proxyConfig,
  placeholder,
  hint,
  className,
}: RemoteModelSelectorProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const [remoteModels, setRemoteModels] = useState<RemoteModelInfo[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [hasLoaded, setHasLoaded] = useState(false);

  const canEnumerate = useMemo(() => {
    return normalizeBaseUrl(baseUrl).length > 0 && apiKey.trim().length > 0;
  }, [baseUrl, apiKey]);

  const cacheKey = useMemo(() => {
    const normalizedBaseUrl = normalizeBaseUrl(baseUrl);
    if (!normalizedBaseUrl || !apiKey.trim()) return "";

    const proxyFingerprint = proxyConfig?.enabled
      ? `${proxyConfig.proxyType ?? "http"}:${proxyConfig.proxyHost ?? ""}:${proxyConfig.proxyPort ?? ""}`
      : "none";

    return `${apiFormat}|${normalizedBaseUrl}|${apiKeyFingerprint(apiKey.trim())}|${proxyFingerprint}`;
  }, [baseUrl, apiFormat, apiKey, proxyConfig]);

  useEffect(() => {
    if (!canEnumerate || !cacheKey) {
      setRemoteModels([]);
      setHasLoaded(false);
      return;
    }

    const cached = remoteModelsCache.get(cacheKey);
    if (cached && cached.expiresAt > Date.now()) {
      setRemoteModels(cached.models);
      setHasLoaded(true);
      return;
    }

    setRemoteModels([]);
    setHasLoaded(false);
  }, [canEnumerate, cacheKey]);

  const loadModels = useCallback(
    async (forceRefresh = false) => {
      if (!canEnumerate || !cacheKey) return;

      if (!forceRefresh) {
        const cached = remoteModelsCache.get(cacheKey);
        if (cached && cached.expiresAt > Date.now()) {
          setRemoteModels(cached.models);
          setHasLoaded(true);
          return;
        }
      }

      setIsLoading(true);

      let request = !forceRefresh ? remoteModelsInflight.get(cacheKey) : undefined;
      if (!request) {
        request = providersApi
          .enumerateModels({
            baseUrl: normalizeBaseUrl(baseUrl),
            apiKey: apiKey.trim(),
            apiFormat,
            proxyConfig: proxyConfig?.enabled ? proxyConfig : undefined,
            forceRefresh,
          })
          .then((models) => normalizeRemoteModels(models));
        remoteModelsInflight.set(cacheKey, request);
      }

      try {
        const models = await request;
        remoteModelsCache.set(cacheKey, {
          expiresAt: Date.now() + REMOTE_MODELS_CACHE_TTL_MS,
          models,
        });
        setRemoteModels(models);
        setHasLoaded(true);
      } catch (error) {
        if (forceRefresh) {
          const message = error instanceof Error ? error.message : String(error);
          toast.error(
            t("providerForm.fetchModelsFailed", {
              defaultValue: "Failed to fetch models: {{error}}",
              error: message,
            }),
          );
        }
      } finally {
        if (remoteModelsInflight.get(cacheKey) === request) {
          remoteModelsInflight.delete(cacheKey);
        }
        setIsLoading(false);
      }
    },
    [canEnumerate, cacheKey, baseUrl, apiKey, apiFormat, proxyConfig, t],
  );

  const handleOpen = () => {
    if (!hasLoaded && !isLoading) {
      void loadModels(false);
    }
  };

  const handleRefresh = () => {
    void loadModels(true);
  };

  const searchText = search.trim();
  const hasExactModel = remoteModels.some(
    (model) => model.id.toLowerCase() === searchText.toLowerCase(),
  );

  return (
    <div className={cn("space-y-2", className)}>
      <FormLabel htmlFor={id}>{label}</FormLabel>
      <div className="flex items-center gap-2">
        <Input
          id={id}
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          autoComplete="off"
        />

        <Popover
          modal
          open={open}
          onOpenChange={(next) => {
            setOpen(next);
            if (next) {
              handleOpen();
            } else {
              setSearch("");
            }
          }}
        >
          <PopoverTrigger asChild>
            <Button
              type="button"
              variant="outline"
              size="icon"
              className="h-9 w-9 shrink-0"
              disabled={!canEnumerate}
              aria-label={t("providerForm.selectModel", {
                defaultValue: "Select model",
              })}
            >
              <ChevronDown className="h-4 w-4" />
            </Button>
          </PopoverTrigger>
          <PopoverContent
            side="bottom"
            align="end"
            sideOffset={6}
            avoidCollisions={true}
            collisionPadding={8}
            className="z-[1000] w-[30rem] max-w-[calc(100vw-2rem)] p-0 border-border-default"
          >
            <Command>
              <CommandInput
                value={search}
                onValueChange={setSearch}
                placeholder={t("providerForm.searchModels", {
                  defaultValue: "Search models...",
                })}
              />
              <CommandList>
                <CommandEmpty>
                  {isLoading
                    ? t("providerForm.loadingModels", {
                        defaultValue: "Loading models...",
                      })
                    : t("providerForm.noModelsFound", {
                        defaultValue: "No models found",
                      })}
                </CommandEmpty>

                {searchText && !hasExactModel && (
                  <CommandGroup
                    heading={t("providerForm.customModel", {
                      defaultValue: "Custom",
                    })}
                  >
                    <CommandItem
                      value={`custom:${searchText}`}
                      onSelect={() => {
                        onChange(searchText);
                        setOpen(false);
                        setSearch("");
                      }}
                    >
                      <Check className="mr-2 h-4 w-4 opacity-0" />
                      <div className="min-w-0 flex flex-col">
                        <span className="truncate">{searchText}</span>
                        <span className="truncate text-xs text-muted-foreground">
                          {t("providerForm.useTypedModel", {
                            defaultValue: "Use typed model",
                          })}
                        </span>
                      </div>
                    </CommandItem>
                  </CommandGroup>
                )}

                <CommandGroup>
                  {remoteModels.map((model) => {
                    const provider = normalizeProviderName(model);
                    return (
                      <CommandItem
                        key={model.id}
                        value={`${model.id} ${provider ?? ""}`}
                        keywords={[
                          model.id,
                          provider ?? "",
                          model.displayName ?? "",
                        ]}
                        onSelect={() => {
                          onChange(model.id);
                          setOpen(false);
                          setSearch("");
                        }}
                      >
                        <Check
                          className={cn(
                            "mr-2 h-4 w-4",
                            value === model.id ? "opacity-100" : "opacity-0",
                          )}
                        />
                        <div className="min-w-0 flex flex-col leading-tight">
                          <span className="truncate">{model.id}</span>
                          {provider && (
                            <span className="truncate text-xs text-muted-foreground">
                              {provider}
                            </span>
                          )}
                        </div>
                      </CommandItem>
                    );
                  })}
                </CommandGroup>
              </CommandList>
            </Command>
          </PopoverContent>
        </Popover>

        <Button
          type="button"
          variant="outline"
          size="icon"
          className="h-9 w-9 shrink-0"
          onClick={handleRefresh}
          disabled={isLoading || !canEnumerate}
        >
          <RefreshCw className={cn("h-4 w-4", isLoading && "animate-spin")} />
        </Button>
      </div>
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  );
}
