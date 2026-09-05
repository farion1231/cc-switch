# Built-in model pricing

Last checked: **2026-09-05**. The built-in catalog contains 210 model IDs,
including legacy names and reasoning-effort aliases. Prices are USD per million
text tokens, in the order input / output / cache read / cache creation in
`src-tauri/src/database/schema.rs`.

## Rate selection

- Use the provider's standard synchronous API rate and shortest context tier.
  Batch, tool calls, cache storage, audio and generated media have
  different billing units or multipliers and are not additional token rows.
- DeepSeek V4 retains **peak** prices; one row cannot represent its peak/off-peak
  schedule. Qwen uses **Singapore / International**, not converted Beijing or
  Global-region prices. Qwen cache reads use implicit caching where listed,
  otherwise explicit cache reads; the schema cannot distinguish both kinds.
- Sol uses the current promotional rate (at least through **2026-11-21**).
  Gemini 3.6/3.7/3.8 Flash uses the introductory rate through **2026-12-31**.
  GLM-5.3-Flash uses the 50% offer through **2026-09-09 24:00 UTC+8**.
  These are dated defaults, not an automatic promotion-expiry scheduler.
- Preserve historical entries when the vendor no longer publishes their rate or
  the ID belongs to an unverified reseller alias. Do not replace them with a
  similarly named model from another host or a subscription's free allowance.
  This applies in particular to old Kimi K2, Grok 3, MiniMax `lightning`,
  `gpt-5-codex-mini`, MiMo V2 Flash and legacy Mistral variants. Existing
  StepFun, Doubao and Hunyuan estimates are retained where an equivalent current
  public USD rate could not be confirmed. These entries are **not** claimed as
  freshly verified official tariffs.

## Primary sources

