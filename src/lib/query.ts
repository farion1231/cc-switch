import { QueryClient, useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Provider } from '../types'
import type { AppType } from '../lib/tauri-api'

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: false,
      refetchOnWindowFocus: false,
    },
  },
})

export const useProvidersQuery = (appType: AppType) => {
  return useQuery({
    queryKey: ['providers', appType],
    queryFn: async () => {
      const [providers, currentProviderId] = await Promise.all([
        window.api.getProviders(appType),
        window.api.getCurrentProvider(appType)
      ])

      // Auto-import default providers if list is empty
      if (Object.keys(providers).length === 0) {
        const result = await window.api.importCurrentConfigAsDefault(appType)
        if (result.success) {
          const [newProviders, newCurrentProviderId] = await Promise.all([
            window.api.getProviders(appType),
            window.api.getCurrentProvider(appType)
          ])
          return { providers: newProviders, currentProviderId: newCurrentProviderId }
        }
      }

      return { providers, currentProviderId }
    }
  })
}

export const useAddProviderMutation = (appType: AppType) => {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (provider: Omit<Provider, "id">) => {
      const newProvider: Provider = {
        ...provider,
        id: crypto.randomUUID(),
        createdAt: Date.now(),
      }
      await window.api.addProvider(newProvider, appType)
      return newProvider
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['providers', appType] })
      await window.api.updateTrayMenu()
    }
  })
}

export const useUpdateProviderMutation = (appType: AppType) => {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (provider: Provider) => {
      await window.api.updateProvider(provider, appType)
      return provider
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['providers', appType] })
      await window.api.updateTrayMenu()
    }
  })
}

export const useDeleteProviderMutation = (appType: AppType) => {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (id: string) => {
      await window.api.deleteProvider(id, appType)
      return id
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['providers', appType] })
      await window.api.updateTrayMenu()
    }
  })
}

export const useSwitchProviderMutation = (appType: AppType) => {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (providerId: string) => {
      const success = await window.api.switchProvider(providerId, appType)
      return { providerId, success }
    },
    onSuccess: async ({ success }) => {
      if (success) {
        await queryClient.invalidateQueries({ queryKey: ['providers', appType] })
        await window.api.updateTrayMenu()
      }
    }
  })
}

export const useVSCodeSyncMutation = (appType: AppType) => {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (providerId: string) => {
      const status = await window.api.getVSCodeSettingsStatus()
      if (!status.exists) {
        throw new Error('VS Code settings not found')
      }

      const raw = await window.api.readVSCodeSettings()
      const providersData = queryClient.getQueryData<{ providers: Record<string, Provider>, currentProviderId: string }>(['providers', appType])
      const provider = providersData?.providers[providerId]

      if (!provider) {
        throw new Error('Provider not found')
      }

      const isOfficial = provider.category === "official"
      let baseUrl: string | undefined = undefined

      if (!isOfficial) {
        const { getCodexBaseUrl } = await import('../utils/providerConfigUtils')
        const parsed = getCodexBaseUrl(provider)
        if (!parsed) {
          throw new Error('Missing base URL for non-official provider')
        }
        baseUrl = parsed
      }

      const { applyProviderToVSCode } = await import('../utils/vscodeSettings')
      const updatedSettings = applyProviderToVSCode(raw, { baseUrl, isOfficial })

      if (updatedSettings !== raw) {
        await window.api.writeVSCodeSettings(updatedSettings)
      }

      await queryClient.invalidateQueries({ queryKey: ['providers', appType] })

      return { success: true, updated: updatedSettings !== raw }
    }
  })
}