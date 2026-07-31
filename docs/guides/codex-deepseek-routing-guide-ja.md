# Codex で DeepSeek V4 Pro を使う：CC Switch ローカルルーティングガイド

> **重要：**内蔵の `DeepSeek` メインプリセットは DeepSeek V4 Flash のみを含み、ネイティブ Responses API へ直接接続するため、**ローカルルーティングは不要**です。このガイドは独立した `DeepSeek V4 Pro` プリセットだけを対象とします。V4 Pro は現在 Chat Completions を使用するため、CC Switch によるローカルでのプロトコル変換が必要です。

> 保存済みの DeepSeek プロバイダーは、内蔵プリセットの変更に伴って自動移行されません。Flash のネイティブ Responses または新しい Pro Chat 設定を使用するには、対応するプリセットを選び直すか、新しいプロバイダーを作成してください。

## 最初に正しいプリセットを選ぶ

| プリセット | モデル | 上流プロトコル | ローカルルーティング |
|------------|--------|----------------|----------------------|
| `DeepSeek` | `deepseek-v4-flash` | ネイティブ Responses | 不要 |
| `DeepSeek V4 Pro` | `deepseek-v4-pro` | Chat Completions | 必要 |

Flash を使用する場合は、`DeepSeek` を選択して API Key を入力し、保存するだけです。Codex モデルカタログには function calling、freeform `apply_patch`、テキスト Web Search、並列ツール呼び出し、`low` / `high` / `max` の推論レベルがすでに宣言されています。

以降の手順は `DeepSeek V4 Pro` のみに適用されます。

## V4 Pro にローカルルーティングが必要な理由

Codex CLI は OpenAI Responses API を使用しますが、V4 Pro プリセットは現在 Chat Completions を使用します。CC Switch は Codex からのリクエストを Responses のまま受け取り、双方向にプロトコルを変換します：

1. Codex の引き継ぎを有効にすると、live 設定は `http://127.0.0.1:15721/v1` を指し、`wire_api = "responses"` は維持されます。
2. `DeepSeek V4 Pro` プリセットは Chat Completions 形式として設定されています。
3. ローカルルートが Responses リクエストを Chat Completions に変換し、DeepSeek へ送信します。
4. DeepSeek の応答後、JSON または SSE ストリームを Codex が認識できる Responses 形式へ戻します。

## 事前準備

必要なもの：

- インストール済みで起動できる CC Switch。
- インストール済みで、少なくとも一度実行した Codex CLI。
- DeepSeek API Key。

プリセットには `https://api.deepseek.com` と正しいモデル名が設定済みです。base URL に `/chat/completions` を手動で追加しないでください。

## Step 1：V4 Pro プロバイダーを追加する

CC Switch を開き、上部の `Codex` タブへ切り替え、右上のプラスボタンをクリックします：

1. `DeepSeek V4 Pro` プリセットを選択します。
2. DeepSeek API Key を入力します。
3. プロバイダーを保存します。

このプリセットは Chat Completions 形式、`deepseek-v4-pro` モデルカタログ、推論パラメータを自動的に設定します。通常、高度な設定を手動で変更する必要はありません。

## Step 2：ローカルルーティングと Codex の引き継ぎを有効にする

設定の `ルーティング` ページを開き、`ローカルルーティング` を展開します：

1. ルーティング総スイッチをオンにしてローカルサービスを起動します。デフォルトアドレスは `127.0.0.1:15721` です。
2. アプリケーションルーティングで `Codex` を有効にします。

引き継ぎを有効にすると、Codex の live 設定はローカルルートを指します。実際の API Key は CC Switch のプロバイダー設定に保持され、転送時に注入されます。

## Step 3：プロバイダーを有効にして Codex を再起動する

Codex プロバイダー一覧へ戻り、`DeepSeek V4 Pro` を有効にします。このプリセットにはルーティング必須の表示があるため、使用中はローカルルーティングを起動したままにしてください。

切り替え後は Codex のターミナルセッションを再起動します。プロセスが古い `config.toml` をすでに読み込んでいる可能性があり、`/model` メニューも通常は新しいプロセスで `model_catalog_json` を再読み込みします。

Codex で `/model` を実行し、現在のモデルが `DeepSeek V4 Pro` であることを確認します。次に小さなリクエストを送り、CC Switch のルーティングまたはリクエストログに表示されることを確認してください。

## 以前の DeepSeek 設定から移行する

以前のバージョンでは、Flash と Pro が一つの Chat プリセットに含まれていました。アップグレード後も既存プロバイダーは保存済みの値を保持し、プロトコルは自動的に切り替わりません：

- Flash を使う場合：`DeepSeek` プリセットを選び直すか、新しいプロバイダーを作成します。ネイティブ Responses へ直接接続するため、ローカルルーティングは不要です。
- Pro を使う場合：`DeepSeek V4 Pro` プリセットを選び直すか、新しいプロバイダーを作成し、Codex ローカルルーティングを有効にしたまま使用します。

プリセットを変更した後は Codex を再起動し、live 設定とモデルカタログを更新してください。

## よくある質問

**V4 Flash だけを使う場合も Codex ローカルルーティングは必要ですか？**

いいえ。メインの `DeepSeek` プリセットを選択してください。Flash は Responses をネイティブサポートするため、CC Switch は Chat プロトコル変換を行いません。

**V4 Pro で 404、または `/responses` が見つからないエラーになる**

`DeepSeek V4 Pro` が選択され、ローカルルーティングサービスが実行中で、Codex のアプリケーションルーティングが有効であることを確認してください。DeepSeek の Chat base URL を Codex 設定へ直接書き込まないでください。

**`/model` に DeepSeek モデルが表示されない**

プロバイダーを保存して有効にした後、Codex を再起動してください。実行中の Codex プロセスはモデルカタログをホットロードしない場合があります。

**ルーティングを有効にしても、別のプロバイダーへ送信される**

Codex タブで `DeepSeek V4 Pro` が有効であること、ルーティング総スイッチがオンであること、アプリケーションルーティングで Codex が有効であることを確認してください。

## 参考リンク

- [CC Switch ユーザーマニュアル：プロバイダーの追加](../user-manual/ja/2-providers/2.1-add.md)
- [CC Switch ユーザーマニュアル：プロキシサービス](../user-manual/ja/4-proxy/4.1-service.md)
- [CC Switch ユーザーマニュアル：アプリケーションルーティング](../user-manual/ja/4-proxy/4.2-routing.md)
- [DeepSeek：Responses API の使用](https://api-docs.deepseek.com/guides/responses_api)
- [DeepSeek：Codex との統合](https://api-docs.deepseek.com/quick_start/agent_integrations/codex)
