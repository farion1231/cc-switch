# Opt-in native Responses compatibility

Some Azure-backed gateways recognize a client's `image_gen.imagegen` namespace as a reserved hosted function with a fixed schema. Others select an Azure API version that predates Responses unless an explicit version is supplied. These can produce HTTP 400 before a model returns usable output.

CC Switch can apply provider-neutral compatibility settings to the native Codex Responses proxy path. The settings below are fields of the provider's JSON `settings_config` object, not fields in Codex's `config.toml` and not a new provider preset:

```json
{
  "baseURL": "https://provider.example/v1",
  "responsesCompatibility": {
    "flattenNamespaces": true,
    "normalizeEncryptedInclude": true,
    "apiVersions": {
      "model-b": "2025-03-01-preview",
      "model-c": "2025-03-01-preview"
    }
  }
}
```

`provider.example`, `model-b`, and `model-c` are illustrative placeholders. Use the real gateway address and the model names after CC Switch resolves local routing. These options are configuration-level knobs; this patch does not add an editor to the UI.

## Namespace conversion

`flattenNamespaces: true` reuses the existing conversion between namespaced client tools and ordinary upstream functions. For example, `image_gen` / `imagegen` becomes `image_gen__imagegen`. Tool schemas, corresponding replayed calls, and tool choice follow the existing conversion rules. Returned JSON and SSE function calls are restored to the client's namespace and name.

The generic opt-in does not enable xAI-only schema or numeric-argument rewrites. Existing third-party late-argument SSE repair remains in place. Missing, false, or invalid values do not opt other gateways in; xAI retains its existing automatic compatibility behavior.

## Per-model API versions

`apiVersions` maps exact resolved upstream model names to version strings. A missing mapping, empty string, or invalid value leaves the request unchanged. A request that already contains `api_version` retains that value. Existing provider-level request-body overrides are applied later and can override this default.

No gateway host, model alias, or API-version string is hardcoded for the generic path. Confirm with your gateway which version it accepts; `2025-03-01-preview` is an example verified in the reported Azure-backed scenarios, not a universal recommendation for every deployment.

## Encrypted-content include selector

`normalizeEncryptedInclude: true` changes the unsupported `output.encrypted_content` selector to `reasoning.encrypted_content` before forwarding. It retains other `include` entries and avoids adding a duplicate `reasoning.encrypted_content` selector. The mainline OpenAI-private-field sanitizer still runs afterwards; with this option disabled, its default removal of `output.encrypted_content` remains in effect. This setting is useful for Azure-backed Responses gateways that reject `output.encrypted_content` with HTTP 400. It is opt-in because different gateways may expose different schemas.

These options run on native Responses forwarding, not on Responses-to-Chat or Responses-to-Anthropic conversion. They do not provision an Azure hosted image deployment. Successful returned function calls do not themselves execute image generation.

## Boundary with native protocol capabilities (#7952)

These settings repair the wire format of individual native Responses requests. They do not declare that a relay implements the official OpenAI protocol. In particular, they do not enable Fast/service tiers, publish official model-catalog capabilities, change authentication or identity headers, enable remote compaction, or bypass the mainline compaction bridge.

PR #7952 proposes `meta.codexOfficialCompatible` for a relay's native protocol capabilities. That is separate from `settings_config.responsesCompatibility`; neither option implies the other. Keep capability declarations in provider metadata and request-format repairs in settings config. A namespace repair must not enable xAI schema/numeric rewriting, including on a Copilot card serving non-Grok models. Existing automatic Copilot/Grok and direct xAI behavior remains model-aware.

All three repair options are independently opt-in. A provider that only needs an API version can omit both boolean options. A relay that supports native compaction does not need namespace flattening unless its own schema requires it. Final naming of the proposed capability option remains subject to maintainer review of #7952; this PR does not add or rename that metadata field.

On current mainline, third-party requests already have OpenAI-private fields removed. With `normalizeEncryptedInclude` disabled, that existing cleanup still applies. With it explicitly enabled, `output.encrypted_content` is converted to `reasoning.encrypted_content` before cleanup; other private fields still follow mainline policy. This selector repair does not grant permission to retain encrypted history or change the compaction policy. Explicit provider body overrides run later and remain authoritative.
