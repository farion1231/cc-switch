# Codex で DeepSeek などの Chat 形式 API を使う: CC Switch ローカルルーティングガイド

> 対象バージョン: CC Switch 3.16.0 およびその前後のバージョン。本記事はリポジトリ内のドキュメントとコードをもとに整理し、OpenAI Chat Completions 互換 API の例として DeepSeek を使用します。スクリーンショットは現在のフロントエンド UI から、実際の API Key やアカウント残高が漏れないよう匿名化したサンプルデータで生成しています。

## ローカルルーティングが必要な理由

新しい Codex CLI は OpenAI Responses API を前提にしています。一方で DeepSeek、Kimi、MiniMax、SiliconFlow など多くのプロバイダーが実際に公開しているのは OpenAI Chat Completions 形式、つまり `/chat/completions` です。この 2 つのプロトコルは、リクエストボディ、ストリーミングイベント、レスポンス構造が異なります。Chat エンドポイントをそのまま Codex 設定に入れると、モデル一覧が合わない、リクエストが 404/400 になる、ストリーミングレスポンスを Codex が正しく解析できない、といった問題が起きがちです。

CC Switch では、Codex が常にローカルルートへ接続し、Responses API のままリクエストを送るようにします。ルート内部で現在のプロバイダーが Chat 形式かどうかを判定し、必要ならリクエストを Chat Completions に書き換えて上流へ送り、最後に Chat レスポンスを Codex が理解できる Responses 形式へ戻します。

![Codex プロバイダー一覧のローカルルーティング必須マーク](../images/codex-deepseek-routing/01-codex-providers-require-routing.png)

この経路は主に 4 つのステップに分かれます：

1. Codex ルーティングを有効にすると、ローカル設定は `http://127.0.0.1:15721/v1` に書き換えられ、`wire_api = "responses"` は維持されます。
2. Provider の `meta.apiFormat = "openai_chat"` が、実際の上流は Chat Completions だとルートに伝えます。
3. ルートは `/responses` または `/v1/responses` を `/chat/completions` に書き換え、Responses のリクエストボディを Chat のリクエストボディへ変換します。
4. 上流から返ってきた後、ルートは Chat の JSON または SSE ストリームを Responses JSON/SSE へ変換して返します。

## 事前準備

先に次の 3 つを用意してください：

- インストール済みで起動できる CC Switch。
- インストール済みの Codex CLI。少なくとも 1 回は実行し、`~/.codex/config.toml` のディレクトリ構造が存在していること。
- DeepSeek または同種の Chat Completions プロバイダーの API Key。

DeepSeek 公式ドキュメントでは、OpenAI 互換 base URL は現在 `https://api.deepseek.com`（他のプロバイダーでは `/v1` 付きの base URL もよくあります）、Chat API のパスは `/chat/completions` と記載されています。CC Switch の DeepSeek プリセットにはこれらの情報がすでに入っているため、まずはプリセットを使い、エンドポイントパスを手で組み立てる必要はありません。

## Step 1: Codex プロバイダーを追加する

CC Switch を開き、上部の `Codex` タブへ切り替え、右上のプラスボタンからプロバイダーを追加します。

内蔵プリセットの `DeepSeek` を選びます。必要なのは次の 2 つだけです：

- DeepSeek API Key を入力する。
- プロバイダーを保存する。

![DeepSeek Codex プロバイダーフォームのローカルルーティング設定](../images/codex-deepseek-routing/02-deepseek-codex-routing-form.png)

プリセットには DeepSeek のリクエスト先、デフォルトモデル、モデルメニュー、thinking/reasoning パラメータがすでに含まれており、`ローカルルーティングが必要` も自動的に有効になります。必要に応じてデフォルトモデルやモデル表示名を調整できますが、プロトコル変換はルーティング層に任せれば十分です。

## Step 2: ローカルルーティングを有効にして Codex をルーティングする

設定の `ルーティング` ページに入り、`ローカルルーティング` を展開して、次の 2 つのスイッチを設定します：

1. `ルーティング総スイッチ` をオンにしてローカルサービスを起動します。デフォルトアドレスは `127.0.0.1:15721` です。
2. `ルーティング有効` で `Codex` をオンにします。Codex だけをルーティングしたい場合は、Claude と Gemini はオフのままで構いません。

![ローカルルーティング画面で Codex ルーティングを有効化](../images/codex-deepseek-routing/03-local-route-codex-takeover.png)

