import React from 'react'
import { QueryClient, useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'
import { listen, UnlistenFn } from '@tauri-apps/api/event'
import { Provider } from '../types'
import { detectApplied, normalizeBaseUrl, applyProviderToVSCode } from '../utils/vscodeSettings'
import { getCodexBaseUrl } from '../utils/providerConfigUtils'

export type AppType = "claude" | "codex"

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
      let providers: Record<string, Provider> = {}
      let currentProviderId = ""

      try {
        providers = await invoke("get_providers", { app_type: appType, app: appType })
      } catch (error) {
        console.error("获取供应商列表失败:", error)
      }

      try {
        currentProviderId = await invoke("get_current_provider", { app_type: appType, app: appType })
      } catch (error) {
        console.error("获取当前供应商失败:", error)
      }

      // Auto-import default providers if list is empty
      if (Object.keys(providers).length === 0) {
        const result = await (async () => {
          try {
            const success = await invoke<boolean>("import_default_config", { app_type: appType, app: appType })
            return {
              success,
              message: success ? "成功导入默认配置" : "导入失败",
            }
          } catch (error) {
            console.error("导入默认配置失败:", error)
            return {
              success: false,
              message: String(error),
            }
          }
        })()
        if (result.success) {
          const [newProviders, newCurrentProviderId] = await Promise.all([
            invoke("get_providers", { app_type: appType, app: appType }),
            invoke("get_current_provider", { app_type: appType, app: appType })
          ])
          return { providers: newProviders as Record<string, Provider>, currentProviderId: newCurrentProviderId }
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
      try {
        return await invoke("add_provider", { provider: newProvider, app_type: appType, app: appType })
      } catch (error) {
        console.error("添加供应商失败:", error)
        throw error
      }
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['providers', appType] })
      try {
        await invoke("update_tray_menu")
      } catch (error) {
        console.error("更新托盘菜单失败:", error)
      }
    }
  })
}

export const useUpdateProviderMutation = (appType: AppType) => {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (provider: Provider) => {
      try {
        return await invoke("update_provider", { provider, app_type: appType, app: appType })
      } catch (error) {
        console.error("更新供应商失败:", error)
        throw error
      }
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['providers', appType] })
      try {
        await invoke("update_tray_menu")
      } catch (error) {
        console.error("更新托盘菜单失败:", error)
      }
    }
  })
}

export const useDeleteProviderMutation = (appType: AppType) => {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (id: string) => {
      try {
        return await invoke("delete_provider", { id, app_type: appType, app: appType })
      } catch (error) {
        console.error("删除供应商失败:", error)
        throw error
      }
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['providers', appType] })
      try {
        await invoke("update_tray_menu")
      } catch (error) {
        console.error("更新托盘菜单失败:", error)
      }
    }
  })
}

export const useSwitchProviderMutation = (appType: AppType) => {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (providerId: string) => {
      try {
        const success = await invoke("switch_provider", { id: providerId, app_type: appType, app: appType })
        return { providerId, success }
      } catch (error) {
        console.error("切换供应商失败:", error)
        return { providerId, success: false }
      }
    },
    onSuccess: async ({ success }) => {
      if (success) {
        await queryClient.invalidateQueries({ queryKey: ['providers', appType] })
        try {
          await invoke("update_tray_menu")
        } catch (error) {
          console.error("更新托盘菜单失败:", error)
        }
      }
    }
  })
}

export const useVSCodeSyncMutation = (appType: AppType) => {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (providerId: string) => {
      const status = await (async () => {
        try {
          return await invoke("get_vscode_settings_status") as { exists: boolean; path: string; error?: string }
        } catch (error) {
          console.error("获取 VS Code 设置状态失败:", error)
          return { exists: false, path: "", error: String(error) }
        }
      })()
      if (!status.exists) {
        throw new Error('VS Code settings not found')
      }

      const raw = await (async () => {
        try {
          return await invoke("read_vscode_settings") as string
        } catch (error) {
          throw new Error(`读取 VS Code 设置失败: ${String(error)}`)
        }
      })()
      const providersData = queryClient.getQueryData<{ providers: Record<string, Provider>, currentProviderId: string }>(['providers', appType])
      const provider = providersData?.providers[providerId]

      if (!provider) {
        throw new Error('Provider not found')
      }

      const isOfficial = provider.category === "official"
      let baseUrl: string | undefined = undefined

      if (!isOfficial) {
        const parsed = getCodexBaseUrl(provider)
        if (!parsed) {
          throw new Error('Missing base URL for non-official provider')
        }
        baseUrl = parsed
      }

      const updatedSettings = applyProviderToVSCode(raw, { baseUrl, isOfficial })

      if (updatedSettings !== raw) {
        await (async () => {
          try {
            return await invoke("write_vscode_settings", { content: updatedSettings }) as boolean
          } catch (error) {
            throw new Error(`写入 VS Code 设置失败: ${String(error)}`)
          }
        })()
      }

      await queryClient.invalidateQueries({ queryKey: ['providers', appType] })

      return { success: true, updated: updatedSettings !== raw }
    }
  })
}

