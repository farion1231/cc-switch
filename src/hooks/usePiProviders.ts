import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import * as piApi from "@/lib/api/pi";
import type { PiProviderConfig } from "@/lib/api/pi";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

const PI_QUERY_KEY = "pi-providers" as const;
const PI_SETTINGS_QUERY_KEY = "pi-settings" as const;

export function usePiProviders() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  const query = useQuery({
    queryKey: [PI_QUERY_KEY],
    queryFn: piApi.getPiProviders,
  });

  const providers = query.data ?? {};
  const providerList = Object.entries(providers).map(([id, config]) => ({
    id,
    ...config,
  }));

  const addProvider = useMutation({
    mutationFn: ({
      id,
      config,
    }: {
      id: string;
      config: PiProviderConfig;
    }) => piApi.setPiProvider(id, config),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [PI_QUERY_KEY] });
      toast.success(t("pi.providerAdded"));
    },
    onError: (error: Error) => {
      toast.error(error.message);
    },
  });

  const updateProvider = useMutation({
    mutationFn: ({
      id,
      config,
    }: {
      id: string;
      config: PiProviderConfig;
    }) => piApi.setPiProvider(id, config),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [PI_QUERY_KEY] });
      toast.success(t("pi.providerUpdated"));
    },
    onError: (error: Error) => {
      toast.error(error.message);
    },
  });

  const deleteProvider = useMutation({
    mutationFn: (id: string) => piApi.removePiProvider(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [PI_QUERY_KEY] });
      toast.success(t("pi.providerDeleted"));
    },
    onError: (error: Error) => {
      toast.error(error.message);
    },
  });

  const setActive = useMutation({
    mutationFn: ({
      providerId,
      modelId,
    }: {
      providerId: string;
      modelId?: string;
    }) => piApi.setActivePiProvider(providerId, modelId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [PI_QUERY_KEY] });
      queryClient.invalidateQueries({ queryKey: [PI_SETTINGS_QUERY_KEY] });
      toast.success(t("pi.providerActivated"));
    },
    onError: (error: Error) => {
      toast.error(error.message);
    },
  });

  return {
    providers: providerList,
    providersMap: providers,
    isLoading: query.isLoading,
    error: query.error,
    refetch: query.refetch,
    addProvider,
    updateProvider,
    deleteProvider,
    setActive,
  };
}

export function usePiSettings() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  const query = useQuery({
    queryKey: [PI_SETTINGS_QUERY_KEY],
    queryFn: piApi.getPiSettings,
  });

  const updateSettings = useMutation({
    mutationFn: (fields: Record<string, unknown>) =>
      piApi.updatePiSettings(fields),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [PI_SETTINGS_QUERY_KEY] });
      toast.success(t("pi.settingsUpdated"));
    },
    onError: (error: Error) => {
      toast.error(error.message);
    },
  });

  return {
    settings: query.data ?? {},
    isLoading: query.isLoading,
    updateSettings,
  };
}
