import { PermanentError } from "./lib/contract.mjs";

/**
 * API Key refresh template configuration.
 *
 * Keep MODE="demo" until you have changed the URL and response mapping in
 * lib/provider.mjs. Environment variables below make local testing convenient
 * without putting credentials in this file.
 */
export const MODE = process.env.API_KEY_TEMPLATE_MODE ?? "demo";

export const HTTP = {
  // Replace this placeholder with your provider's HTTPS endpoint.
  url:
    process.env.API_KEY_REFRESH_URL ??
    "https://api.example.invalid/v1/refresh-key",
  // The bearer token is inherited from the CC Switch process environment.
  tokenEnvName: "API_KEY_REFRESH_TOKEN",
  // This is POST JSON body example. Adapt it for your provider.
  requestBody: { audience: "my-api" },
  // CC Switch allows 5-300 seconds; this template validates the same range.
  timeoutSeconds: Number(process.env.API_KEY_REFRESH_TIMEOUT_SECONDS ?? "20"),
};

export function getConfiguration() {
  if (MODE !== "demo" && MODE !== "http") {
    throw new PermanentError('API_KEY_TEMPLATE_MODE must be "demo" or "http".');
  }
  if (
    !Number.isFinite(HTTP.timeoutSeconds) ||
    HTTP.timeoutSeconds < 5 ||
    HTTP.timeoutSeconds > 300
  ) {
    throw new PermanentError(
      "API_KEY_REFRESH_TIMEOUT_SECONDS must be a number from 5 to 300.",
    );
  }
  return { mode: MODE, http: HTTP };
}
