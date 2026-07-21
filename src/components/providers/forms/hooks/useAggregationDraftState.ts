import { useEffect, useRef, useState } from "react";
import type {
  AggregationConfig,
  AggregationRouteConfig,
  ClaudeApiFormat,
  ClaudeApiKeyField,
} from "@/types";

/** 聚合上游草稿（表单内部状态） */
export interface AggUpstreamDraft {
  id: string;
  name: string;
  baseUrl: string;
  apiKey: string;
  apiFormat: ClaudeApiFormat;
  apiKeyField: ClaudeApiKeyField;
  isFullUrl: boolean;
}

/** Claude 模型角色 */
export type AggRoleKey =
  | "sonnet"
  | "opus"
  | "fable"
  | "haiku"
  | "subagent"
  | "default";

export const AGG_ROLE_KEYS: AggRoleKey[] = [
  "sonnet",
  "opus",
  "fable",
  "haiku",
  "subagent",
  "default",
];

/** 支持声明 1M 的角色（与单一供应商一致；subagent 不显示在 /model 菜单，无 1M） */
export const AGG_ROLE_SUPPORTS_1M: Record<AggRoleKey, boolean> = {
  sonnet: true,
  opus: true,
  fable: true,
  haiku: false,
  subagent: false,
  default: true,
};

/** 每个角色对应的 Claude 模型环境变量键 */
const ROLE_MODEL_ENV: Record<AggRoleKey, string> = {
  sonnet: "ANTHROPIC_DEFAULT_SONNET_MODEL",
  opus: "ANTHROPIC_DEFAULT_OPUS_MODEL",
  fable: "ANTHROPIC_DEFAULT_FABLE_MODEL",
  haiku: "ANTHROPIC_DEFAULT_HAIKU_MODEL",
  subagent: "CLAUDE_CODE_SUBAGENT_MODEL",
  default: "ANTHROPIC_MODEL",
};

/** 由角色映射托管的所有 env 键（保存时先清空再按当前角色重建） */
export const AGG_MANAGED_ENV_KEYS: string[] = [
  "ANTHROPIC_MODEL",
  "ANTHROPIC_DEFAULT_SONNET_MODEL",
  "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
  "ANTHROPIC_DEFAULT_OPUS_MODEL",
  "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
  "ANTHROPIC_DEFAULT_FABLE_MODEL",
  "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
  "ANTHROPIC_DEFAULT_HAIKU_MODEL",
  "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
  "CLAUDE_CODE_SUBAGENT_MODEL",
];

const ONE_M_MARKER = "[1M]";

/** 单个角色映射草稿：左=Claude 角色，右=聚合提供的（上游 + 模型） */
export interface AggRoleDraft {
  upstreamId: string;
  model: string;
  supports1m: boolean;
}

/** 序列化后的聚合配置（写入 provider meta.aggregation） */
export type AggregationConfigPayload = AggregationConfig;

export type AggregationValidationError = "no_upstream" | "no_role";

function genId(): string {
  if (typeof crypto !== "undefined" && crypto.randomUUID) {
    return crypto.randomUUID().slice(0, 8);
  }
  return Math.random().toString(36).slice(2, 10);
}

function newUpstream(): AggUpstreamDraft {
  return {
    id: genId(),
    name: "",
    baseUrl: "",
    apiKey: "",
    apiFormat: "anthropic",
    apiKeyField: "ANTHROPIC_AUTH_TOKEN",
    isFullUrl: false,
  };
}

function emptyRoles(): Record<AggRoleKey, AggRoleDraft> {
  const roles = {} as Record<AggRoleKey, AggRoleDraft>;
  for (const key of AGG_ROLE_KEYS) {
    roles[key] = { upstreamId: "", model: "", supports1m: false };
  }
  return roles;
}

