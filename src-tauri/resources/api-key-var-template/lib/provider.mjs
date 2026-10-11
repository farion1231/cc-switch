import { PermanentError, RetryableError } from "./contract.mjs";

export async function getDemoKey() {
  // This is deliberately not a real credential. It only proves the contract.
  return {
    apiKey: `demo-not-a-real-key-${Date.now().toString(36)}`,
    expiresAt: new Date(Date.now() + 60 * 60 * 1000).toISOString(),
    refreshAfter: Math.floor(Date.now() / 1000) + 30 * 60,
    message: "Demo key only; configure HTTP mode before using a real provider.",
  };
}

export async function getHttpKey(http) {
  let url;
  try {
    url = new URL(http.url);
  } catch {
    throw new PermanentError("API_KEY_REFRESH_URL must be a valid URL.");
  }
  if (url.hostname.endsWith(".invalid")) {
    throw new PermanentError(
      "Replace the example URL before enabling HTTP mode.",
    );
  }
  if (
    url.protocol !== "https:" &&
    url.hostname !== "127.0.0.1" &&
    url.hostname !== "localhost"
  ) {
    throw new PermanentError("Use HTTPS for a remote key refresh endpoint.");
  }
  const token = process.env[http.tokenEnvName];
  if (!token) throw new PermanentError(`${http.tokenEnvName} is not set.`);

  const controller = new AbortController();
  const timer = setTimeout(
    () => controller.abort(),
    http.timeoutSeconds * 1000,
  );
  try {
    const response = await fetch(url, {
      method: "POST",
      headers: {
        authorization: `Bearer ${token}`,
        "content-type": "application/json",
        accept: "application/json",
      },
      body: JSON.stringify(http.requestBody),
      signal: controller.signal,
      redirect: "manual",
    });
    if (response.status === 401 || response.status === 403) {
      throw new PermanentError("Provider rejected the refresh credential.");
    }
    if (
      response.status === 408 ||
      response.status === 429 ||
      response.status >= 500
    ) {
      throw new RetryableError(
        `Provider is temporarily unavailable (HTTP ${response.status}).`,
      );
    }
    if (!response.ok)
      throw new PermanentError(
        `Provider rejected the request (HTTP ${response.status}).`,
      );

    let body;
    try {
      body = await response.json();
    } catch (error) {
      if (error instanceof SyntaxError) {
        throw new PermanentError("Provider returned invalid JSON.");
      }
      throw error;
    }
    if (!body || typeof body !== "object" || Array.isArray(body)) {
      throw new PermanentError("Provider response must be a JSON object.");
    }
    // Response mapping point: adapt these field names for your provider.
    return {
      apiKey: body.apiKey,
      expiresAt: body.expiresAt,
      refreshAfter: body.refreshAfter,
      message: body.message,
    };
  } catch (error) {
    if (error instanceof PermanentError || error instanceof RetryableError)
      throw error;
    if (error?.name === "AbortError")
      throw new RetryableError("Provider request timed out.");
    throw new RetryableError("Provider request failed due to a network error.");
  } finally {
    clearTimeout(timer);
  }
}
