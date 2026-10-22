# サイトに「CC Switch にワンクリックでインポート」ボタンを置く：サイト運営者向け連携ガイド

[中文](deeplink-site-integration-guide-zh.md) | [English](deeplink-site-integration-guide-en.md) | 日本語

サイト運営者であれば、CC Switch を通じて、ユーザーがあなたのプロバイダーを Claude Code や Codex などの Agent アプリにワンクリックで取り込めるようにできます。

> 対象バージョン：CC Switch v4.0.10 以降。本文中で「v4.0.10 以降」と記した動作は、それより前のバージョンでは異なります。違いは末尾の「旧バージョンとの互換性」を参照してください。本記事は現在のコードをもとに整理しており、例に出てくるドメインと Key はすべて架空のものです。

## この記事の対象読者

API サービス（中継サービス、集約プラットフォーム、自前のゲートウェイなど）を運営していて、ユーザーがコンソールでボタンを 1 回押すだけで、エンドポイント、Key、モデル、それに残高の照会までまとめて CC Switch に追加できるようにしたい方を想定しています。ユーザーが手でフォームを埋める必要はなくなります。

これは `ccswitch://` ディープリンクで実現します。ユーザーのシステムに CC Switch がインストールされていれば、ブラウザで `ccswitch://...` のリンクを押すと CC Switch が起動してインポート確認ダイアログが開き、ユーザーが内容を確かめて「インポート」を押します。

ユーザーマニュアルの [ディープリンクプロトコル](../user-manual/ja/5-faq/5.3-deeplink.md) はパラメーターの早見表です。この記事では、サイト運営者が実際に決める必要のあること——リンクの生成方法、何を含められるか、何が拒否されるか、インポート直後に残高を表示させる方法——を説明します。

## 最短ルート

> サイトを New API または Sub2API で構築していますか？ どちらも Key 一覧に「CC Switch へのインポート」ボタンが最初から付いているので、自分で開発しなくてもそのまま使えます。違いは残高です。New API のリンクには使用量クエリスクリプトが含まれないため、インポート後のカードに残高は表示されません。Sub2API のリンクには含まれています。残高を表示したい、自社アイコンを付けたい、対応アプリを増やしたいといった場合は、この記事に沿って手を加えてください。

ほとんどのサイト運営者がやることは 3 つだけです。以下の節を順に読めば足ります。

