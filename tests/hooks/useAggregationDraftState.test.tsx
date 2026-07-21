import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useAggregationDraftState } from "@/components/providers/forms/hooks/useAggregationDraftState";

describe("useAggregationDraftState", () => {
  it("serializes upstreams + role mapping and generates env", () => {
    const { result } = renderHook(() => useAggregationDraftState());
    const upstreamId = result.current.upstreams[0].id;

    act(() => {
      result.current.updateUpstream(upstreamId, {
        name: "Anthropic",
        baseUrl: " http://upstream.example:15722/ ",
        apiKey: " secret ",
        apiFormat: "anthropic",
        apiKeyField: "ANTHROPIC_API_KEY",
      });
      result.current.updateRole("sonnet", {
        upstreamId,
        model: "claude-opus-4-8",
        supports1m: true,
      });
      result.current.updateRole("default", {
        upstreamId,
        model: "claude-opus-4-8",
      });
    });

    expect(result.current.validate()).toBeNull();

    const config = result.current.toConfig();
    expect(config.upstreams).toEqual([
      {
        id: upstreamId,
        name: "Anthropic",
        baseUrl: "http://upstream.example:15722/",
        apiKey: "secret",
        apiFormat: "anthropic",
        apiKeyField: "ANTHROPIC_API_KEY",
        isFullUrl: undefined,
      },
    ]);
    expect(config.roles?.sonnet).toEqual({
      upstreamId,
      model: "claude-opus-4-8",
      supports1m: true,
    });
    expect(config.roles?.default).toEqual({
      upstreamId,
      model: "claude-opus-4-8",
      supports1m: undefined,
    });

    // env: sonnet 带 [1M]，default → ANTHROPIC_MODEL（无 1M）
    expect(result.current.toEnv()).toEqual({
      ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-opus-4-8[1M]",
      ANTHROPIC_MODEL: "claude-opus-4-8",
    });
  });

  it("clears role bindings when an upstream is deleted", () => {
    const { result } = renderHook(() => useAggregationDraftState());
    const firstId = result.current.upstreams[0].id;

    act(() => {
      result.current.updateUpstream(firstId, {
        baseUrl: "http://first.example",
      });
      result.current.updateRole("sonnet", {
        upstreamId: firstId,
        model: "model-a",
      });
      result.current.addUpstream();
    });

    act(() => {
      result.current.removeUpstream(firstId);
    });

    expect(result.current.roles.sonnet.upstreamId).toBe("");
    expect(result.current.upstreams.some((u) => u.id === firstId)).toBe(false);
  });

  it("requires at least one valid role", () => {
    const { result } = renderHook(() => useAggregationDraftState());
    const upstreamId = result.current.upstreams[0].id;

    act(() => {
      result.current.updateUpstream(upstreamId, {
        baseUrl: "http://valid.example",
      });
      // role points at a non-existent upstream → not counted
      result.current.updateRole("sonnet", {
        upstreamId: "missing",
        model: "model-a",
      });
    });

    expect(result.current.validate()).toBe("no_role");
    expect(result.current.toConfig().roles).toEqual({});
  });

  it("preserves legacy routes until the user explicitly removes them", () => {
    const initial = {
      upstreams: [
        {
          id: "legacy-upstream",
          baseUrl: "http://legacy.example",
          apiKey: "key",
          apiFormat: "openai_chat",
        },
      ],
      routes: [
        {
          model: "grok-*",
          upstreamId: "legacy-upstream",
          upstreamModel: "grok-4.5",
        },
      ],
    };

    const { result } = renderHook(() => useAggregationDraftState(initial));

    expect(result.current.roles.sonnet.model).toBe("");
    expect(result.current.validate()).toBeNull();
    expect(result.current.toConfig().routes).toEqual(initial.routes);

    act(() => result.current.removeLegacyRoute(0));

    expect(result.current.toConfig().routes).toBeUndefined();
    expect(result.current.validate()).toBe("no_role");
  });

  it("rehydrates when a different provider is edited", () => {
    const first = {
      upstreams: [
        {
          id: "first",
          baseUrl: "http://first.example",
          apiKey: "",
          apiFormat: "openai_responses",
          apiKeyField: "ANTHROPIC_AUTH_TOKEN",
        },
      ],
      roles: { sonnet: { upstreamId: "first", model: "model-a" } },
    };
    const second = {
      upstreams: [
        {
          id: "second",
          baseUrl: "http://second.example",
          apiKey: "key",
          apiFormat: "anthropic",
          apiKeyField: "ANTHROPIC_API_KEY",
        },
      ],
      roles: { sonnet: { upstreamId: "second", model: "model-b" } },
    };

    const { result, rerender } = renderHook(
      ({ initial }) => useAggregationDraftState(initial),
      { initialProps: { initial: first } },
    );

    rerender({ initial: second });

    expect(result.current.upstreams[0]).toMatchObject({
      id: "second",
      apiKeyField: "ANTHROPIC_API_KEY",
    });
    expect(result.current.roles.sonnet.model).toBe("model-b");
  });
});
