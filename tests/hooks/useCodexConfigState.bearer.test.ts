import { renderHook } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { useCodexConfigState } from "@/components/providers/forms/hooks/useCodexConfigState";

// Regression for #6414: multiple Codex providers share a base URL but use
// different API keys. In bearer-token mode (preserveCodexOfficialAuthOnSwitch)
// the active key lives in config.toml's experimental_bearer_token while the
// shared auth.json may hold ANOTHER provider's stale key. The edit form must
// treat the per-provider bearer as authoritative — both for display and for
// what gets saved — so keys don't silently converge across providers.
describe("useCodexConfigState bearer-token auth reconciliation", () => {
  it("lifts the config bearer token over a stale shared auth.json key", () => {
    const initialData = {
      settingsConfig: {
        // Stale shared slot: another provider's key, preserved across switches.
        auth: { OPENAI_API_KEY: "sk-stale-other-provider" },
        config:
          'model_provider = "custom"\nmodel = "model-A"\nexperimental_bearer_token = "sk-real-key-A"\n',
      },
    };

    const { result } = renderHook(() => useCodexConfigState({ initialData }));

    expect(result.current.codexApiKey).toBe("sk-real-key-A");

    // The saved auth (codexAuth) must also carry the real key, otherwise
    // onSubmit would persist the stale key back into the provider's DB record.
    const savedAuth = JSON.parse(result.current.codexAuth);
    expect(savedAuth.OPENAI_API_KEY).toBe("sk-real-key-A");
  });

  it("preserves an empty auth.json key when the config carries the bearer", () => {
    // Official-login preservation leaves auth.json with tokens but no API key;
    // the third-party key is only in the bearer. The form must still surface
    // the bearer as the editable key and persist it into auth.OPENAI_API_KEY.
    const initialData = {
      settingsConfig: {
        auth: { tokens: { account_id: "acc" } },
        config:
          'model_provider = "custom"\nmodel = "model-A"\nexperimental_bearer_token = "sk-real-key-A"\n',
      },
    };

    const { result } = renderHook(() => useCodexConfigState({ initialData }));

    expect(result.current.codexApiKey).toBe("sk-real-key-A");
    const savedAuth = JSON.parse(result.current.codexAuth);
    expect(savedAuth.OPENAI_API_KEY).toBe("sk-real-key-A");
    // Other auth fields (the preserved login material) are kept intact.
    expect(savedAuth.tokens).toEqual({ account_id: "acc" });
  });

  it("does not reconcile when the config has no bearer (default mode / manual live edits)", () => {
    // Default mode keeps the active key in auth.json; the config has no bearer.
    // A user's manual live edit (auth.json = "live-key") must be preserved
    // exactly — this is the intentional backfill/capture behavior, and the
    // reconciliation must not touch it.
    const initialData = {
      settingsConfig: {
        auth: { OPENAI_API_KEY: "live-key" },
        config: 'model_provider = "custom"\nmodel = "model-A"\n',
      },
    };

    const { result } = renderHook(() => useCodexConfigState({ initialData }));

    expect(result.current.codexApiKey).toBe("live-key");
    const savedAuth = JSON.parse(result.current.codexAuth);
    expect(savedAuth.OPENAI_API_KEY).toBe("live-key");
  });

  it("is a no-op when the bearer already matches auth.OPENAI_API_KEY", () => {
    const initialData = {
      settingsConfig: {
        auth: { OPENAI_API_KEY: "sk-real-key-A" },
        config:
          'model_provider = "custom"\nmodel = "model-A"\nexperimental_bearer_token = "sk-real-key-A"\n',
      },
    };

    const { result } = renderHook(() => useCodexConfigState({ initialData }));

    expect(result.current.codexApiKey).toBe("sk-real-key-A");
    const savedAuth = JSON.parse(result.current.codexAuth);
    expect(savedAuth.OPENAI_API_KEY).toBe("sk-real-key-A");
  });
});