1. 「まず要点」を読み、続いて「プロバイダーリンク」の「最小構成のリンク」「対応アプリ」「よく使うパラメーター」を読んで、どのアプリ向けにリンクを生成するか、エンドポイントと形式をどう書くかを決めます。まず [オンライン生成ツール](https://ccswitch.io/deplink.html) のフォームに入力してリンクを 1 本作り、自分のパソコンで押して確認ダイアログの見た目を確かめておくとよいでしょう。
2. インポート後に残高を表示したい場合は「使用量クエリスクリプト」を読みます。NewAPI / OneAPI 系、Sub2API 系のサイトには既成のスクリプトがあるので、そのまま写せば済みます。
3. 「リンクを生成するコード」と「リンクをボタンに載せる」を読みます。バックエンドでユーザーごとにリンクを生成し、フロントエンドにボタンを置きます。

折りたたんである節（同梱する設定の原文、拒否されるリンクのまとめ、MCP / プロンプト / Skill）は、必要になったときに開いてください。

## まず要点

1. **リンクはユーザーごとに生成します**。プロバイダーのインポートでは API Key が必須で、Key のないリンクはインポートに失敗します。そのため「ワンクリックインポート」ボタンはユーザーがログインした後のコンソールに置き、バックエンドがそのユーザー自身の Key でリンクを組み立てるようにしてください。公開ページには置かないでください。ボタンの作り方は「リンクをボタンに載せる」を参照してください。
2. **`apiFormat` で API の形式を宣言します**。値は `anthropic`、`openai_chat`、`openai_responses`、`gemini_native` のいずれかで、アプリごとの対応状況は後述します。省略するとアプリのデフォルト形式になります。Claude Code は Anthropic Messages、Codex と Grok Build は OpenAI Responses、OpenCode / OpenClaw / Hermes / Pi は OpenAI Chat Completions です。
3. **モデル名は必要なときだけ書きます**。中継サービスが純正の Claude モデルを提供している場合、Claude Code のモデルはすべて空欄にしてください。Claude Code は内蔵のモデル一覧を使います。DeepSeek や Kimi のようなネイティブでないモデルの場合にだけ、`model`、`haikuModel`、`sonnetModel`、`opusModel` を書きます。OpenCode、OpenClaw、Hermes のモデル一覧は `model` から作られるため、書かないと選べるモデルがありません。Pi では必須です。
4. **ユーザーに残高を見せたいなら、使用量クエリスクリプトを付けます**。v4.0.10 以降は `usageScript` を付けるだけでデフォルトで有効になります。旧バージョンとも互換にしたい場合は、あわせて `usageEnabled=true` を書いてください。スクリプトがリクエストできるのは、プロバイダーのエンドポイントと同一オリジンの HTTPS API だけです。詳しくは後述します。
5. **「インポート後すぐに切り替えるか」はユーザーが決めます**。確認ダイアログにそのスイッチがあり、リンクの `enabled` はスイッチの初期値にすぎません。通常は書かなくて構いません。
6. **Base64 のパラメーターはさらに URL エスケープします**。リンクは `URLSearchParams` などの同等の関数で組み立て、文字列を手で連結しないでください。

## リンクの構造

```
ccswitch://v1/import?resource=<種類>&<パラメーター>...
```

- `v1` はプロトコルのバージョン、`/import` は固定のパスで、どちらも間違えると拒否されます。
- `resource` は `provider`（プロバイダー）、`mcp`、`prompt`（プロンプト）、`skill` の 4 つのいずれかです。
- パラメーター名は大文字と小文字を区別し、キャメルケースで書きます。`api_key` ではなく `apiKey`、`usageScript` です。

### エンコード規則

エンコードは 2 段階で、順番を逆にしてはいけません。

1. `config`、`usageScript`、`content` のような「ひとまとまりの内容」を渡すパラメーターは、まず UTF-8 テキストを **Base64** エンコードします。標準の Base64 と URL-safe Base64 のどちらも受け付け、末尾の `=` はあってもなくても構いません。
2. リンクに組み込むときに、**すべてのパラメーター値**をもう一度 URL エスケープします（`application/x-www-form-urlencoded` の規則）。標準 Base64 に含まれる `+` は必ず `%2B` にしてください。そうしないとスペースとして解釈されます。CC Switch はスペースを `+` に戻そうとしますが、それに頼らないでください。

`URLSearchParams` は手順 2 を自動で行います。リンクを手書きするときに最も多い間違いが、この手順の抜けです。

真偽値のパラメーターは `true` / `false` で書きます。それ以外の書き方は、書かなかったものとして扱われます。

## ユーザーがリンクをクリックした後

1. ブラウザが「CC Switch を開きますか」と確認し、ユーザーが許可すると CC Switch が起動します（起動していなければまず起動します）。
2. CC Switch がインポート確認ダイアログを開き、リンクの内容をすべて表示します。エンドポイント、マスクされた Key、モデル、同梱された設定の原文、使用量クエリスクリプトの全文です。疑わしい内容は黄色で強調して警告します。たとえばローカルや社内ネットワークを指すアドレス、プロセスの読み込み動作を変えられる環境変数（`NODE_OPTIONS`、`LD_PRELOAD` など）、シェルでコマンド全体を実行する MCP などです。
3. ユーザーが「インポート」を押すと、次のようになります。
   - **プロバイダー**：対応するアプリのプロバイダー一覧に追加されます。確認ダイアログの下部にスイッチが 1 つあり、初期値はリンクの `enabled` から取られ、ユーザーが変更できます。
     - Claude Code、Codex、Gemini CLI、Antigravity CLI、Grok Build：スイッチは「インポート後にこのプロバイダーへ切り替える」で、デフォルトはオフです。オンにすると、確認ダイアログに「クライアントのリクエストは上記のエンドポイントに送られます」という注意が表示されます。
     - OpenCode、OpenClaw、Hermes、Pi：これらのアプリは複数のプロバイダーを同時に置けるため、スイッチは「インポート後に <アプリ名> の設定へ追加する」（Pi では「インポート後に Pi で有効にする」）で、デフォルトはオンです。オフにすると CC Switch の一覧に保存されるだけになります。
   - **MCP**：`apps` に挙げた各アプリの設定ファイルにすぐ書き込まれます。
   - **プロンプト**：プロンプト一覧に追加されます。確認ダイアログに「インポート後に有効にする」スイッチがあり、初期値はリンクの `enabled` から取られます。オンにすると、そのアプリの現在のプロンプトファイルが置き換えられ、それまで有効だったプロンプトは無効になります。
   - **Skill**：追加されるのは Skill リポジトリです。ユーザーはさらに「Skills」ページで、どの Skill をインストールするかを選ぶ必要があります。

サイト運営者がつまずきやすい細かな点：

- リンクはユーザーのクリックでしか起動できません。ページ読み込み時に `ccswitch://` へ自動で遷移させると、ほとんどのブラウザでブロックされます。
- CC Switch が一度に処理するリンクは 1 本だけです。前のリンクをユーザーが処理し終える前に新しいリンクが押されると、最新の 1 本だけが残ります。「1 つのボタンで複数のリンクを続けて起動する」設計にはせず、プロバイダー、MCP、プロンプトにそれぞれボタンを用意して、ユーザーに順に押してもらってください。
- 同じプロバイダーリンクを 2 回押すと 2 件インポートされます。プロバイダー ID は名前とタイムスタンプから生成され、重複は除かれません。
- Web ページからは、ユーザーが CC Switch をインストールしているかどうかを確実には判定できません。ボタンの横に案内文とダウンロード先を添えてください。書き方は「リンクをボタンに載せる」を参照してください。

## プロバイダーリンク

### 最小構成のリンク

```
ccswitch://v1/import?resource=provider&app=claude&name=Example%20AI&endpoint=https%3A%2F%2Fapi.example.com&apiKey=sk-xxxx
```

必須なのは `app`、`name`、それにエンドポイントと Key だけです（エンドポイントと Key は URL パラメーターに書いても、同梱する `config` に書いても構いません）。Pi ではこれに加えて `model` が必須です。

### 対応アプリ

| `app` | アプリ | デフォルト形式 | 指定できる `apiFormat` | `config` に含められるもの |
|-------|------|---------|--------------------|-------------------|
| `claude` | Claude Code | `anthropic` | 4 種類すべて | `{"env": {...}}`。`env` 内の変数はすべて保持 |
| `codex` | Codex | `openai_responses` | `openai_responses`、`openai_chat`、`anthropic` | `config.toml` の全体または一部。後述 |
| `gemini` | Gemini CLI | `gemini_native` | `gemini_native` のみ | `GEMINI_API_KEY`、`GOOGLE_GEMINI_BASE_URL`、`GEMINI_MODEL` の 3 キー。ほかは無視 |
| `agy` | Antigravity CLI | `gemini_native` | `gemini_native` のみ | Gemini のフラットな環境キー、または `{env, config:{model:{name}}}`。URL パラメーターが優先され、モデルは `GEMINI_MODEL` より `config.model.name` が優先。ほかのネイティブ設定は無視 |
| `grokbuild` | Grok Build | `openai_responses` | `openai_responses`、`openai_chat`、`anthropic` | `[model."<名前>"]` テーブル 1 つ。後述 |
| `opencode` | OpenCode | `openai_chat` | 4 種類すべて | エンドポイントと Key のみ取得 |
| `openclaw` | OpenClaw | `openai_chat` | 4 種類すべて | エンドポイントと Key のみ取得 |
| `hermes` | Hermes | `openai_chat` | `openai_chat`、`openai_responses`、`anthropic` | エンドポイントと Key のみ取得 |
| `pi` | Pi | `openai_chat` | 4 種類すべて | エンドポイントと Key のみ取得 |

Claude Desktop、MiniMax Code のプロバイダーは現在ディープリンクでのインポートに対応しておらず、リンクは拒否されます。

`apiFormat` が反映される場所はアプリによって異なります。

- **Claude Code、Codex、Grok Build**：この 3 つのクライアントは自分のネイティブ形式しか話しません（Claude Code は Anthropic、残りの 2 つは Responses）。ネイティブでない形式を書くと、インポートされたカードに「ルーティングが必要」マークが付き、ユーザーが切り替えるときにはローカルルーティングを有効にして、CC Switch にリクエスト形式を変換させる必要があります。
- **OpenCode、OpenClaw、Hermes、Pi**：これらのクライアントは自身で複数の形式に対応しているため、CC Switch は `apiFormat` を各クライアントの設定のプロトコル欄（OpenCode の `npm`、OpenClaw / Pi の `api`、Hermes の `api_mode`）に換算します。クライアントはその形式で直接あなたの API にリクエストし、ローカルルーティングは経由しません。

エンドポイントは選んだ形式の慣例に従って書きます。Anthropic 形式は Claude Code では `/v1` を付けず（例：`https://api.example.com`）、OpenAI の 2 形式は通常 `/v1` を付けます。OpenCode、OpenClaw、Hermes、Pi のエンドポイントはそれぞれの SDK の base URL で、パスの組み立て方は統一されていません。同じ Anthropic 形式でも、CC Switch の内蔵プリセットでは OpenCode のエンドポイントに `/v1` が付き、Hermes と Pi には付きません。アプリごとに別々のリンクを生成し、エンドポイントはそのアプリで実際に通ることを確かめた書き方にしてください。

### よく使うパラメーター

| パラメーター | 説明 |
|------|------|
| `name` | 表示名。必須 |
| `endpoint` | API エンドポイント。半角カンマ区切りで複数指定でき、最初が主エンドポイント、残りはエンドポイント候補に追加され、ユーザーが速度テストして切り替えられます |
| `apiKey` | API Key。URL パラメーターと `config` の両方にある場合は URL パラメーターが優先 |
| `apiFormat` | API の形式。上の表を参照。省略時はアプリのデフォルト形式 |
| `model` | デフォルトモデル。Claude Code で純正の Claude モデルを使う場合は空欄。Codex は省略すると `gpt-5-codex`、Grok Build は省略すると `grok-4.5` を使用。Pi は必須 |
| `haikuModel` / `sonnetModel` / `opusModel` | Claude Code のみ：3 段階のモデルエイリアスに対応する実際のモデル名。ネイティブでないモデルの場合にだけ必要 |
| `homepage` | 公式サイト。省略時はエンドポイントから推定（`api.` の接頭辞を除く） |
| `icon` | CC Switch 内蔵アイコンの名前。英小文字・数字と `. _ -` のみ使用でき、それ以外の書き方は捨てられます。知らない名前の場合は名前の頭文字を表示。自社ブランドの内蔵アイコンがなければ書く必要はありません |
| `iconUrl` | 自社アイコン画像の URL。後述の「自社アイコンを付ける」を参照。ダウンロードに失敗した場合は `icon` に戻ります |
| `notes` | メモ。プロバイダーカードに表示されます |
| `enabled` | 確認ダイアログの「インポート後に切り替える / 追加する」スイッチの初期値。ユーザーが変更できます。通常は書かなくて構いません |
| `config` + `configFormat` | 同梱する設定の原文（Base64）。`configFormat` は `json`（デフォルト）または `toml` |

`configUrl`（リモート設定）はまだ実装されておらず、このパラメーターを含むリンクはそのまま拒否されます。

### 自社アイコンを付ける：iconUrl

CC Switch の内蔵アイコンに自社ブランドがない場合は、`iconUrl` で画像の URL を指定します。確認ダイアログが開いた時点で CC Switch がその画像をダウンロードして上部に表示し、インポート後はそれがカードのアイコンになります。ユーザーが追加で何かを押す必要はありません。

画像は 96×96 の PNG に描き直して CC Switch に保存され、以後その URL にはアクセスしません。後でアイコンを変えても、すでにインポートしたユーザーには反映されません。

URL と画像は以下の条件を満たす必要があります。1 つでも満たさない場合、確認ダイアログはエラーを出さず、アイコンは `icon` で指定した内蔵アイコンに戻り、`icon` もなければ名前の頭文字が表示されます。

- `https://` で始まり、ホストがドメイン名（IP は不可）、デフォルトポート、アカウントやパスワードを含まないこと。形の正しくない URL はリンクの解析時に捨てられ、リンクのそれ以外の部分は通常どおりインポートされます。
- 公開アクセスできること：CC Switch はダウンロード時に Cookie と Referer を付けず、ユーザーのシステムプロキシも使いません。ログインが必要な URL や、ホットリンク防止を有効にした URL は失敗します。
- ドメインの解決先がパブリックアドレスであること。社内ネットワークやローカルを指すものは一切ダウンロードしません。
- 5 秒以内にダウンロードが終わること。リダイレクトは最大 3 回までたどり、リダイレクト先もそれぞれ上記の条件を満たす必要があります。
- 256 KB 以下であること。
- レスポンスヘッダーの `Content-Type` が `image/png`、`image/jpeg`、`image/webp`、`image/svg+xml` のいずれかで、ファイルの実際の形式と一致していること。`application/octet-stream` のような汎用の型は不可です。
- ビットマップの幅と高さが 4096 ピクセル以下であること。

画像そのものについての推奨：

- 正方形の画像を使ってください。四隅が不透明な正方形の画像はアイコン枠いっぱいに表示されます。四隅が透明なロゴは中央に縮小され、周囲に余白ができて、内蔵アイコンと同じ見た目になります。
- SVG は画像としてのみ表示され、中のスクリプトは実行されず、参照する外部フォントや画像も読み込まれません。文字はパスに変換してください。
- ユーザーが確認ダイアログを開くたびにこの URL へ 1 回リクエストが来ます。CDN に置けば十分です。

### Claude Code：環境変数を同梱する

<details>
<summary>追加の環境変数（カスタムリクエストヘッダー、タイムアウトなど）が必要な場合に開く</summary>

追加の環境変数（カスタムリクエストヘッダー、タイムアウト、実験的機能の無効化など）が必要なサイトは、それらを `config` に入れます。

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "https://api.example.com",
    "ANTHROPIC_AUTH_TOKEN": "sk-xxxx",
    "ANTHROPIC_MODEL": "example-sonnet",
    "API_TIMEOUT_MS": "600000",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1"
  }
}
```

`env` 内の変数はそのまま保持されます。エンドポイント、Key、モデル名については、URL パラメーターにも書かれていれば URL パラメーターが優先されます。確認ダイアログには変数が 1 つずつ表示され、機密性のある値はマスクされます。

</details>

### Codex：config.toml を同梱する

<details>
<summary><code>wire_api</code>、リクエストヘッダー、推論レベルなどの追加設定を含めたい場合にだけ必要です。開くと、受け付けるキーと拒否される書き方を確認できます</summary>

書き方は 2 通りです。

- `configFormat=json` で、内容を `{"auth": {"OPENAI_API_KEY": "sk-xxxx"}, "config": "<config.toml のテキスト>"}` にする。
- `configFormat=toml` で、内容を `config.toml` の原文そのものにする。

CC Switch はファイル全体をユーザーの `~/.codex/config.toml` にそのまま持ち込むことはせず、このプロバイダーに属する部分だけを受け取ります。

- **選ばれた `[model_providers.<id>]` テーブル**：`model_provider` が指しているテーブルです。`model_provider` を書かない場合、ファイル内のテーブルは 1 つだけでなければなりません。テーブルからは次のキーを受け取ります：`name`、`base_url`、`wire_api`、`requires_openai_auth`、`experimental_bearer_token`、`model_catalog_url`、`capabilities`、`query_params`、`http_headers`、`request_max_retries`、`stream_max_retries`、`stream_idle_timeout_ms`、`websocket_connect_timeout_ms`、`supports_websockets`、`supports_standalone_web_search`。
- **トップレベルのモデル関連フィールド**：`model`、`review_model`、`model_reasoning_effort`、`plan_mode_reasoning_effort`、`disable_response_storage`、`web_search`、`model_context_window`、`model_auto_compact_token_limit`、`model_supports_reasoning_summaries`、`model_verbosity`、それに `[agents]` のサブエージェント用モデルと `[memories]` のメモリー用モデル。

それ以外は一切受け取りません。`[features]`、`[tools]`、MCP、sandbox、`notify`、ほかのプロバイダーテーブルなどは、ユーザー自身のグローバル設定だからです。

インポート後、テーブル名は一律 `custom` に変わり（CC Switch が管理する Codex プロバイダーはすべて `[model_providers.custom]` に書かれます）、`base_url` と `experimental_bearer_token` にはマージ後のエンドポイントと Key が使われます。`model_catalog_url` はプロバイダーのエンドポイントと同一オリジンでなければならず、そうでなければインポートは拒否されます。

**拒否される書き方**：テーブルに `auth`（Codex にコマンドを実行させて Key を取得する）、`aws`、`gateway_oauth`（ローカルのクラウド認証情報を読む）、`env_http_headers`（環境変数を読む）が含まれるもの。`env_key` は捨てられるので、Key は必ずリンク自体で渡してください。`env_key` だけを書いて Key を渡していないリンクは拒否されます。

例（`configFormat=toml`）：

```toml
model_provider = "example"
model = "example-coder"
model_reasoning_effort = "high"

