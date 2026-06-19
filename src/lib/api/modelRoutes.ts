import { invoke } from "@tauri-apps/api/core";
import type { AppId } from "./types";

/** Model classes that can be routed to a dedicated provider. */
export type ModelClass = "opus" | "sonnet" | "haiku";

/** A configured route: which provider, and optionally which model on it. */
export interface ModelRoute {
  /** Provider the model class is routed to. */
  providerId: string;
  /**
   * Exact model to request on that provider. `null`/empty means "use the
   * provider's own model mapping / default model".
   */
  model: string | null;
}

/** Map of model class -> route for a given app. */
export type ModelRoutes = Partial<Record<ModelClass, ModelRoute>>;

export const modelRoutesApi = {
  /** Get all configured model-class -> route entries for an app. */
  async getModelRoutes(appType: AppId): Promise<ModelRoutes> {
    return invoke("get_model_routes", { appType });
  },

  /**
   * Set (or clear) the route for a single model class.
   *
   * - Pass `providerId = null` to clear the route (falls back to the app's
   *   normal current/failover provider).
   * - `targetModel` is the exact model to use on that provider; pass `null` or
   *   an empty string to use the provider's own model mapping / default model.
   */
  async setModelRoute(
    appType: AppId,
    modelClass: ModelClass,
    providerId: string | null,
    targetModel: string | null,
  ): Promise<void> {
    return invoke("set_model_route", {
      appType,
      modelClass,
      providerId,
      targetModel,
    });
  },
};
