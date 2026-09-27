# Codex で自前 LLM ゲートウェイ（llm-gateway-go）を使う：公式ログイン維持 + 複数モデル共存

> 対象バージョン：CC Switch v3.20.4 以上。本記事は 2026-09-28 の E2E 検証ログ（CODEX_HOME = `~/.codex`、実インスタンス）に基づきます。スクリーンショットは未定稿。

## 何を解決するか

OpenAI 互換のゲートウェイ（[llm-gateway-go](https://github.com/kaixuan/llm-gateway-go) など）を社内で立てて、複数のプロバイダ（Anthropic / GLM / Kimi / MiniMax / DeepSeek …）を単一の base_url で公開しているチームがあります。彼らは Codex をこのゲートウェイに繋ぎたい、しかも **ChatGPT 公式ログイン状態は失いたくない**（公式アプリ / リモート操作 / 公式プラグインがそれを必要とする）。

CC Switch v3.20.1+ はすでにサードパーティ API キーを `config.toml` のみに書き込む設計です。本記事ではゲートウェイ固有のケースに焦点を当てます：

1. 単一 Codex プロバイダで複数非 OpenAI モデルをリストアップし、TUI のモデルピッカーに全部出す。
2. OpenAI 公式に戻しても副作用なしで動く。
3. プロバイダ切替で `approval_policy` / `sandbox_mode` / `agents` などの共通設定を巻き込まない。

## 結論

1. CC Switch を v3.20.4 以上にアップグレード（これ未満は `[model_providers.custom]` の往復で壊れる）。
2. Codex タブで「開轩 LLM 网关」または「本地 LLM 网关 (8782)」プリセットを選択。なければ `設定 → カスタムプリセット` で追加するか、`src/config/codexProviderPresets.ts` に PR。
3. ゲートウェイの API Key を入力 → 保存。CC Switch は Key を `config.toml` にのみ書き、`auth.json` には触れないため ChatGPT OAuth キャッシュは無傷。
4. Codex を一度起動し、`~/.codex/cc-switch-model-catalog.json` を解析させる（モデルピッカーに反映）。
5. 4 プロバイダ（`default` / `OpenAI Official` / kxpms-gateway / local-gateway-8782）は **独立行**として並ぶ。UI でクリック切替、互いに破壊しない。

## アーキテクチャ

```
┌──────────────────────────────────────────────────────────┐
│ CC Switch が 4 つの codex プロバイダを維持：              │
│  • default          → ChatGPT OAuth（公式）              │
│  • OpenAI Official  → API キー プレースホルダ             │
│  • 开轩 LLM 网关     → https://llm.kxpms.cn/v1（複数モデル） │
│  • 本地 LLM 网关(8782)→ http://localhost:8782/v1（複数モデル）│
│                                                          │
│ UI 切替：DB is_current 反転 → live config 上書き          │
│      model_provider / model / model_catalog_json / auth  │
└──────────────────────────────────────────────────────────┘
```

## E2E 検証（2026-09-28）

合格基準：`codex exec --json` のイベントストリームに `item.completed(type=agent_message)`（テキストが `TOKEN-<model>` と完全一致）と `turn.completed` の両方が出ること。

| ゲートウェイ | モデル | 結果 |
|---|---|---|
| kxpms | claude-opus-5 | **PASS** |
| kxpms | claude-opus-4-8 | **PASS** |
| 8782 | claude-opus-5 | **PASS** |
| 8782 | claude-sonnet-5 | **PASS** |

**既知の制限（ゲートウェイ側であり設定の問題ではない）**：

- `glm-5.2` / `kimi-k3` は採取ウィンドウでレート制限（8782 側 6 分で 264 回の `rate_limit_exceeded`、kxpms 側は 140 秒のテールレイテンシ）。
- `claude-sonnet-5` / `claude-opus-5-5` は kxpms 上でハング（curl 100 秒、ゼロバイト）— 上流認証情報の問題。
- `deepseek-v4-pro` / `grok-4.7` → 503 provider_unavailable、`qwen*.max` / `gemini-3.x` → 503 no_candidate。

`[profiles.kx-*]` は **codex 0.158 で拒否される** — `~/.codex/<name>.config.toml` に移行済み。`config.toml` にレガシー `[profiles.xx]` が残っていると `--profile xx` は即失敗：

```
Error loading config.toml: --profile `xx` cannot be used while
config.toml contains legacy `profile = "xx"` or `[profiles.xx]`
```

この契約は `codexProviderPresets.gateways.test.ts` で固定。

## 前提条件

1. CC Switch v3.20.4 以上。
2. Codex CLI 0.158 以上（`codex --version`）。
3. ゲートウェイの URL + API Key。**実キーをチケットに貼らないこと**。
4. ゲートウェイが OpenAI Responses API（`/v1/responses`）を公開している（Chat Completions だけでは不可）。

## ステップ 1：まず ChatGPT 公式にログイン

`OpenAI Official` または `default` に切り替え、Codex を起動して公式ログインを完了（Free 枠で可）。OAuth は `auth.json` に保存され、後でゲートウェイに切り替えても上書きされない。

## ステップ 2：ゲートウェイプロバイダを追加

Codex タブで「开轩 LLM 网关」または「本地 LLM 网关 (8782)」を選択 → API Key を入力 → 保存。これで Key は `config.toml` にのみ書かれる。

## ステップ 3：Codex にモデルディレクトリを食べさせる

`codex exec --skip-git-repo-check -m claude-opus-5 "ping"` を 1 回実行し、`~/.codex/cc-switch-model-catalog.json` を Codex にキャッシュさせる。`slug` が `--model` の候補になる。

## ステップ 4：OpenAI に戻して副作用がないか確認

CC Switch で `OpenAI Official` をクリック → `codex exec -m gpt-5.6-sol "ping"` が `chatgpt.com/backend-api` に普通に届くこと（4 モデルとも 12 秒以内に認証応答、設定は無傷）。終わったらゲートウェイに戻す。

## トラブルシューティング

| 症状 | 確認点 |
|---|---|
| 切替後に ChatGPT ログイン消失 | cc-switch < v3.20.1（サードパーティキーが `auth.json` を上書き）。アップグレード |
| `[profiles.xx]` レガシー警告 | `[profiles.xx]` を `~/.codex/xx.config.toml` へ移動（codex 0.158 release notes 参照） |
| `--model glm-5.2` が 503 | 上流の認証切れ or レート制限。`curl -m 10 -X POST <base>/v1/responses` で直検証 |
| `Reconnecting 1/5...5/5` ばかり | `[model_providers.custom]` 下に `request_max_retries=2` を入れない — 0.158 は無視する（bogus-host プローブで実証済） |

## 実装規約（プリセット作者向け）

`codexProviderPresets.ts` に自前ゲートウェイを追加する場合：

- `apiFormat: "openai_responses"`（CC Switch の Responses→Chat 変換を経由せず直結）。
- `modelCatalog` で **全モデル** に `contextWindow` / `inputModalities` / `reasoningLevels` を明示する（バックエンドに推測させない）。
- デフォルト `model = "<catalog 先頭の model>"`。
- `[profiles.kx-*]` を書かない（0.158 で拒否される）。
- `request_max_retries` / `stream_max_retries` / `stream_idle_timeout_ms` を書かない（0.158 は `[model_providers.custom]` 層で消費しない — 偽機能と確認済）。

回帰テスト：静的アサーション `src/config/codexProviderPresets.gateways.test.ts` + 実ディレクトリ書き出し + 逆パース往復の `src-tauri/src/codex_config.rs::tests::self_hosted_gateway_presets_round_trip_through_catalog_pipeline`。