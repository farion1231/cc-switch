# Opt-in native Responses compatibility

Some Azure-backed gateways recognize a client's `image_gen.imagegen` namespace as a reserved hosted function with a fixed schema. Others select an Azure API version that predates Responses unless an explicit version is supplied. These can produce HTTP 400 before a model returns usable output.

CC Switch can apply provider-neutral compatibility settings to the native Codex Responses proxy path. The settings below are fields of the provider's JSON `settings_config` object, not fields in Codex's `config.toml` and not a new provider preset:

```json
{
  "baseURL": "https://provider.example/v1",
  "responsesCompatibility": {
    "flattenNamespaces": true,
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

These options run on native Responses forwarding, not on Responses-to-Chat or Responses-to-Anthropic conversion. They do not provision an Azure hosted image deployment. Successful returned function calls do not themselves execute image generation.