[model_providers.example]
name = "Example AI"
base_url = "https://api.example.com/v1"
wire_api = "responses"
http_headers = { "X-Client" = "cc-switch" }
stream_idle_timeout_ms = 300000
```

</details>

### Grok Build：モデルテーブルを 1 つ同梱する

<details>
<summary>コンテキストウィンドウ、サンプリングパラメーターなどの追加設定を含めたい場合にだけ必要です</summary>

書き方は Codex と同じです（`{"config": "<config.toml のテキスト>"}`、または `configFormat=toml`）。

インポートされるのは `[models] default` が指す `[model."<名前>"]` テーブルだけです。`default` を書かない場合、ファイル内のテーブルは 1 つだけでなければならず、テーブルが複数あるのに `default` がない場合は拒否されます。CC Switch では 1 つの Grok Build プロバイダーが 1 つのモデルテーブルに対応するため、1 本のリンクで複数のモデルをインポートすることはできません。

テーブルからは表示名、`description`、`api_backend`、`context_window`、`max_completion_tokens`、サンプリングとリトライのパラメーター、推論レベル関連のフィールドなどを保持します。エンドポイント、Key、モデル名にはリンクのパラメーターを使います（URL パラメーターが優先）。テーブルに `api_backend`、`context_window` がない場合はデフォルト値を補います。

テーブルに `auth_provider`、`env_http_headers`、`mtls_cert_dir` を含むリンクは拒否されます。`env_key` の扱いは Codex と同じです。

</details>

### 拒否されるリンクのまとめ

<details>
<summary>確認ダイアログでエラーが出たときに開き、エラーの原文から原因を調べる</summary>

| 状況 | 確認ダイアログのエラー（英語の原文、抜粋） |
|------|-------------------------------|
| Key がない | `API key is required` |
| エンドポイントがない | `Endpoint is required` |
| エンドポイントが http / https でない | `Invalid URL scheme for 'endpoint[0]'` |
| `app` に対応していない値を書いた | `Invalid provider app type` |
| `apiFormat` の値が正しくない | `Unsupported apiFormat` |
| そのアプリがこの `apiFormat` に対応していない | `is not supported for app` |
| Pi でモデルを書いていない | `Pi providers need a model` |
| OpenCode / OpenClaw / Hermes / Pi の名前、エンドポイント、Key、モデルに、クライアントが展開する書き方が含まれる（下記参照） | `uses syntax that … would expand` |
| `configUrl` を含む | `'configUrl' is not supported yet` |
| Base64 をデコードできない | `config 参数 Base64 解码失败`（config パラメーターの Base64 デコードに失敗） |
| Codex / Grok で `env_key` だけで Key を渡している | `supplies its API key indirectly` |
| Codex のテーブルに `auth` などがある | `cannot be imported from a link` |
| Codex のモデル目録の URL がプロバイダーのエンドポイントと同一オリジンでない | `model_catalog_url` |
| Grok でテーブルが複数あるのに `default` がない | `several [model.*] tables` |

OpenCode、OpenClaw、Hermes、Pi は、設定を読むときに特定の書き方を展開します。Pi では Key が `!` で始まるとコマンドとして実行され、`$NAME` / `${NAME}` は環境変数に置き換えられます。OpenCode の `{env:…}` は環境変数を、`{file:…}` はローカルのファイルを読みます。OpenClaw と Hermes の `${…}` は環境変数を読みます。リンクからのインポートでは、これらの書き方はすべて拒否されます。あなたの Key とエンドポイントはもともとリテラル値なので、影響はありません。

</details>

## インポート直後に残高を表示する：使用量クエリスクリプト

CC Switch のプロバイダーカードには残高やプランの使用量を表示でき、そのデータは使用量クエリスクリプトから取得します。スクリプトをリンクに入れておけば、ユーザーはインポートした直後から残高を確認できます。

### スクリプトの形

スクリプトは JavaScript の式で、`request`（どんなリクエストを送るか）と `extractor`（レスポンスからどう値を取り出すか）を持つオブジェクトを返します。

```javascript
({
  request: {
    url: "{{baseUrl}}/user/balance",
    method: "GET",
    headers: {
      "Authorization": "Bearer {{apiKey}}"
    }
  },
  extractor: function (response) {
    return {
      remaining: response.balance,
      unit: "USD"
    };
  }
})
```

実行前に、CC Switch はスクリプト中のプレースホルダーを実際の値に置き換えます。

| プレースホルダー | 置き換え後 |
|--------|--------|
| `{{apiKey}}` | `usageApiKey`。指定がなければプロバイダーの API Key |
| `{{baseUrl}}` | `usageBaseUrl`。指定がなければプロバイダーの主エンドポイント（末尾の `/` を除く） |
| `{{accessToken}}` | `usageAccessToken` |
| `{{userId}}` | `usageUserId` |

`{{baseUrl}}` はプロバイダーのエンドポイントそのままである点に注意してください。`/v1` が付いていれば、組み立てた結果は `https://api.example.com/v1/user/balance` になります。残高 API が `/v1` の下にない場合は、スクリプトに完全な URL を直接書いて構いません。エンドポイントと同一オリジンでありさえすれば大丈夫です。