export const useVSCodeSettingsQuery = () => {
  return useQuery({
    queryKey: ['vscode-settings'],
    queryFn: async () => {
      const status = await (async () => {
        try {
          return await invoke("get_vscode_settings_status") as { exists: boolean; path: string; error?: string }
        } catch (error) {
          console.error("获取 VS Code 设置状态失败:", error)
          return { exists: false, path: "", error: String(error) }
        }
      })()
      if (!status.exists) {
        return { status: null, content: null }
      }

      const content = await (async () => {
        try {
          return await invoke("read_vscode_settings")
        } catch (error) {
          throw new Error(`读取 VS Code 设置失败: ${String(error)}`)
        }
      })()
      return { status, content }
    },
    enabled: false, // Manual query only
    retry: false
  })
}

export const useVSCodeAppliedQuery = (appType: AppType, currentProviderId: string, providers: Record<string, Provider>) => {
  return useQuery({
    queryKey: ['vscode-applied', appType, currentProviderId],
    queryFn: async () => {
      if (appType !== "codex" || !currentProviderId) {
        return null
      }

      const status = await (async () => {
        try {
          return await invoke("get_vscode_settings_status") as { exists: boolean; path: string; error?: string }
        } catch (error) {
          console.error("获取 VS Code 设置状态失败:", error)
          return { exists: false, path: "", error: String(error) }
        }
      })()

      if (!status.exists) {
        return null
      }

      try {
        const content = await invoke("read_vscode_settings") as string
        const detected = detectApplied(content)
        const current = providers[currentProviderId]

        let applied = false
        if (current && current.category !== "official") {
          const base = getCodexBaseUrl(current)
          if (detected.apiBase && base) {
            applied = normalizeBaseUrl(detected.apiBase) === normalizeBaseUrl(base)
          }
        }

        return applied ? currentProviderId : null
      } catch (error) {
        console.error("检查 VS Code 应用状态失败:", error)
        return null
      }
    },
    enabled: appType === "codex" && !!currentProviderId,
    retry: false,
    refetchOnWindowFocus: true
  })
}

export const useVSCodeRemoveMutation = () => {
  return useMutation({
    mutationFn: async () => {
      const status = await (async () => {
        try {
          return await invoke("get_vscode_settings_status") as { exists: boolean; path: string; error?: string }
        } catch (error) {
          console.error("获取 VS Code 设置状态失败:", error)
          return { exists: false, path: "", error: String(error) }
        }
      })()
      if (!status.exists) {
        throw new Error('VS Code settings not found')
      }

      const raw = await (async () => {
        try {
          return await invoke("read_vscode_settings") as string
        } catch (error) {
          throw new Error(`读取 VS Code 设置失败: ${String(error)}`)
        }
      })()
      const updatedSettings = applyProviderToVSCode(raw, {
        baseUrl: undefined,
        isOfficial: true,
      })

      if (updatedSettings !== raw) {
        await (async () => {
          try {
            return await invoke("write_vscode_settings", { content: updatedSettings }) as boolean
          } catch (error) {
            throw new Error(`写入 VS Code 设置失败: ${String(error)}`)
          }
        })()
      }

      return { success: true, updated: updatedSettings !== raw }
    }
  })
}

// Settings-related queries and mutations
export const useSettingsQuery = () => {
  return useQuery({
    queryKey: ['settings'],
    queryFn: async () => {
      try {
        const settings = await invoke("get_settings")
        return settings
      } catch (error) {
        console.error("获取设置失败:", error)
        throw error
      }
    }
  })
}

export const useSaveSettingsMutation = () => {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (settings: any) => {
      try {
        return await invoke("save_settings", { settings })
      } catch (error) {
        console.error("保存设置失败:", error)
        throw error
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['settings'] })
    }
  })
}

export const useAppConfigPathQuery = () => {
  return useQuery({
    queryKey: ['app-config-path'],
    queryFn: async () => {
      try {
        const path = await invoke("get_app_config_path")
        return path as string
      } catch (error) {
        console.error("获取配置路径失败:", error)
        throw error
      }
    }
  })
}

export const useConfigDirQuery = (appType: AppType) => {
  return useQuery({
    queryKey: ['config-dir', appType],
    queryFn: async () => {
      try {
        const dir = await invoke("get_config_dir", { app_type: appType })
        return dir as string
      } catch (error) {
        console.error(`获取${appType}配置目录失败:`, error)
        throw error
      }
    }
  })
}

export const useIsPortableQuery = () => {
  return useQuery({
    queryKey: ['is-portable'],
    queryFn: async () => {
      try {
        const portable = await invoke("is_portable")
        return portable as boolean
      } catch (error) {
        console.error("检测便携模式失败:", error)
        throw error
      }
    }
  })
}

export const useVersionQuery = () => {
  return useQuery({
    queryKey: ['version'],
    queryFn: async () => {
      try {
        const { getVersion } = await import("@tauri-apps/api/app")
        const version = await getVersion()
        return version
      } catch (error) {
        console.error("获取版本号失败:", error)
        throw error
      }
    }
  })
}

// Event listener for provider switching
export const useProviderSwitchedListener = (
  callback: (data: { appType: string; providerId: string }) => void
) => {
  React.useEffect(() => {
    let unlisten: UnlistenFn | null = null

    const setupListener = async () => {
      try {
        unlisten = await listen("provider-switched", (event) => {
          callback(event.payload as { appType: string; providerId: string })
        })
      } catch (error) {
        console.error("Failed to setup provider switched listener:", error)
      }
    }

    setupListener()

    return () => {
      if (unlisten) {
        unlisten()
      }
    }
  }, [callback])
}