/** 从已保存的 meta.aggregation 还原草稿（编辑态） */
function parseInitial(initial: unknown): {
  upstreams: AggUpstreamDraft[];
  roles: Record<AggRoleKey, AggRoleDraft>;
  legacyRoutes: AggregationRouteConfig[];
} {
  const obj = (initial ?? {}) as Record<string, unknown>;
  const rawUpstreams = Array.isArray(obj.upstreams) ? obj.upstreams : [];

  const upstreams: AggUpstreamDraft[] = rawUpstreams.map((u) => {
    const up = (u ?? {}) as Record<string, unknown>;
    return {
      id: typeof up.id === "string" && up.id ? up.id : genId(),
      name: typeof up.name === "string" ? up.name : "",
      baseUrl: typeof up.baseUrl === "string" ? up.baseUrl : "",
      apiKey: typeof up.apiKey === "string" ? up.apiKey : "",
      apiFormat: (typeof up.apiFormat === "string"
        ? up.apiFormat
        : "anthropic") as ClaudeApiFormat,
      apiKeyField:
        up.apiKeyField === "ANTHROPIC_API_KEY"
          ? "ANTHROPIC_API_KEY"
          : "ANTHROPIC_AUTH_TOKEN",
      isFullUrl: up.isFullUrl === true,
    };
  });

  const roles = emptyRoles();
  const rawRoles = (obj.roles ?? {}) as Record<string, unknown>;
  for (const key of AGG_ROLE_KEYS) {
    const r = (rawRoles[key] ?? {}) as Record<string, unknown>;
    roles[key] = {
      upstreamId: typeof r.upstreamId === "string" ? r.upstreamId : "",
      model: typeof r.model === "string" ? r.model : "",
      supports1m:
        r.supports1m === true && AGG_ROLE_SUPPORTS_1M[key] ? true : false,
    };
  }

  const rawRoutes = Array.isArray(obj.routes) ? obj.routes : [];
  const legacyRoutes = rawRoutes.flatMap((route) => {
    const r = (route ?? {}) as Record<string, unknown>;
    if (typeof r.model !== "string" || typeof r.upstreamId !== "string") {
      return [];
    }
    return [
      {
        model: r.model,
        upstreamId: r.upstreamId,
        upstreamModel:
          typeof r.upstreamModel === "string" ? r.upstreamModel : undefined,
      },
    ];
  });

  return {
    upstreams: upstreams.length > 0 ? upstreams : [newUpstream()],
    roles,
    legacyRoutes,
  };
}

/**
 * 「供应商聚合」表单草稿状态（角色映射版）。
 *
 * 维护多条上游 + 按 Claude 角色的模型映射；`toConfig()` 序列化为
 * `provider.meta.aggregation`，`toEnv()` 生成写入 Claude settings.json 的
 * `ANTHROPIC_DEFAULT_*_MODEL` 等键，使 Claude Code 按角色发送对应模型名。
 */
