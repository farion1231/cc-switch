# CC Switch v3.8.0

> 永続化アーキテクチャを刷新し、クラウド同期の土台を構築

**[English →](release-note-v3.8.0-en.md) | [中文版 →](release-note-v3.8.0-zh.md)**

---

## 概要

CC Switch v3.8.0 はデータ永続化レイヤーと UI を大幅に作り替え、今後のクラウド同期やローカルプロキシ機能に向けた基盤を整えたメジャーアップデートです。

**リリース日**: 2025-11-28  
**コミット数**: v3.7.1 以降 51 commits  
**変更量**: 207 files, +17,297 / -6,870 lines

---

## 主要アップデート

### 永続化アーキテクチャの刷新

単一の JSON 保存から、階層化された SQLite + JSON の二層構造へ移行。

**アーキテクチャ変更**:

```
v3.7.x (旧)                       v3.8.0 (新)
┌─────────────────┐              ┌─────────────────────────────────┐
│  config.json    │              │  SQLite (同期対象データ)         │
│  ┌───────────┐  │              │  ├─ providers     プロバイダ設定 │
│  │ providers │  │              │  ├─ mcp_servers   MCP サーバー   │
│  │ mcp       │  │     ──>      │  ├─ prompts       プロンプト     │
│  │ prompts   │  │              │  ├─ skills        Skills         │
│  │ settings  │  │              │  └─ settings      汎用設定       │
│  └───────────┘  │              ├─────────────────────────────────┤
└─────────────────┘              │  JSON (デバイス固有データ)        │
                                 │  └─ settings.json ローカル設定    │
                                 │     ├─ ウィンドウ位置            │
                                 │     ├─ パスの上書き              │
                                 │     └─ 現在のプロバイダ ID        │
                                 └─────────────────────────────────┘
```

**二層構造の設計**:

| レイヤー | ストレージ | データ種別                          | 同期戦略         |
| -------- | ---------- | ----------------------------------- | ---------------- |
| クラウド | SQLite     | Providers, MCP, Prompts, Skills     | 将来同期対象     |
| デバイス | JSON       | ウィンドウ状態、ローカルパス         | ローカル保持     |

**実装ポイント**:

- **スキーマバージョン管理**: DB 構造のマイグレーションに対応
- **SQL インポート/エクスポート**: `backup.rs` が SQL ダンプをサポート
- **トランザクション対応**: SQLite ネイティブで整合性を確保
- **自動マイグレーション**: 初回起動で `config.json` から自動移行

**モジュール分割**:

```
database/
├── mod.rs              Database 構造体と初期化
├── schema.rs           テーブル定義とスキーマ移行
├── backup.rs           SQL インポート/エクスポートとスナップショット
├── migration.rs        JSON → SQLite 変換エンジン
└── dao/                DAO レイヤー
    ├── providers.rs    プロバイダ CRUD
    ├── mcp.rs          MCP CRUD
    ├── prompts.rs      プロンプト CRUD
    ├── skills.rs       Skills CRUD
    └── settings.rs     設定 Key-Value 保存
```

---

### 新しいユーザーインターフェース

よりモダンな見た目と操作感に再設計。

- レイアウト全面刷新、コンポーネントスタイルを統一
- トランジションを滑らかにし、視覚的階層を最適化
- メインビューのオーバースクロールバウンスを無効化
- ブラウザ互換性向上のため Tailwind CSS を v4→v3.4 にダウングレード

---

### 日語化

UI が日本語に対応し、国際化が 3 言語（中/英/日）へ拡大。

---

## 新機能

### Skills 递帰スキャン

Skills 管理がリポジトリを再帰的に走査し、ネストされた `SKILL.md` を自動検出。

- 複数階層のディレクトリに対応
- すべての `SKILL.md` を自動発見
- パスをキーにした重複排除で同名スキルを許容

### プロバイダアイコン設定

プリセットがデフォルトアイコンを持ち、複製してもアイコンを保持。カスタム色も設定可能。

### フォームバリデーション強化

必須項目にリアルタイム検証と分かりやすいエラーメッセージを追加し、トースト通知を統一。

### 自動起動

Windows/macOS/Linux で自動起動をサポート。

- 設定画面からワンクリックで ON/OFF
- Registry / LaunchAgent / XDG autostart を使用

### 新プロバイダプリセット

- **MiniMax** - 公式パートナー

---

## バグ修正

### 重要修正

**カスタムエンドポイント消失**

- 原因: SQLite の `INSERT OR REPLACE` が内部で `DELETE + INSERT` を実行し、外部キーのカスケード削除が発生
- 対応: 既存プロバイダ更新を `UPDATE` に変更

**Gemini 設定**

- カスタム環境変数が `.env` に正しく書き込まれない問題を修正
- 認証設定が他ファイルに誤って書き込まれる問題を修正

