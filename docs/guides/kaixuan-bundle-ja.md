# kaixuan bundle ワンクリック導入ガイド（ja）

> 対象バージョン: v3.20.4+（feat: kaixuan bundle）
> 要約: `~/.codex` の用意が終われば、cc-switch UI 上部に「kaixuan bundle」カードが現れます。「インストール/再インストール」を押すだけで、开軒 kxpms.cn とローカル 8782 の両方を Codex に登録し、フェイルオーバーを自動で有効化します。

## 1. 前提条件

### 1.1 公開ゲートウェイ（开軒 kxpms.cn）

リモートゲートウェイは kx-citus-pg17 + llm-gateway-go コンテナ構成で、47.97.111.154（NPS+VPN）の背後にデプロイ済みです。必要なのは `https://llm.kxpms.cn/v1/models` に到達可能であることだけです:

```bash
curl -s -o /dev/null -w "%{http_code}\n" --max-time 5 https://llm.kxpms.cn/v1/models
# 期待値: 401（ゲートウェイは稼働中だがキー不足）または 200
```

### 1.2 ローカルゲートウェイ（127.0.0.1:8782）

ローカルで llm-gateway-go を動かします。優先度順に 3 つの方式があります:

| 方式                                 | パス                                                                            | トリガー                                                                                     |
| ------------------------------------ | ------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| **(a) カスタムコマンド**             | `KAIXUAN_GATEWAY_START_CMD` 環境変数                                            | 「8782 を再起動」ボタンを押したとき cc-switch が `sh -c "$KAIXUAN_GATEWAY_START_CMD"` を実行 |
| **(b) 慣習のプロジェクトスクリプト** | `~/kaixuan/llm-gateway-local/start.sh`                                          | ファイルが存在すれば実行。実行権限がなければ `chmod +x`                                      |
| **(c) Docker フォールバック**        | `docker run -d --name llm-gateway-local-8782 -p 8782:8782 llm-gateway-go:local` | 上 2 つが見つからない場合に実行。ローカルに `llm-gateway-go:local` イメージが必要            |

### 1.3 ローカルゲートウェイの確認

```bash
curl -s -o /dev/null -w "%{http_code}\n" --max-time 3 http://127.0.0.1:8782/v1/models
# 期待値: 200
```

## 2. cc-switch 側の操作

### 2.1 ワンクリックインストール

1. cc-switch を開いて **Codex** アプリタブに切り替える
2. 上部に「kaixuan bundle」カードが表示される
3. 「メインエンドポイント API Key」に入力する:
   - プレースホルダ形式: `$KAIXUAN_KXPMS_KEY`
   - または平文形式: `sk-live-xxxxx`（非推奨。DB に保存される）
4. 「バックアップエンドポイント API Key」は空でも可（同じキーを使う）または別キーを入力
5. 「インストール/再インストール」をクリック

バックエンドは次を行います:

- 2 つの provider を書き込む: `kaixuan-kxpms`（メイン）+ `kaixuan-local-8782`（サブ）
- 両方をフェイルオーバーキューに追加し、`sort_index` を採番
- 当該 AppType のプロキシ接管を有効化し、`auto_failover_enabled=true` を設定
- P1 = `kaixuan-kxpms` に切り替え

これらの書き込みは**単一データベーストランザクション**内で行われます。途中で失敗すればバッチ全体がロールバックされるため、インストールに失敗しても「provider は書かれたのにキュー未登録」のような中途半端な状態は残りません。

### 2.2 ステータス表示

カードには 2 つのヘルスランプがあります:

| エンドポイント | 正常                   | 異常                    |
| -------------- | ---------------------- | ----------------------- |
| `kxpms`        | <Wifi/> + `200 · 35ms` | <WifiOff/> + エラー内容 |
| `local-8782`   | 同上                   | 同上                    |

15 秒ごとにポーリングされ、ウィンドウがフォーカスれると即時更新されます。8782 に到達できない場合は、8782 行の右側にある ▶ ボタンで起動できます。

## 3. トラブルシューティング

### 3.1 \_bundle install が "bundle unknown" で失敗する

```bash
# バックエンドに登録されているか確認
sqlite3 ~/.cc-switch/cc-switch.db "SELECT 1;"
# cc-switch を開き直して startup を走らせるか、アプリを再起動
```

### 3.2 curl では通るのにヘルスランプが赤い

cc-switch プロセスが環境を継承できているか確認してください:

