import { invoke } from "@tauri-apps/api/core";

/**
 * Wire DTO mirrored on the Rust side as `commands::api_key::ApiKeyDto`.
 * One row of `provider_api_keys` per provider.
 *
 * `apiKey` is plaintext in the DB to match the rest of the app's
 * storage (Provider.settings_config.api_key). A later secret-store
 * migration would touch both columns at once.
 */
export interface ApiKeyDto {
  id: string;
  providerId: string;
  appType: string;
  label: string;
  apiKey: string;
  tags: string[];
  notes: string;
  enabled: boolean;
  sortIndex: number;
  isActive: boolean;
  /** Unix seconds; 0 = no cooldown */
  cooldownUntil: number;
  failureCount: number;
  lastUsedAt: number | null;
  lastError: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface CreateApiKeyInput {
  providerId: string;
  appType: string;
  label: string;
  apiKey: string;
  tags?: string[];
  notes?: string;
  /** Default true on the Rust side; we mirror the default here for clarity. */
  enabled?: boolean;
}

export interface UpdateApiKeyInput {
  label?: string;
  apiKey?: string;
  tags?: string[];
  notes?: string;
  enabled?: boolean;
  sortIndex?: number;
  isActive?: boolean;
}

/**
 * 删除 key 后端返回的元数据——前端用来精确 invalidate
 * `["apiKeys", providerId, appType]` 这一格缓存。
 */
export interface DeletedApiKeyInfo {
  keyId: string;
  providerId: string;
  appType: string;
}

/**
 * Thin Tauri invoke wrapper for the 8 `cmd_*_api_key` commands. The
 * frontend never talks to the DB directly — all state changes go
 * through here so KeyRing + settings.json side-effects can be hooked
 * up later (Phase 10).
 */
export const apiKeysApi = {
  /** 列出某 provider 的全部 key（按 sortIndex ASC）。 */
  list: (providerId: string, appType: string) =>
    invoke<ApiKeyDto[]>("cmd_list_api_keys", { providerId, appType }),

  /** 取单个 key。 */
  get: (keyId: string) =>
    invoke<ApiKeyDto | null>("cmd_get_api_key", { keyId }),

  /**
   * 取当前 active key（写入 settings.json 用的那把）。
   * 返回 null 当没有 key 被激活。
   */
  getActive: (providerId: string, appType: string) =>
    invoke<ApiKeyDto | null>("cmd_get_active_api_key", {
      providerId,
      appType,
    }),

  /** 新建一把 key。返回插入后的整行。 */
  create: (payload: CreateApiKeyInput) =>
    invoke<ApiKeyDto>("cmd_create_api_key", { payload }),

  /**
   * 更新 key。`payload` 里未提供的字段不会被写——DAO 的动态 SET
   * clause 依赖 Some/None 来决定要 UPDATE 哪些列。
   */
  update: (keyId: string, payload: UpdateApiKeyInput) =>
    invoke<ApiKeyDto>("cmd_update_api_key", { keyId, payload }),

  /** 删除 key。返回 (keyId, providerId, appType) 让前端精确 invalidate
   * 那一格 apiKeys 缓存——不再广播到所有 ["apiKeys"] 触发多余 refetch。 */
  remove: (keyId: string) =>
    invoke<DeletedApiKeyInfo>("cmd_delete_api_key", { keyId }),

  /**
   * 拖拽重排——传完整的有序 keyId 列表。DAO 会把 list[0] 设为
   * sortIndex=0、list[1] 设为 sortIndex=1，以此类推。
   */
  reorder: (appType: string, orderedIds: string[]) =>
    invoke<void>("cmd_reorder_api_keys", { appType, orderedIds }),

  /**
   * 把某 key 设为「写入 settings.json 的那把」+ 触达 KeyRing。
   * 事务保证同 provider 下只有一个 active key。
   */
  setActive: (providerId: string, appType: string, keyId: string) =>
    invoke<ApiKeyDto>("cmd_set_active_api_key", {
      providerId,
      appType,
      keyId,
    }),
};
