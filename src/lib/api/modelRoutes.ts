import { invoke } from "@tauri-apps/api/core";
import type { AppId } from "./types";

/** Model classes that can be routed to a dedicated provider. */
export type ModelClass = "opus" | "sonnet" | "haiku";

/** Map of model class -> provider id for a given app. */
export type ModelRoutes = Partial<Record<ModelClass, string>>;

export const modelRoutesApi = {
  /** Get all configured model-class -> provider routes for an app. */
  async getModelRoutes(appType: AppId): Promise<ModelRoutes> {
    return invoke("get_model_routes", { appType });
  },

  /**
   * Set (or clear) the provider route for a single model class.
   * Pass `providerId = null` to clear the route.
   */
  async setModelRoute(
    appType: AppId,
    modelClass: ModelClass,
    providerId: string | null,
  ): Promise<void> {
    return invoke("set_model_route", { appType, modelClass, providerId });
  },
};