**プロバイダ検証**

- 現在プロバイダ ID が欠落している場合のバリデーションエラーを修正
- 複製時にアイコンフィールドが失われる問題を修正

### プラットフォーム互換性

**Linux**

- WebKitGTK の DMA-BUF 描画問題を解消
- ユーザーの `.desktop` カスタマイズを保持

### その他修正

- アプリ切り替え時の不要な使用量クエリを削減
- DMXAPI プリセットの誤ったトークンフィールドを修正
- Deeplink コンポーネントの欠損翻訳キーを補完
- 使用量スクリプトテンプレート初期化を修正

---

## 技術的改善

### アーキテクチャ再編

**Provider Service のモジュール化**:

```
services/provider/
├── mod.rs          追加/更新/削除/切替/検証の中核
├── live.rs         ライブ設定ファイル操作
├── gemini_auth.rs  Gemini 認証タイプ検出
├── endpoints.rs    カスタムエンドポイント管理
└── usage.rs        使用量スクリプト実行
```

**Deeplink のモジュール化**:

```
deeplink/
├── mod.rs       エクスポート
├── parser.rs    URL パース
├── provider.rs  プロバイダ取り込み
├── mcp.rs       MCP 取り込み
├── prompt.rs    プロンプト取り込み
├── skill.rs     Skills 取り込み
└── utils.rs     ユーティリティ
```

### コード品質

- レガシーな JSON 時代のインポート/エクスポート死蔵コードを削除
- 使われていない MCP 型を削除し、エラーハンドリングを統一
- テストを SQLite バックエンドに移行し、MSW ハンドラを最新 API に合わせて更新

---

## 技術統計

```
全体変更:
- コミット: 51
- 変更ファイル: 207
- 追加: +17,297 行
- 削除: -6,870 行
- 純増: +10,427 行

コミット種別:
- fix: 25
- refactor: 11
- feat: 9
- test: 1
- other: 5

変更箇所:
- フロントエンド: 112 files
- Rust バックエンド: 63 files
- テスト: 20 files
- i18n: 3 files
```

---

## マイグレーションガイド

### v3.7.x からのアップグレード

**自動マイグレーション**（初回起動時）:

1. `config.json` の存在を検出
2. 全データをトランザクションで SQLite に移行
3. デバイス設定を `settings.json` へ移行
4. 移行成功の通知を表示

**データ保護**:

- 元の `config.json` は保持（削除しない）
- 失敗時はエラーダイアログを表示し、`config.json` を温存
- Dry-run モードで検証可能

---

## ダウンロード & インストール

### システム要件

- **Windows**: Windows 10+
- **macOS**: macOS 10.15 (Catalina)+
- **Linux**: Ubuntu 22.04+ / Debian 11+ / Fedora 34+

### ダウンロード

[Releases](https://github.com/farion1231/cc-switch/releases/latest) から入手:

- **Windows**: `CC-Switch-v3.8.0-Windows.msi` または `-Portable.zip`
- **macOS**: `CC-Switch-v3.8.0-macOS.tar.gz` または `.zip`
- **Linux**: `CC-Switch-v3.8.0-Linux.AppImage` または `.deb`

### Homebrew (macOS)

```bash
brew tap farion1231/ccswitch
brew install --cask cc-switch
```

アップデート:

```bash
brew upgrade --cask cc-switch
```

---

## 謝辞

### コントリビューター

- [@YoVinchen](https://github.com/YoVinchen) - UI とデータベースリファクタ
- [@farion1231](https://github.com/farion1231) - バグ修正と機能拡張
- コミュニティの皆さん - テストとフィードバック

### スポンサー

**Zhipu AI** - GLM CODING PLAN スポンサー  
[10% オフリンク](https://z.ai/subscribe?ic=8JVLJQFSKB)

**PackyCode** - API リレーサービスパートナー  
[登録時に「cc-switch」で 10% オフ](https://www.packyapi.com/register?aff=cc-switch)

**ShandianShuo** - ローカルファースト音声入力  
[Mac/Windows 無料ダウンロード](https://shandianshuo.cn)

**MiniMax** - MiniMax M2 CODING PLAN スポンサー  
[ブラックフライデーセール中、$2 から](https://platform.minimax.io/subscribe/coding-plan)

---

## フィードバック & サポート

- **Issue**: [GitHub Issues](https://github.com/farion1231/cc-switch/issues)
- **Discussions**: [GitHub Discussions](https://github.com/farion1231/cc-switch/discussions)
- **ドキュメント**: [README](../README.md)
- **更新履歴**: [CHANGELOG.md](../CHANGELOG.md)

---

## 今後のロードマップ

**v3.9.0 予告（予定）**:

- ローカルプロキシ機能

続報にご期待ください！

---

**Happy Coding!**