ルーティングを有効にすると、CC Switch は Codex の live 設定をローカルルートへ向け、認証はプレースホルダーで管理します。実際の DeepSeek Key は CC Switch の Provider 設定内に残り、ローカルルートが転送時に注入します。そのため、Codex の live 設定に Key を露出させる必要はありません。

## Step 3: プロバイダーを切り替えて Codex を再起動する

Codex プロバイダー一覧に戻り、DeepSeek プロバイダーの `有効化` をクリックします。`ルーティングが必要` の表示が見える場合、そのプロバイダーはルーティング実行中に使う必要があります。ルーティングが起動していない場合、CC Switch は「ルーティングサービスが必要」という趣旨のメッセージを表示します。

切り替え後は、現在の Codex ターミナルセッションを再起動することをおすすめします。理由は次のとおりです：

- Codex プロセスがすでに古い `config.toml` を読み込んでいる可能性があります。
- `model_catalog_json` の生成後、`/model` メニューの更新には通常、新しいプロセスが必要です。

Codex に入ったら、`/model` で現在のモデルが DeepSeek プリセット由来かどうかを確認します。たとえば `DeepSeek V4 Flash` などです。現在の Codex app は複数モデル選択に対応していないため、設定内の最初のモデルをデフォルトで使用します。その後、小さな質問を 1 つ送って、ルーティングパネルのリクエスト数が増えるか、usage / リクエストログに Codex リクエストが出るかを確認します。

## 他の Chat プロバイダーの場合

DeepSeek、Kimi、MiniMax、SiliconFlow など一般的な Chat 形式プロバイダーは CC Switch にプリセットがあるため、まずはプリセットを使ってください。プリセットにないプロバイダーだけ、カスタム設定を選びます。その場合は相手側のドキュメントに従って API Key、base URL、モデルを入力し、`API 形式` を `OpenAI Chat Completions (ルーティングが必要)` に設定します。

上流が OpenAI Responses API を直接サポートしている場合は、`ローカルルーティングが必要` を有効にする必要はありません。その場合、CC Switch は Responses のまま直結でき、Chat 変換は行いません。

## よくある質問

**Codex が 404 を返す、または `/responses` が見つからない**

多くの場合、Codex ルーティングが有効になっていないか、上流 Chat base URL を手動で Codex に直接書いています。`~/.codex/config.toml` が `http://127.0.0.1:15721/v1` を指しているか確認してください。

**DeepSeek 上流が 404 を返す**

内蔵 DeepSeek プリセットを使っている場合は、まず現在のプロバイダーが本当にプリセット由来であること、そして Codex ルーティングが有効であることを確認してください。カスタムプロバイダーを使っている場合だけ、base URL を追加で確認します。base URL はサービスのルートであり、`/chat/completions` 付きの完全なエンドポイントパスではありません。

**`/model` に DeepSeek モデルが表示されない**

プロバイダーを保存した後、Codex を再起動してください。CC Switch は `cc-switch-model-catalog.json` を生成し、そのパスを `model_catalog_json` に書き込みますが、実行中の Codex プロセスがモデルカタログをホットロードするとは限りません。
現在の Codex app は複数モデル選択に対応していないため、設定内の最初のモデルをデフォルトで使用します。

**ルーティングを有効にしたのに、リクエストが別のプロバイダーへ行く**

次の 3 つの状態が一致しているか確認してください：Codex タブの現在のプロバイダーが DeepSeek であること、ローカルルーティングサービスが実行中であること、`ルーティング有効` で Codex スイッチがオンであること。

**公式 OpenAI Codex アカウントをローカルルーティング経由で使えますか**

おすすめしません。CC Switch はローカルルーティング有効中、公式プロバイダーへの切り替えをブロックします。プロキシ経由で公式 API にアクセスすると、アカウントリスクが発生する可能性があるためです。ルーティングは主にサードパーティ、集約サービス、またはプロトコル変換のための機能です。

## 参考リンク

- [CC Switch ユーザーマニュアル: プロバイダーの追加](../user-manual/ja/2-providers/2.1-add.md)
- [CC Switch ユーザーマニュアル: プロキシサービス](../user-manual/ja/4-proxy/4.1-service.md)
- [CC Switch ユーザーマニュアル: アプリケーションルーティング](../user-manual/ja/4-proxy/4.2-routing.md)
- [DeepSeek API Docs: Your First API Call](https://api-docs.deepseek.com/)
- [DeepSeek API Docs: Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [DeepSeek API Docs: Multi-round Conversation](https://api-docs.deepseek.com/guides/multi_round_chat)