export function useAggregationDraftState(initial?: unknown) {
  const [initialState] = useState(() => parseInitial(initial));
  const [upstreams, setUpstreams] = useState<AggUpstreamDraft[]>(
    initialState.upstreams,
  );
  const [roles, setRoles] = useState<Record<AggRoleKey, AggRoleDraft>>(
    initialState.roles,
  );
  const [legacyRoutes, setLegacyRoutes] = useState<AggregationRouteConfig[]>(
    initialState.legacyRoutes,
  );
  const initialSignature = JSON.stringify(initial ?? null);
  const previousInitialSignature = useRef(initialSignature);

  useEffect(() => {
    if (previousInitialSignature.current === initialSignature) return;
    previousInitialSignature.current = initialSignature;
    const next = parseInitial(initial);
    setUpstreams(next.upstreams);
    setRoles(next.roles);
    setLegacyRoutes(next.legacyRoutes);
  }, [initial, initialSignature]);

  const addUpstream = () => setUpstreams((prev) => [...prev, newUpstream()]);
  const removeUpstream = (id: string) => {
    setUpstreams((prev) => prev.filter((u) => u.id !== id));
    // 清除引用了该上游的角色绑定
    setRoles((prev) => {
      const next = { ...prev };
      for (const key of AGG_ROLE_KEYS) {
        if (next[key].upstreamId === id) {
          next[key] = { ...next[key], upstreamId: "" };
        }
      }
      return next;
    });
    setLegacyRoutes((prev) => prev.filter((route) => route.upstreamId !== id));
  };
  const updateUpstream = (id: string, patch: Partial<AggUpstreamDraft>) =>
    setUpstreams((prev) =>
      prev.map((u) => (u.id === id ? { ...u, ...patch } : u)),
    );

  const updateRole = (role: AggRoleKey, patch: Partial<AggRoleDraft>) =>
    setRoles((prev) => ({ ...prev, [role]: { ...prev[role], ...patch } }));

  const removeLegacyRoute = (index: number) =>
    setLegacyRoutes((prev) => prev.filter((_, i) => i !== index));

  const validRoleEntries = (): Array<[AggRoleKey, AggRoleDraft]> =>
    AGG_ROLE_KEYS.map(
      (key) => [key, roles[key]] as [AggRoleKey, AggRoleDraft],
    ).filter(([, r]) => r.model.trim() && r.upstreamId.trim());

  const toConfig = (): AggregationConfigPayload => {
    const serializedUpstreams = upstreams
      .filter((u) => u.baseUrl.trim())
      .map((u) => ({
        id: u.id,
        name: u.name.trim() || undefined,
        baseUrl: u.baseUrl.trim(),
        apiKey: u.apiKey.trim(),
        apiFormat: u.apiFormat,
        apiKeyField: u.apiKeyField,
        isFullUrl: u.isFullUrl || undefined,
      }));
    const validUpstreamIds = new Set(serializedUpstreams.map((u) => u.id));

    const rolesOut: AggregationConfig["roles"] = {};
    for (const [key, r] of validRoleEntries()) {
      if (!validUpstreamIds.has(r.upstreamId.trim())) continue;
      rolesOut![key] = {
        upstreamId: r.upstreamId.trim(),
        model: r.model.trim(),
        supports1m:
          AGG_ROLE_SUPPORTS_1M[key] && r.supports1m ? true : undefined,
      };
    }

    return {
      upstreams: serializedUpstreams,
      roles: rolesOut,
      routes: legacyRoutes.length > 0 ? legacyRoutes : undefined,
    };
  };

  /** 生成写入 Claude settings.json env 的模型键（含 [1M] 标记） */
  const toEnv = (): Record<string, string> => {
    const validUpstreamIds = new Set(
      upstreams.filter((u) => u.baseUrl.trim()).map((u) => u.id),
    );
    const env: Record<string, string> = {};
    for (const [key, r] of validRoleEntries()) {
      if (!validUpstreamIds.has(r.upstreamId.trim())) continue;
      const oneM =
        AGG_ROLE_SUPPORTS_1M[key] && r.supports1m ? ONE_M_MARKER : "";
      env[ROLE_MODEL_ENV[key]] = r.model.trim() + oneM;
    }
    return env;
  };

  const validate = (): AggregationValidationError | null => {
    const validUpstreamIds = new Set(
      upstreams.filter((u) => u.baseUrl.trim()).map((u) => u.id),
    );
    if (validUpstreamIds.size === 0) return "no_upstream";
    const hasRole = validRoleEntries().some(([, r]) =>
      validUpstreamIds.has(r.upstreamId.trim()),
    );
    const hasLegacyRoute = legacyRoutes.some(
      (route) =>
        route.model.trim() && validUpstreamIds.has(route.upstreamId.trim()),
    );
    if (!hasRole && !hasLegacyRoute) return "no_role";
    return null;
  };

  return {
    upstreams,
    roles,
    legacyRoutes,
    addUpstream,
    removeUpstream,
    updateUpstream,
    updateRole,
    removeLegacyRoute,
    toConfig,
    toEnv,
    validate,
  };
}

export type AggregationDraft = ReturnType<typeof useAggregationDraftState>;