`extractor` が受け取るのはパース済みのレスポンス JSON で、オブジェクト 1 つ、またはオブジェクトの配列（プランが複数ある場合）を返します。フィールドはすべて任意です。

| フィールド | 型 | 説明 |
|------|------|------|
| `remaining` | 数値 | 残りの額 |
| `used` | 数値 | 使用済み |
| `total` | 数値 | 総額 |
| `unit` | 文字列 | 単位。例：`USD`、`CNY`、`回` |
| `planName` | 文字列 | プラン名 |
| `isValid` | 真偽値 | Key が有効かどうか。`false` のときカードに無効と表示 |
| `invalidMessage` | 文字列 | 無効の理由 |
| `extra` | 文字列 | 補足説明 |

### 実行時の制限

スクリプトは信頼できない入力として扱われ、CC Switch はハードな制限をかけています。

- QuickJS のサンドボックス内で動き、`fetch`、ファイル、ネットワークなどの機能は一切ありません。スクリプト自体は「リクエストの記述」と「レスポンスの解析」だけを担い、リクエストは CC Switch が代わりに送ります。
- **1 回の照会で送るリクエストは 1 つだけ**です。先にログインしてから照会する、といったことはできません。
- **リクエストは HTTPS で、かつ `{{baseUrl}}` と同一オリジン（同じドメイン、同じポート）でなければなりません**。オリジンが異なるリクエストは拒否されます。残高 API が別のドメインにある場合（たとえばコンソールが `console.example.com`、API が `api.example.com`）は、`usageBaseUrl` でコンソールのドメインを指してください。
- レスポンスは JSON でなければなりません。
- リクエストはブラウザを経由せず、CC Switch がユーザーのパソコンから直接送るため、残高 API に CORS の設定は不要です。API は API Key で認証できる必要があります。ログイン状態やシステムアクセストークンでしか照会できない場合は、`usageAccessToken` でトークンをリンクに含めてください（下の NewAPI の節を参照）。
- スクリプトの実行時間の上限は 5 秒、メモリの上限は 16 MiB、HTTP リクエストのタイムアウトは 10 秒です。

