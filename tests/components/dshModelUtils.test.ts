import { describe, expect, it } from "vitest";
import {
  deriveDshCredentialRef,
  parseDshCapacity,
  validateDshApiKey,
  validateDshModels,
  validateDshRoute,
} from "@/components/dsh/dshModelUtils";
import { isDshConflictError, readDshError } from "@/lib/api/dsh";

describe("DSH editor validation", () => {
  it("accepts route ids that can become credential references", () => {
    expect(validateDshRoute("acme-gateway")).toBeUndefined();
    expect(validateDshRoute("1-acme")).toBeTruthy();
    expect(validateDshRoute("Acme")).toBeTruthy();
    expect(validateDshRoute("acme_gateway")).toBeTruthy();
  });

  it("derives a stable credential reference without storing a key", () => {
    expect(deriveDshCredentialRef("acme-gateway")).toBe("ACME_GATEWAY_API_KEY");
    expect(deriveDshCredentialRef("gateway.v2")).toBe("GATEWAY_V2_API_KEY");
  });

  it("treats an empty API-key field as keep/no credential", () => {
    expect(validateDshApiKey("")).toBeUndefined();
    expect(validateDshApiKey("   ")).toBeTruthy();
    expect(validateDshApiKey("'secret'")).toBeTruthy();
    expect(validateDshApiKey("KEY=secret")).toBeTruthy();
    expect(validateDshApiKey("sk-live-123")).toBeUndefined();
  });

  it("validates unique model ids and positive capacities", () => {
    expect(validateDshModels([])).toEqual({
      index: 0,
      messageKey: "dsh.validation.modelRequired",
    });
    expect(validateDshModels([{ id: "chat" }, { id: "chat" }])).toMatchObject({
      index: 1,
    });
    expect(validateDshModels([{ id: "chat", contextWindow: 0 }])).toMatchObject(
      { index: 0 },
    );
    expect(
      validateDshModels([
        { id: "chat", contextWindow: 128_000, maxTokens: 4_096 },
      ]),
    ).toBeUndefined();
  });

  it("parses human-friendly decimal capacities", () => {
    expect(parseDshCapacity("256K")).toBe(256_000);
    expect(parseDshCapacity("1.5M")).toBe(1_500_000);
    expect(parseDshCapacity(" 4096 ")).toBe(4096);
    expect(parseDshCapacity("0")).toBeUndefined();
    expect(parseDshCapacity("1MiB")).toBeUndefined();
  });

  it("parses structured Rust errors serialized as JSON strings", () => {
    const error = JSON.stringify({
      code: "stale-revision",
      message: "settings changed",
      secret: "must-not-be-returned",
    });

    expect(readDshError(error)).toEqual({
      code: "stale-revision",
      message: "settings changed",
      detail: undefined,
      namespace: undefined,
      path: undefined,
    });
    expect(isDshConflictError(error)).toBe(true);
  });
});
