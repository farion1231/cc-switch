# CC Switch ユーザーマニュアル

> Claude Code / Codex / Gemini CLI / OpenCode / OpenClaw オールインワンアシスタント

## 目次構成

```
CC Switch ユーザーマニュアル
│
├── 1. はじめに
│   ├── 1.1 ソフトウェア紹介
│   ├── 1.2 インストールガイド
│   ├── 1.3 インターフェース概要
│   ├── 1.4 クイックスタート
│   └── 1.5 個人設定
│
├── 2. プロバイダー管理
│   ├── 2.1 プロバイダーの追加
│   ├── 2.2 プロバイダーの切り替え
│   ├── 2.3 プロバイダーの編集
│   ├── 2.4 並べ替えと複製
│   └── 2.5 使用量クエリ
│
├── 3. 拡張機能
│   ├── 3.1 MCP サーバー管理
│   ├── 3.2 Prompts プロンプト管理
│   └── 3.3 Skills スキル管理
│
├── 4. プロキシと高可用性
│   ├── 4.1 プロキシサービス
│   ├── 4.2 アプリケーション接管
│   ├── 4.3 フェイルオーバー
│   ├── 4.4 使用量統計
│   └── 4.5 モデルテスト
│
└── 5. よくある質問
    ├── 5.1 設定ファイルの説明
    ├── 5.2 FAQ
    ├── 5.3 ディープリンクプロトコル
    └── 5.4 環境変数の競合
```

## ファイル一覧

### 1. はじめに

| ファイル | 内容 |
|------|------|
| [1.1-introduction.md](./1-getting-started/1.1-introduction.md) | ソフトウェア紹介、主要機能、対応プラットフォーム |
| [1.2-installation.md](./1-getting-started/1.2-installation.md) | Windows/macOS/Linux インストールガイド |
| [1.3-interface.md](./1-getting-started/1.3-interface.md) | インターフェースレイアウト、ナビゲーションバー、プロバイダーカードの説明 |
| [1.4-quickstart.md](./1-getting-started/1.4-quickstart.md) | 5 分でできるクイックスタートチュートリアル |
| [1.5-settings.md](./1-getting-started/1.5-settings.md) | 言語、テーマ、ディレクトリ、クラウド同期の設定 |

### 2. プロバイダー管理

| ファイル | 内容 |
|------|------|
| [2.1-add.md](./2-providers/2.1-add.md) | プリセットの使用、カスタム設定、統一プロバイダー |
| [2.2-switch.md](./2-providers/2.2-switch.md) | メイン画面での切り替え、トレイでの切り替え、反映方法 |
| [2.3-edit.md](./2-providers/2.3-edit.md) | 設定の編集、API Key の変更、バックフィル機能 |
| [2.4-sort-duplicate.md](./2-providers/2.4-sort-duplicate.md) | ドラッグで並べ替え、プロバイダーの複製、削除 |
| [2.5-usage-query.md](./2-providers/2.5-usage-query.md) | 使用量クエリ、残額表示、複数プラン表示 |

### 3. 拡張機能

| ファイル | 内容 |
|------|------|
| [3.1-mcp.md](./3-extensions/3.1-mcp.md) | MCP プロトコル、サーバーの追加、アプリバインド |
| [3.2-prompts.md](./3-extensions/3.2-prompts.md) | プリセットの作成、有効化の切り替え、スマートバックフィル |
| [3.3-skills.md](./3-extensions/3.3-skills.md) | スキルの発見、インストール・アンインストール、リポジトリ管理 |

### 4. プロキシと高可用性

| ファイル | 内容 |
|------|------|
| [4.1-service.md](./4-proxy/4.1-service.md) | プロキシの起動、設定項目、実行状態 |
| [4.2-takeover.md](./4-proxy/4.2-takeover.md) | アプリケーション接管、設定変更、ステータス表示 |
| [4.3-failover.md](./4-proxy/4.3-failover.md) | フェイルオーバーキュー、サーキットブレーカー、ヘルスステータス |
| [4.4-usage.md](./4-proxy/4.4-usage.md) | 使用量統計、トレンドグラフ、料金設定 |
| [4.5-model-test.md](./4-proxy/4.5-model-test.md) | モデルテスト、ヘルスチェック、レイテンシテスト |

### 5. よくある質問

| ファイル | 内容 |
|------|------|
| [5.1-config-files.md](./5-faq/5.1-config-files.md) | CC Switch のストレージ、CLI 設定ファイル形式 |
| [5.2-questions.md](./5-faq/5.2-questions.md) | よくある質問と回答 |
| [5.3-deeplink.md](./5-faq/5.3-deeplink.md) | ディープリンクプロトコル、生成と使用方法 |
| [5.4-env-conflict.md](./5-faq/5.4-env-conflict.md) | 環境変数の競合検出と対処 |

## クイックリンク

- **初めての方**：[1.1 ソフトウェア紹介](./1-getting-started/1.1-introduction.md) からお読みください
- **インストールの問題**：[1.2 インストールガイド](./1-getting-started/1.2-installation.md) をご確認ください
- **プロバイダーの設定**：[2.1 プロバイダーの追加](./2-providers/2.1-add.md) をご確認ください
- **プロキシの使用**：[4.1 プロキシサービス](./4-proxy/4.1-service.md) をご確認ください
- **お困りの方**：[5.2 FAQ](./5-faq/5.2-questions.md) をご確認ください

## バージョン情報

- ドキュメントバージョン：v3.12.0
- 最終更新：2026-03-09
- CC Switch v3.12.0+ 対応

## コントリビュート

Issue や PR でドキュメントの改善にご協力ください：

- [GitHub Issues](https://github.com/farion1231/cc-switch/issues)
- [GitHub Repository](https://github.com/farion1231/cc-switch)