### 有効になるかどうか

| リンクの書き方 | インポート後 |
|----------|--------|
| `usageScript` あり、`usageEnabled` なし | 有効（v4.0.10 以降。それより前のバージョンでは無効） |
| `usageScript` あり、`usageEnabled=true` | 有効 |
| `usageScript` あり、`usageEnabled=false` | 無効。ユーザーがアプリ内で手動でオンにできます |
| スクリプトなし | 無効 |

確認ダイアログには、スクリプトの本文全体と「有効 / 無効」の表示が出ます。

`usageAutoInterval` は自動照会の間隔（分）です。`0` または省略すると定期的な照会は行いません。短くしすぎないでください。すべてのユーザーがこの頻度であなたの API を叩くことになります。

### NewAPI / OneAPI 系のサイト

この種のサイトの残高 API は、API Key ではなくシステムアクセストークンとユーザー ID を使います。リンクには `usageAccessToken` と `usageUserId` を含めてください。

```javascript
({
  request: {
    url: "{{baseUrl}}/api/user/self",
    method: "GET",
    headers: {
      "Authorization": "Bearer {{accessToken}}",
      "New-Api-User": "{{userId}}"
    }
  },
  extractor: function (response) {
    if (response.success && response.data) {
      return {
        planName: response.data.group,
        remaining: response.data.quota / 500000,
        used: response.data.used_quota / 500000,
        unit: "USD"
      };
    }
    return { isValid: false, invalidMessage: response.message };
  }
})
```

