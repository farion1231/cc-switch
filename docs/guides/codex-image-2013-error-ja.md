# Codex で 10MB を超える画像を添付すると "context window exceeds limit (2013)" という誤ったエラーが出る件

> CC Switch v3.16.4 以前が対象。上流 issue #4820 で報告済み。

## このガイドが解決する問題

CC Switch 経由で Codex CLI を MiniMax 系プロバイダー
(`MiniMax-M3`、`MiniMax-M2.7` など) にルーティングしているユーザーが、
Codex の会話に画像を添付すると以下のエラーに遭遇することがあります:

```
CC Switch local proxy failed while handling Codex endpoint /responses.
Provider: MiniMax; model: MiniMax-M3;
upstream_status: HTTP 400;
cause: invalid params, context window exceeds limit (2013)
```

エラーメッセージは **"context window"** と言っているため、
ユーザーは会話履歴をクリアしたり、コンテキストウィンドウが大きいモデルに
切り替えたりと、間違った調査パスに進んでしまいます。実際の原因は別です。

## 根本原因

MiniMax API は複数の異なる失敗モードに対して同じエラーコード **2013** を
返します。アップストリームのペイロードは以下のいずれかです:

```json
// 本当のコンテキストウィンドウ超過 (200K トークン超過)
{"error":{"message":"prompt is too long (2013)",
          "code":"invalid_prompt"}}

// 単一の画像添付が 10MB 超
{"error":{"message":"media exceeds size limit: max 10485760 bytes (2013)",
          "code":"invalid_prompt"}}
```

CC Switch のアップストリームエラーラッパーは現在、すべての `2013` を
一律「context window exceeds limit」という人間向け文字列に翻訳しており、
2 つのケースを区別していません。結果として、実際の問題がコンテキスト長
なのかメディアサイズなのかに関わらず、ユーザーは同じメッセージを見ること
になります。

`https://api.minimaxi.com/v1/responses` への直接 `curl` で検証済み:

```bash
$ curl .../v1/responses -d '{ "model": "MiniMax-M3",
                              "input": [...10MB を超える base64 画像...] }'
{"error":{"message":"media exceeds size limit: max 10485760 bytes (2013)",
          "code":"invalid_prompt"}}
```

`MiniMax-M3` は実際には 200K+ トークンのコンテキストをサポートしています。
トークン数は少ないが画像バイト数が大きいリクエストは、コンテキストでは
なく `2013` / `media exceeds size limit` で失敗します。

## どちらのケースかを見分ける方法

| 症状 | 推定原因 |
|---|---|
| 長い会話履歴 + 画像 | 本当のコンテキスト超過 (稀) |
| 単一の 10MB 超画像 ( Retina スクショ、PDF ページなど) | メディアサイズ制限 (一般的) |
| 会話が 1K トークン未満 + 画像で失敗 | ほぼ確実にメディアサイズ |

最も速い確認方法: 同じペイロードを直接アップストリーム API
(`api.minimaxi.com`) に `curl` し、`error.message` フィールドを
そのまま読む。

## 推奨されるアップストリーム修正 (issue #4820 で提案済み)

1. CC Switch が MiniMax 系プロバイダーから `2013` を受信したとき、
   アップストリームの `error.message` 文字列を Codex UI にそのまま転送する
   (汎用文字列で置換しない)。アップストリームのメッセージにはすでに
   "media" か "prompt" かが書かれているので、翻訳は不要。
2. オプション: forwarder のエラーマッピング層でアップストリームエラーを
   分類し、小さなルックアップテーブルから選ぶ
   (`2013` + `"media"` → `"Image exceeds 10 MB limit"`、
   `2013` + `"prompt"` → `"Conversation too long"`)。

これは forwarder のエラーマッピング層の小さな変更で、プロバイダー側の
変更は不要。

## エンドユーザーの回避策 (当面)

アップストリームの修正がリリースされるまで、
[Konsn666/codex-img-prep](https://github.com/Konsn666/codex-img-prep)
というコンパニオンツールで画像をクライアントサイド前処理し、
MiniMax の 10MB メディア上限に常に収まるようにします。ワンライナー
インストール:

```bash
curl -fsSL https://raw.githubusercontent.com/Konsn666/codex-img-prep/main/install-remote.sh | bash
```

インストール後:

- クリップボードにコピーしたスクリーンショットは 2 秒以内に自動リサイズ
  (最大 1920px、JPEG q85、≤ 8MB)。
- Finder で画像を右クリック → **Quick Actions → Compress for Codex**。
- シェルエイリアス: `prep-clip`、`prep-img`、`prep-watch`。

さらに installer は Codex プロバイダーの `meta.apiFormat` を
`openai_chat` から `openai_responses` に変更し、forwarder での
lossy な `/responses → /chat/completions` 変換を避け、ログと
実際のリクエストパスを一致させます。

## 環境

- CC Switch: v3.16.4
- Codex CLI: 最新、`wire_api = "responses"`
- MiniMax-M3 via `api.minimaxi.com`
- macOS 15.4 (Apple Silicon) — 回避策は組み込みの `sips` ツールを
  使用するため macOS のみ。Linux/Windows ユーザーは `sips` を
  ImageMagick などに置き換える必要があります。

## 関連

- アップストリーム issue: https://github.com/farion1231/cc-switch/issues/4820
- 回避策リポジトリ: https://github.com/Konsn666/codex-img-prep
- エンドツーエンド検証: 38.64MB スクリーンショット → パイプライン後
  1.04MB → CC Switch → MiniMax → 200 OK (視覚認識正解)。
---

## 中国語版 / 中文版

このガイドの中国語訳は [Konsn666/codex-img-prep/blob/main/README.zh-CN.md](https://github.com/Konsn666/codex-img-prep/blob/main/README.zh-CN.md)、
アップストリームの中国語バグレポートは [ISSUE.zh-CN.md](https://github.com/Konsn666/codex-img-prep/blob/main/ISSUE.zh-CN.md) で参照できます。