```bash
ps eww -p $(pgrep -f "cc-switch") | tr ' ' '\n' | grep -i "kaixuan\|http_proxy"
# macOS の GUI アプリは既定で ~/.zshenv を読まない。launchctl setenv を使う（env-vars ドキュメント参照）
```

### 3.3 ヘルスランプが黄色/赤だが実際には使える

reqwest の既定は connect timeout 200 ms / 全体 timeout 500 ms のため、公開ネットワークが混雑すると一瞬で落ちることがあります。`src-tauri/src/gateway_health.rs::shared_client` の timeout を調整してください:

```rust
Client::builder()
    .timeout(Duration::from_millis(2000))  // 大きくする
    .connect_timeout(Duration::from_millis(1000))
```

### 3.4 ローカルゲートウェイを起動したのに到達できない

1. cc-switch の起動コマンドが実際に実行されたか確認: `start_local_gateway` が返した `stdout` / `stderr`
2. コンテナを確認: `docker ps | grep llm-gateway-local-8782`
3. コンテナログを確認: `docker logs llm-gateway-local-8782`

## 4. 拡張: bundle を追加する

`src-tauri/src/provider_bundle.rs::bundles()` に 1 行追加します:

```rust
pub fn bundles() -> Vec<BundleSpec> {
    vec![kaixuan_bundle(), my_other_bundle()]
}
```

UI 側の `useKaixuanBundles()` はすべての bundle を返すので、UI の変更は不要です。

## 5. ポート上書きとクロスプラットフォーム

### 5.1 ローカルゲートウェイのポートが 8782 以外の場合

```bash
# cc-switch 起動前に export。kxpms 側には影響しない
export KAIXUAN_LOCAL_GATEWAY_PORT=8899
launchctl setenv KAIXUAN_LOCAL_GATEWAY_PORT 8899  # macOS GUI にも必要
open -a cc-switch
```

bundle インストール時は base_url として `http://127.0.0.1:8899/v1` を使い、起動エントリポイントも 8899 番ポートを使います。カードには実際にプローブしているポートが表示されるため、8782 以外を探索しているのに UI が「8782」と書くことはありません。

### 5.2 Windows

`start_local_gateway` はシェル経由で動作し、Windows では `/bin/sh` ではなく自動的に `cmd.exe /C` を使います。ただし `~/kaixuan/llm-gateway-local/start.sh` というパスは Windows では無意味です。Windows ユーザーは `KAIXUAN_GATEWAY_START_CMD` のカスタムコマンドを使うか、Docker フォールバック（`docker run -d --name llm-gateway-local-8782 -p 8782:8782 llm-gateway-go:local`。Docker Desktop が必要）に頼ってください。

## 6. 既存の「独立プリセット」との関係

|                  | 旧方式                                                                         | 新しい kaixuan bundle                                                              |
| ---------------- | ------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------- |
| UI 操作          | 「开軒 LLM 网关」プリセットと「本地 LLM 网关 (8782)」プリセットを 2 回クリック | 「インストール/再インストール」で 1 回                                             |
| フェイルオーバー | 2 つとも手動でキューに追加                                                     | 自動登録 + auto_failover 有効化                                                    |
| デフォルトモデル | 独立した `default` の切り替え                                                  | インストール後 P1 = kxpms                                                          |
| 設定の上書き     | 再インストールでユーザーが編集した API キーを上書きする                        | 同じ id が存在する場合、ユーザーの `settings_config` を保持（membership のみ更新） |

既存の 2 つの独立プリセット（`codexProviderPresets.ts` の `开軒 LLM 网关` / `本地 LLM 网关 (8782)`）は残っており、個別に使い続けることもできます。競合はしません。

## 7. 2 つのエンドポイント間の切り替え

各エンドポイントは独自の TOML テーブル id（`kxpms` と `local8782`）を持ち、トップレベルの `model_provider` が有効な方を指します。そのため切り替え時に衝突せず、両方のテーブルが live 設定に残ります。切り替えは設定ファイルを壊すことなく新しい設定を書き込みます。Codex がすでに起動中の場合、cc-switch は再起動を提案し、実行中のセッションにも新しいプロバイダを反映させられます。

両エンドポイントのモデルは `/model` ピッカーに `claude-opus-5`（有効なエンドポイント）と `claude-opus-5@kxpms` / `claude-opus-5@local8782`（もう一方）として一緒に表示されます。`@` 付きのエントリは相手エンドポイントのカタログを*見えるように*するためのもので、実際のリクエスト経路は引き続きトップレベルの `model_provider` が決定します。