`500000` は NewAPI のデフォルトのクォータ換算比率です。変更している場合は自分の値に合わせてください。システムアクセストークンは API Key より権限が大きいので、ユーザー本人がログインした後に生成するリンクにだけ入れてください。

### Sub2API 系のサイト

Sub2API には CC Switch 向けの `GET /v1/usage` API が最初から用意されており、API Key で認証するため `usageAccessToken` は不要です。返されるトップレベルのフィールドは、CC Switch が理解する `isValid`、`planName`、`remaining`、`unit` そのものなので、スクリプトはほぼそのまま写せます。

```javascript
({
  request: {
    url: "{{baseUrl}}".replace(/\/+$/, "").replace(/\/v1$/, "") + "/v1/usage",
    method: "GET",
    headers: {
      "Authorization": "Bearer {{apiKey}}",
      "User-Agent": "cc-switch/1.0"
    }
  },
  extractor: function (response) {
    var quota = response.quota || {};
    var result = {
      isValid: response.isValid !== false,
      unit: response.unit || quota.unit || "USD"
    };
    if (typeof response.planName === "string") result.planName = response.planName;
    if (typeof response.remaining === "number" && response.remaining >= 0) {
      result.remaining = response.remaining;
    }
    if (quota.limit > 0) {
      result.used = quota.used;
      result.total = quota.limit;
    }
    if (!result.isValid) {
      result.invalidMessage = response.status
        ? String(response.status)
        : "照会に失敗しました";
    }
    return result;
  },
})
```

- Codex、Grok Build のプロバイダーのエンドポイントには `/v1` が付き、Claude Code のものには付きません。スクリプトは先に末尾の `/v1` を取り除いてから組み立てるので、どのアプリでも共通に使え、`usageBaseUrl` を書く必要はありません。
- 3 つのケースいずれでも `remaining` があります。ウォレット残高（`planName` はウォレット残高を示す名前）、サブスクリプショングループ（`planName` はグループ名で、日 / 週 / 月の上限のうち残りが最も少ないものを取る）、Key に個別の総額上限を設定した場合（このときは `quota.used` と `quota.limit` も返る）です。サブスクリプショングループに上限が一切設定されていない場合、`remaining` は `-1` で無制限を意味し、スクリプトはこの値を表示しません。
- レスポンスには `rate_limits`（Key の 5 時間 / 1 日 / 7 日のウィンドウ）と `subscription`（グループの日 / 週 / 月の使用量と上限）も含まれます。カードに行を分けて表示したい場合は、`extractor` から配列を返し、要素ごとに 1 行にできます。
- このスクリプトは、CC Switch のアプリ内「利用状況を設定」にある Sub2API テンプレート（v4.0.10 以降）とまったく同じです。ユーザーがあなたのサイトを手動で追加するときも、ワンクリックで選べます。
- Sub2API の管理画面にある「CC Switch へのインポート」ボタンが生成するリンクには独自の照会スクリプトが含まれています。`?.` と `??` を使った書き方で、CC Switch でも動作しますが、サブスクリプショングループが無制限の場合は `-1` と表示されます。

## MCP・プロンプト・Skill のリンク

<details>
<summary>API サービスとはあまり関係がありません。関連ツールもあわせて配布する場合に開いてください</summary>

この 3 種類は API サービスとはあまり関係がありませんが、関連ツールをあわせて配布するサイトもあるので、要点だけ簡単に挙げておきます。

**MCP**（`resource=mcp`）

- `apps` は必須で、カンマ区切りです：`claude`、`codex`、`gemini`、`agy`、`grokbuild`、`opencode`、`hermes`、`pi`、`mcode`。`openclaw` は無視されます。
- `config` は `{"mcpServers": {"<ID>": {...}}}` を Base64 にしたもので、1 本のリンクに複数のサーバーを含められます。
- ユーザーがすでに同じ ID のサーバーを持っている場合は、そのサーバーに新しいアプリのチェックが付くだけで、設定内容はリンクで上書きされません。設定を更新したい場合は別の ID にしてください。
- インポートと同時にクライアントの設定ファイルへ書き込まれます。`enabled` パラメーターは現在効果がありません。

**プロンプト**（`resource=prompt`）

