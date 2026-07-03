import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { extractErrorMessage } from "@/utils/errorUtils";
import { apiKeysApi, type CreateApiKeyInput, type UpdateApiKeyInput } from "@/lib/api/apiKey";

/**
 * React Query surface for the `provider_api_keys` table.
 *
 * **单一真相来源**：`useApiKeys` 返回的列表里每条 key 都带 `is_active` 字段，
 * 调用方自己 `.find((k) => k.is_active)` 即可拿到 active key。不要再单独
 * 起一个 `useActiveApiKey` 查询——同一份状态走两个 query 会让 React Query
 * 缓存里出现两份可能不一致的快照（`apiKeys` refetch 完但 `activeApiKey` 还
 * 没 refetch，UI 上会闪一下"老 active + 新 list"）。
 *
 * Query key shape：
 *   ["apiKeys", providerId, appType]
 *   ["apiKey", keyId]                —— 仅给单 key 详情页用，普通列表不需要
 *
 * 所有 mutations 都会 invalidate `["apiKeys", providerId, appType]`，
 * active key 状态会随着 list 一起更新，零额外协调。
 */

// ========== Queries ==========

/** List all keys for a provider (sorted by sortIndex ASC). */
export function useApiKeys(providerId: string | null, appType: string) {
  return useQuery({
    queryKey: ["apiKeys", providerId, appType],
    queryFn: () => apiKeysApi.list(providerId as string, appType),
    enabled: !!providerId && !!appType,
    // 30s refetch keeps the runtime fields (cooldown, failure_count)
    // fresh even when the proxy is doing the writes. The KeyRing
    // writes through the same DB row, so this is the cheapest way to
    // surface "this key just got 429'd" to the user.
    refetchInterval: 30_000,
  });
}

/**
 * 从 `useApiKeys` 列表里挑出 active key。等价于原来的 `useActiveApiKey`，
 * 但少一次网络往返 + 一份缓存——保持单一数据源。
 *
 * 选 `is_active=true` 的第一条；如果 DB 没设 active（理论不会出现，
 * set_active_api_key_with_settings 事务里强制保证），退回 sortIndex=0。
 */
export function pickActiveKey<T extends { id: string; isActive: boolean; sortIndex: number }>(
  keys: T[] | undefined,
): T | null {
  if (!keys || keys.length === 0) return null;
  return (
    keys.find((k) => k.isActive) ??
    [...keys].sort((a, b) => a.sortIndex - b.sortIndex)[0] ??
    null
  );
}

/** Single key lookup. */
export function useApiKey(keyId: string | null) {
  return useQuery({
    queryKey: ["apiKey", keyId],
    queryFn: () => apiKeysApi.get(keyId as string),
    enabled: !!keyId,
  });
}

// ========== Mutations ==========

/** Create a new key. Defaults `enabled=true` server-side. */
export function useCreateApiKey() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (payload: CreateApiKeyInput) => apiKeysApi.create(payload),
    onSuccess: (created) => {
      queryClient.invalidateQueries({
        queryKey: ["apiKeys", created.providerId, created.appType],
      });
      toast.success(t("apiKeys.create.success", { defaultValue: "API key 已添加" }));
    },
    onError: (error) => {
      toast.error(
        t("apiKeys.create.error", {
          defaultValue: `添加失败: ${extractErrorMessage(error)}`,
        }),
      );
    },
  });
}

/**
 * Patch a key. Pass only the fields you want to change — the DAO's
 * dynamic SET clause is `None` for everything else.
 */
export function useUpdateApiKey() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: ({
      keyId,
      payload,
    }: {
      keyId: string;
      payload: UpdateApiKeyInput;
    }) => apiKeysApi.update(keyId, payload),
    onSuccess: (updated) => {
      queryClient.invalidateQueries({
        queryKey: ["apiKeys", updated.providerId, updated.appType],
      });
      queryClient.invalidateQueries({ queryKey: ["apiKey", updated.id] });
      toast.success(t("apiKeys.update.success", { defaultValue: "已保存" }));
    },
    onError: (error) => {
      toast.error(
        t("apiKeys.update.error", {
          defaultValue: `保存失败: ${extractErrorMessage(error)}`,
        }),
      );
    },
  });
}

/**
 * Delete a key. The backend returns the deleted key's parent
 * (providerId, appType), which we use to invalidate exactly that
 * `["apiKeys", providerId, appType]` cache slice — no more broad
 * `["apiKeys"]` fan-out that would refetch all pools.
 */
export function useDeleteApiKey() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: async (keyId: string) => {
      return apiKeysApi.remove(keyId);
    },
    onSuccess: (info) => {
      queryClient.invalidateQueries({
        queryKey: ["apiKeys", info.providerId, info.appType],
      });
      queryClient.invalidateQueries({ queryKey: ["apiKey", info.keyId] });
      toast.success(t("apiKeys.delete.success", { defaultValue: "已删除" }));
    },
    onError: (error) => {
      toast.error(
        t("apiKeys.delete.error", {
          defaultValue: `删除失败: ${extractErrorMessage(error)}`,
        }),
      );
    },
  });
}

/**
 * Drag-reorder. The caller hands us the full ordered list of keyIds
 * for an `(appType, providerId)`. We don't know providerId here, so
 * we invalidate the broad list query.
 */
export function useReorderApiKeys() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      orderedIds,
    }: {
      appType: string;
      providerId: string;
      orderedIds: string[];
    }) => apiKeysApi.reorder(appType, orderedIds),
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["apiKeys", variables.providerId, variables.appType],
      });
    },
  });
}

/**
 * Promote a key to `is_active=1` (the one written into settings.json).
 * Also invalidates the active-key query so the form's radio updates.
 */
export function useSetActiveApiKey() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: ({
      providerId,
      appType,
      keyId,
    }: {
      providerId: string;
      appType: string;
      keyId: string;
    }) => apiKeysApi.setActive(providerId, appType, keyId),
    onSuccess: (updated) => {
      queryClient.invalidateQueries({
        queryKey: ["apiKeys", updated.providerId, updated.appType],
      });
      toast.success(
        t("apiKeys.setActive.success", { defaultValue: "已设为默认 key" }),
      );
    },
    onError: (error) => {
      toast.error(
        t("apiKeys.setActive.error", {
          defaultValue: `设置失败: ${extractErrorMessage(error)}`,
        }),
      );
    },
  });
}
