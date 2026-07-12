# xAI Grok OAuth プロバイダーガイド

> このガイドは、CC Switch で Claude Code / Claude Desktop から利用する xAI Grok のマネージド OAuth プロバイダー向けです。Grok Build の OAuth/API 権限を持つ SuperGrok または X Premium+ などの対象アカウントを想定しています。利用可否は xAI が管理しており、ログイン成功だけでは推論権限は保証されません。

## 概要

`xAI Grok OAuth (SuperGrok / X Premium+)` プリセットを使用すると、プロバイダー設定に静的な xAI API Key を保存せず、Claude Code または Claude Desktop のリクエストを CC Switch 経由で xAI に転送できます。

既定値：

- プロバイダー種別：`xai_oauth`
- Base URL：`https://api.x.ai/v1`
- アップストリームパス：`/v1/responses`
- API 形式：OpenAI Responses
- 既定モデル：`grok-build-0.1`
- 認証ストア：CC Switch のアプリ設定ディレクトリにある `xai_oauth_auth.json`

実際の access token はローカルプロキシが転送するときだけ解決されます。Claude の Live 設定には `PROXY_MANAGED` プレースホルダーのみが書き込まれます。

## 前提条件

- CC Switch のローカルルーティングサービスが利用可能であること。
- Claude Code または Claude Desktop が CC Switch に設定済みであること。
- xAI アカウントに Grok Build OAuth/API 権限があること。
- CC Switch と同じマシンでループバックコールバックを完了できるブラウザーがあること。

現在の実装はブラウザー OAuth + PKCE とローカルループバックを使用し、device-code フローは含みません。リモートまたはヘッドレス環境では、ブラウザーから CC Switch を実行しているマシンのコールバックへ到達できる必要があります。それができない場合は静的 xAI API Key プロバイダーを使用してください。

## プロバイダーの追加

1. Claude または Claude Desktop のプロバイダーフォームで `xAI Grok OAuth (SuperGrok / X Premium+)` を選択します。
2. xAI ログインボタンを押し、ブラウザーで認証と同意を完了します。
3. フォームでログイン済み xAI アカウントを選択します。
4. プロバイダーを保存します。
5. 対象アプリのローカルルーティングを有効にして、このプロバイダーへ切り替えます。
6. 古い設定が読み込まれている場合は Claude Code または Claude Desktop を再起動します。

保存されるのは `authProvider = "xai_oauth"` と選択したアカウント ID だけで、bearer token は保存されません。

## Live 設定とルーティング

- `ANTHROPIC_BASE_URL` は設定された xAI ルートを指します。
- 既定モデルは `grok-build-0.1` です。
- `ANTHROPIC_API_KEY` と、Copilot 以外のマネージド認証で使う `ANTHROPIC_AUTH_TOKEN` には `PROXY_MANAGED` が入ります。
- 実際の token は転送時にローカル認証ストアから読み出されます。

認証ストアはアプリ設定ディレクトリ内の JSON ファイルです。Unix では所有者だけが読み書きできる `0600` で保存されますが、アプリケーションレベルの暗号化は行われません。

Claude のマネージド認証プロバイダーを書き込むと、既存の GitHub Copilot と Codex OAuth の Live 認証環境も正規化されます。古い API Key 変数を削除し `PROXY_MANAGED` に置き換えることで、すべてのマネージドプロバイダーを同じテイクオーバー契約に揃えます。

xAI bearer token は `https://api.x.ai` にだけ注入されます。他のホストではマネージド認証ガードが送信前に失敗させます。

## 保存、更新、ログインのキャンセル

アカウントメタデータと refresh token は `xai_oauth_auth.json` に保存されます。デバッグ出力では access token、refresh token、ID token、認可コード、token endpoint の応答を秘匿します。

転送前に access token を確認し、期限が近い場合は refresh token で更新します。アカウントが存在しない、または削除済みの場合は、アップストリームへ送信する前に認証エラーになります。

ブラウザーログイン中は固定コールバック `127.0.0.1:56121` を使用します。キャンセル時にはリスナーを即座に解放し、新しいログイン開始時にも放置されたリスナーを置き換えます。他のアプリがポートを使用している場合は、認可 URL を開く前に明確なポート競合エラーを表示します。

## 403 と権限エラー

OAuth ログインが成功しても、推論時に xAI が `403` を返す場合があります。サブスクリプション、API 権限、地域制限、段階的ロールアウトなどが原因です。

- xAI がサポートするクライアントで Grok Build を利用できるか確認します。
- 再ログイン後に小さなリクエストを送り、期限切れを除外します。
- OAuth API 権限がない場合は通常の xAI API Key プロバイダーを使用します。

これはプロバイダー消失や設定破損ではなく、xAI アカウント/API 権限の問題として扱います。

## セキュリティ特性

- Claude の Live 設定に実際の xAI OAuth token を保存しません。
- `PROXY_MANAGED` などのプレースホルダーを送信前に拒否します。
- xAI token の注入先を `https://api.x.ai` に固定します。
- 保存前にログイン済みで利用可能な xAI アカウントを要求します。
- ブラウザー認証のキャンセルまたは拒否後、すぐに再試行できます。

## 手動確認

- 静的 API Key を入力せずログインして保存できること。
- Live 設定に `PROXY_MANAGED` だけがあり、実 token がないこと。
- `https://api.x.ai/v1/responses` に転送されること。
- バインド済みアカウント削除後は上流アクセス前に失敗すること。
- ログインを一度キャンセルし、直ちに再ログインできること。
- xAI から切り替えた後も他の Claude プロバイダーが残ること。

## 参考資料

- [xAI Grok Build 0.1 発表](https://x.ai/news/grok-build-0-1)
- [Hermes Agent xAI Grok OAuth ガイド](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/guides/xai-grok-oauth.md)
- [OpenClaw xAI ドキュメント](https://docs.openclaw.ai/providers/xai)
- [OpenCode プロバイダードキュメント](https://opencode.ai/docs/providers/)
- [CC Switch xAI Grok OAuth 実装契約](../research/xai-grok-oauth-contract.md)