- `app` は `claude`、`codex`、`gemini`、`agy`、`grokbuild`、`opencode`、`pi`、`mcode` のいずれかです。`openclaw`、`hermes` は CC Switch ではワークスペースエディターとメモリー管理が担当しているため、ディープリンクでプロンプトを配布することはお勧めしません。
- `name`、`content`（Base64 にした Markdown）は必須、`description` は任意です。
- `enabled` は確認ダイアログの「インポート後に有効にする」スイッチの初期値です。有効にすると、この内容でユーザーの現在のプロンプトファイル（`CLAUDE.md` など）が置き換えられるため、通常は書かないでください。

**Skill**（`resource=skill`）

- `repo` は必須で、形式は `owner/name`、GitHub 上の公開リポジトリでなければなりません。`branch` のデフォルトは `main` です。
- 追加されるのはリポジトリで、どの Skill をインストールするかはユーザーが自分で選びます。`directory` パラメーターは現在無視されます。

</details>

## リンクを生成するコード

### バックエンド（Node.js）

```javascript
function b64(text) {
  return Buffer.from(text, "utf8").toString("base64");
}

const usageScript = `({
  request: {
    url: "{{baseUrl}}/user/balance",
    method: "GET",
    headers: { "Authorization": "Bearer {{apiKey}}" }
  },
  extractor: function (r) {
    return { remaining: r.balance, unit: "USD" };
  }
})`;

function claudeImportLink(user) {
  const params = new URLSearchParams({
    resource: "provider",
    app: "claude",
    name: "Example AI",
    endpoint: "https://api.example.com",
    apiKey: user.apiKey,
    // 純正の Claude モデルを提供する場合、モデルはすべて空欄。DeepSeek、Kimi などネイティブでないモデルのときだけ次を書く：
    // model / haikuModel / sonnetModel / opusModel
    homepage: "https://example.com",
    // 内蔵アイコンに自社ブランドがない場合は自社アイコンの URL を指定する。「自社アイコンを付ける」を参照
    iconUrl: "https://example.com/static/logo.png",
    usageScript: b64(usageScript),
    usageEnabled: "true",
  });
  return `ccswitch://v1/import?${params}`;
}
```

OpenCode 向けに Anthropic 形式のリンクを生成する場合は、`app` と `apiFormat` を変えてモデルを加えるだけです。

```javascript
const params = new URLSearchParams({
  resource: "provider",
  app: "opencode",
  name: "Example AI",
  endpoint: "https://api.example.com/v1",
  apiKey: user.apiKey,
  apiFormat: "anthropic",
  model: "claude-sonnet-5-5",
});
```

### バックエンド（Python）

```python
import base64
from urllib.parse import urlencode

def b64(text: str) -> str:
    return base64.b64encode(text.encode("utf-8")).decode("ascii")

codex_toml = """\
model_provider = "example"
model = "example-coder"

[model_providers.example]
name = "Example AI"
base_url = "https://api.example.com/v1"
wire_api = "responses"
"""

def codex_import_link(api_key: str) -> str:
    params = {
        "resource": "provider",
        "app": "codex",
        "name": "Example AI",
        "apiKey": api_key,
        "configFormat": "toml",
        "config": b64(codex_toml),
    }
    return "ccswitch://v1/import?" + urlencode(params)
```

`urlencode` はデフォルトでスペースを `+` に、`+` を `%2B` に変換し、CC Switch の解析規則と一致します。

### ブラウザで生成する

ブラウザの `btoa` は中国語などの非 ASCII 文字に対応していないため、先に UTF-8 に変換します。

```javascript
function b64(text) {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  bytes.forEach((b) => (binary += String.fromCharCode(b)));
  return btoa(binary);
}
```

バックエンドで生成できるならバックエンドで生成してください。Key がフロントエンドの JavaScript を経由しないほうが安全です。

## リンクをボタンに載せる

推奨する流れ：

1. ボタン自体にはリンクを持たせません。ユーザーがクリックしたら、フロントエンドがバックエンドにそのユーザーのリンクを要求します。
2. バックエンドはユーザーがログイン済みであることを確認し、そのユーザー自身の Key でリンクを生成して返します（前の節の生成関数を使います）。
3. フロントエンドはリンクを受け取ったら `window.location.href = link` を実行します。ブラウザが CC Switch を開くかどうかを確認し、ユーザーが許可するとインポート確認ダイアログが開きます。

こうすればリンクがページのソースに書き込まれることはなく、ユーザーが実際にボタンを押したときに 1 回だけ生成されます。

### バックエンド：ユーザーごとにリンクを返す

Node.js（Express）の例です。`claudeImportLink` は前の節の生成関数で、`codexImportLink` は同じ書き方で別に用意します。

```javascript
const builders = {
  claude: claudeImportLink,
  codex: codexImportLink,
};

app.post("/api/ccswitch-link", requireLogin, (req, res) => {
  const build = builders[req.body.app];
  if (!build) return res.status(400).json({ error: "unsupported app" });
  // レスポンスにユーザーの Key が含まれる：キャッシュさせず、ログにも出さない
  res.set("Cache-Control", "no-store");
  res.json({ link: build(req.user) });
});
```

GET ではなく POST を使ってください。GET リクエストはブラウザに先読みされたり、キャッシュされたり、パラメーター付きでアクセスログに残ったりすることがあります。

### フロントエンド：ボタンと案内文

```html
<button type="button" onclick="importToCcSwitch('claude')">Claude Code にインポート</button>
<button type="button" onclick="importToCcSwitch('codex')">Codex にインポート</button>
<p class="hint">
  ブラウザが CC Switch を開くかどうか確認するので、「開く」を選んでください。押しても何も起きない場合は、まだインストールされていない可能性があります。
  <a href="https://ccswitch.io" target="_blank" rel="noopener">CC Switch をダウンロード</a>してインストールしてから、もう一度押してください。
