# Explicit service tiers for Codex model catalogs

Codex filters `service_tier` out of requests when the selected model's catalog
does not advertise that tier. Setting `service_tier = "priority"` alone therefore
does not enable Fast for a custom model whose `service_tiers` is empty.

For an upstream that supports it, declare the capability in the provider's
`settingsConfig.modelCatalog`:

```json
{
  "models": [
    {
      "model": "relay/gpt-6.1-sol",
      "serviceTiers": [
        {
          "id": "priority",
          "name": "Fast",
          "description": "Priority processing"
        }
      ]
    }
  ]
}
```

The generated catalog retains `service_tiers` and derives
`additional_speed_tiers: ["fast"]` for the `priority` tier. **Fast** is the UI
name; **priority** is the value sent in `service_tier`. Editing, importing,
switching and publishing Stack models retain this explicit model declaration.

This advertises a capability without selecting it. Use Codex's `/fast` command
or `service_tier = "priority"` to select Fast. An optional per-model
`defaultServiceTier` can select a declared tier by default; absent declarations
do not inherit an official model's default. `"default"` explicitly selects the
standard tier. CamelCase and snake_case capability keys are accepted.

Omit `serviceTiers`, or set it to `[]`, for models without verified upstream
support. These fields do not enable WebSockets, change protocol conversion,
or guarantee that the upstream honors priority processing. Check the upstream's
completed response when verifying the actual tier.