| Family | Source and refresh notes |
| --- | --- |
| OpenAI | [Pricing](https://developers.openai.com/api/docs/pricing): add GPT-6 Astra; update Sol and its existing aliases. [GPT-5.5 Pro](https://developers.openai.com/api/docs/models/gpt-5.5-pro) adds the missing Pro row. |
| OpenAI legacy | Correct [o3-mini](https://developers.openai.com/api/docs/models/o3-mini), [o1-mini](https://developers.openai.com/api/docs/models/o1-mini), [GPT-5.1 Codex Mini](https://developers.openai.com/api/docs/models/gpt-5.1-codex-mini) and [Codex Mini](https://developers.openai.com/api/docs/models/codex-mini-latest); include the exact `codex-mini-latest` ID. |
| Anthropic | [Pricing](https://platform.claude.com/docs/en/about-claude/pricing): retain Fable/Mythos 5.1 and Sonnet 5 prices; the previously proposed Sonnet 5 increase was cancelled. |
| Google | [Gemini API pricing](https://ai.google.dev/gemini-api/docs/pricing): add Gemini 3.8 Flash; correct 3.6 Flash to its current introductory rate. |
| DeepSeek | [Pricing](https://api-docs.deepseek.com/quick_start/pricing/): retain peak rates, add Pro 0813 and Flash Vision Exp. |
| MiniMax | [Pay as you go](https://platform.minimax.io/docs/guides/pricing-paygo): correct M2/M2.1/M2.5, including cache writes; add the exact M2.1/M2.5 highspeed IDs. |
| MiMo | [Pricing](https://mimo.mi.com/docs/en-US/price/pay-as-you-go) and [model catalog](https://mimo.mi.com/): correct V2.5 output and add V2.5 Pro Ultraspeed. Cache writes are temporarily free. |
| Z.AI | [Pricing](https://docs.z.ai/guides/overview/pricing): add GLM-5.3, 5.3 Flash, 4.7 Flash and 4.7 FlashX. |
| Qwen | [Pricing](https://www.alibabacloud.com/help/en/model-studio/model-pricing) and individual model pages below: refresh Singapore rates and cache prices; retain 235B's thinking-mode estimate. |
| xAI | [Pricing](https://docs.x.ai/developers/pricing): current Grok rates match; add the exact 4.20 multi-agent ID. |
| Mistral | [Pricing](https://docs.mistral.ai/inference/pricing): correct Medium 3.5 cache read and Small 4 input/output/cache read. |
| Cohere | [Command A](https://docs.cohere.com/docs/command-a): existing input/output match. No invented public tariff for sales-only models. |

Qwen model pages checked for region and cache details:
[3.8 Max](https://www.alibabacloud.com/help/en/model-studio/qwen3-8-max),
[3.7 Max](https://www.alibabacloud.com/help/en/model-studio/qwen3-7-max),
[3.7 Plus](https://www.alibabacloud.com/help/en/model-studio/qwen3-7-plus),
[3.6 Plus](https://www.alibabacloud.com/help/en/model-studio/qwen3-6-plus),
[3.6 Flash](https://www.alibabacloud.com/help/en/model-studio/qwen3-6-flash),
[3.5 Plus](https://www.alibabacloud.com/help/en/model-studio/qwen3-5-plus),
[Coder Plus](https://www.alibabacloud.com/help/en/model-studio/qwen3-coder-plus),
[Coder Flash](https://www.alibabacloud.com/help/en/model-studio/qwen3-coder-flash),
[Coder Next](https://www.alibabacloud.com/help/en/model-studio/qwen3-coder-next),
[235B-A22B](https://www.alibabacloud.com/help/en/model-studio/qwen3-235b-a22b).

## Updating existing installations

Seeding inserts missing IDs. Guarded repairs change an existing row only when
all four prices match a known old default. Custom prices remain intact, and
`model-pricing.json` overrides are reapplied by the existing startup flow.
Historical cache-write repairs run before the latest Terra/Luna/Sol corrections
so upgrades reach the current rate in one startup.

Base price changes affect future cost calculations; they do not rewrite stored
request costs or constitute a provider billing statement. Models.dev sync is a
separate optional user setting and may select a different host's price.

## Fast request pricing

Schema v20 records a one-time tier-pricing version. Observed response tiers take
precedence over request settings; when only request settings exist, costs are
API-equivalent estimates based on those settings, not confirmed billed charges.
Fast badges are green. Hovering the cost cell shows the stored input, output, cache
read and cache creation costs, their effective per-million prices, and the
provider multiplier. A zero-token component has no inferred unit price.

All models use configured rates without any automatic context-length surcharge.
The following fixed Fast factors apply at every context length, including with
custom rates. Input factors also apply to cache reads and writes. This local
estimation policy does not reproduce providers' long-context billing tiers.
The base Fast factors were checked on 2026-09-05 against [OpenAI pricing](https://developers.openai.com/api/docs/pricing)
and [Claude Fast mode](https://platform.claude.com/docs/en/build-with-claude/fast-mode).

| Model | Fixed input / output factor |
| --- | --- |
| GPT-6 Astra, GPT-5.6 Sol/Terra/Luna | 2 / 2 |
| GPT-5.5 | 2.5 / 2.5 |
| GPT-5.4 | 2 / 2 |
| GPT-5.4 Mini, GPT-5.3 Codex, GPT-5.2, GPT-5.1 | 2 / 2 |
| Claude Opus 5, Opus 4.8 | 2 / 2 |

OpenAI recognizes `fast` and `priority`; Claude requires `speed: fast`, including
`usage.speed` from responses and newly imported Claude sessions. Claude's
`priority` tier alone does not activate Fast pricing. Standard fallback responses
retain standard rates. Claude Fast rates cover the full context window.
No generic surcharge is inferred for other models or unverified aliases; their
configured base prices remain in use. OpenAI exact IDs and dated snapshots are
recognized. Cache TTL distinctions remain limited by the existing single cache
creation field.

Retained historical Fast rows are repaired once from their stored component
costs and multiplier, preserving original rates, tokens and timestamps. Pricing
version 2 removes the earlier context-dependent Fast adjustment without applying
Fast twice. Positive CLI-reported totals remain authoritative. This
also runs after Codex metadata enrichment. Inconsistent or zero-cost records
are left for the existing price-backfill path; already pruned rollups cannot be
retrofitted with per-request tier information. Back up the database before
upgrading; older builds cannot open schema v20.