</p>

<script>
  async function importToCcSwitch(app) {
    const res = await fetch("/api/ccswitch-link", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ app }),
    });
    if (!res.ok) {
      alert("インポート用リンクの生成に失敗しました。しばらくしてからもう一度お試しください");
      return;
    }
    const { link } = await res.json();
    window.location.href = link;
  }
</script>
```

補足：

- **遷移はクリックの直後に行ってください**。ブラウザが外部アプリの起動を許可するのはユーザーのクリックによるものだけで、クリックから時間が空きすぎるとクリックによる起動とみなさなくなるブラウザもあります。API は 1〜2 秒以内に返すようにし、リンクを受け取った後にさらに確認ダイアログを出してもう一度押させる、といったことはしないでください。
- **1 本のリンクでインポートできるのは 1 つのアプリだけです**。複数のアプリに対応する場合は、アプリごとにボタンを置くか、先にユーザーにクライアントを選んでもらってから同じ「インポート」ボタンを押してもらいます。アプリごとにエンドポイントや形式が異なることがあります。「対応アプリ」を参照してください。
- **CC Switch がインストールされていないとき、ブラウザは通常何も表示しません**。Web ページからも確実には判定できません。そのため、ボタンの横の案内文は判定結果を待たずに常に表示しておいてください。
- 同じボタンを 2 回押すと 2 件インポートされます。ユーザーが 1 回押した後は、ボタンの表示を「CC Switch に送信済み」に変えるか、数秒間無効にしておくとよいでしょう。

## セキュリティとプライバシー

- **Key を含むリンクは Key そのものと同じです**。ボタンの `href` に直接書いたリンクはページのソースに現れ、ブラウザの拡張機能に読まれる可能性があります。「リンクをボタンに載せる」のやり方に従い、ユーザーがクリックしたときに初めてバックエンドへリンクを要求すれば、これを避けられます。リンクはユーザーがログインした後のページでだけ生成し、公開ページ、一斉配信メール、コミュニティのお知らせなどには載せないでください。アクセスログやサードパーティの解析ツールにも残さないようにしてください。
- リンクは短いほど良いです。`config` 全体が長い場合、Windows では CC Switch に渡すコマンドラインに約 3.2 万文字の上限があり、長すぎるリンクは開けません。
- CC Switch がリンクの内容に対して行うのはホワイトリストによるフィルタリングと目立つ警告表示であり、ユーザーに代わって内容を保証するものではありません。確認ダイアログに表示される項目はすべてユーザーの目に入ります。あなたのサービスと関係のないものを入れないでください。ユーザーがこのボタンを信用しなくなります。

## デバッグ

1. まず自分のパソコンで一通り押してみて、確認ダイアログの各項目が想定どおりかを確かめ、インポート、切り替え、メッセージの送信まで行います。
2. 確認ダイアログにエラーが表示された場合は、そのエラーメッセージが原因です。上の「拒否されるリンクのまとめ」と照らし合わせて調べてください。
3. インポートには成功したが残高が表示されない場合：プロバイダーカードで「利用状況を設定」を開き、「スクリプトをテスト」を押してどんなエラーが出るか確認します。よくある原因は、リクエスト先のオリジンが異なる、残高 API が JSON を返していない、`extractor` で取り出すフィールドを間違えている、のいずれかです。
4. インポートには成功したが、切り替えた後にリクエストが失敗する場合：たいていは API の形式が合っていない（「対応アプリ」を参照）か、モデル名が間違っています。
5. `iconUrl` のアイコンが表示されない場合：ダウンロードに失敗してもエラーにはならず、原因は CC Switch のログ（`~/.cc-switch/logs/cc-switch.log`、`Skipped deep link icon` で検索）に記録されます。よくある原因は `Content-Type` が正しくない、画像が 256 KB を超えている、URL にログインが必要かホットリンク防止が有効になっている、のいずれかです。`curl -I` でレスポンスヘッダーを確認してください。

## 旧バージョンとの互換性

ユーザー全員が最新版に更新しているとは限りません。以下の動作は旧バージョンでは異なります。

| 動作 | v4.0.10 以降 | v3.19.0 – v4.0.9 |
|------|-----------|------------------|
| スクリプトあり、`usageEnabled` なし | 有効 | 無効 |
| Codex に同梱する `config.toml` | ホワイトリストに従ってプロバイダーテーブルとモデルフィールドを受け取る | エンドポイント、Key、モデル名だけを取り出し、残りは捨てて固定テンプレートで組み直す |
| Grok Build に同梱するモデルテーブル | 選ばれたテーブルのフィールドを受け取る | エンドポイント、Key、モデル名だけを取り出す |
| CC Switch が起動していないときにリンクを押す | 起動後に確認ダイアログが開く | まれに取りこぼし、もう一度押す必要がある |
| `apiFormat` | アプリごとに換算 | 無視し、常にデフォルト形式を使う |
| `app=pi` | 対応 | インポートを拒否 |
| `iconUrl` | ダウンロードしてカードのアイコンにする | 無視し、`icon` または名前の頭文字を表示 |
| 「インポート後に切り替える / 有効にする」 | 確認ダイアログのスイッチで、`enabled` は初期値 | リンクの `enabled` どおりにそのまま実行。OpenCode / OpenClaw / Hermes は常にアプリの設定へ書き込む |

どのバージョンでも同じ動作にしたい場合は、使用量クエリに `usageEnabled=true` を明示し、Codex / Grok Build の重要な情報（エンドポイント、Key、モデル）は `config` だけに入れず URL パラメーターにも書いてください。あなたの API がアプリのデフォルト形式でない場合、旧バージョンでインポートしたカードは形式が正しくなくなるので、ボタンの横で CC Switch の更新を促すとよいでしょう。
