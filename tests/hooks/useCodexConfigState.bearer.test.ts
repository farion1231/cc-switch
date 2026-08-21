import { act, renderHook } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { useCodexConfigState } from "@/components/providers/forms/hooks/useCodexConfigState";

// The hook is also used for stored providers and presets. Those inputs keep
// auth.OPENAI_API_KEY as their canonical credential; only EditProviderDialog's
// current-live boundary may lift a bearer into auth (#6414).
describe("useCodexConfigState bearer-token precedence", () => {
  it("keeps stored auth authoritative when config also has a bearer", () => {
    const initialData = {
      settingsConfig: {
        auth: { OPENAI_API_KEY: "sk-db-key" },
        config:
          'model_provider = "custom"\nmodel = "model-A"\nexperimental_bearer_token = "sk-leftover-live-key"\n',
      },
    };

    const { result } = renderHook(() => useCodexConfigState({ initialData }));

    expect(result.current.codexApiKey).toBe("sk-db-key");
    const savedAuth = JSON.parse(result.current.codexAuth);
    expect(savedAuth.OPENAI_API_KEY).toBe("sk-db-key");
  });

  it("falls back to the bearer for display without mutating an auth object that has no key", () => {
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
    expect(savedAuth.OPENAI_API_KEY).toBeUndefined();
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

  it("keeps preset auth authoritative when reset config contains a bearer", () => {
    const { result } = renderHook(() => useCodexConfigState({}));

    act(() => {
      result.current.resetCodexConfig(
        { OPENAI_API_KEY: "sk-preset-key" },
        'model_provider = "custom"\nexperimental_bearer_token = "sk-leftover-key"\n',
      );
    });

    expect(result.current.codexApiKey).toBe("sk-preset-key");
    const savedAuth = JSON.parse(result.current.codexAuth);
    expect(savedAuth.OPENAI_API_KEY).toBe("sk-preset-key");
  });
});
