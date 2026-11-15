# CC Switch ユーザーマニュアル

> Claude Code / Claude Desktop / Codex / Gemini CLI / Grok Build / OpenCode / OpenClaw / Hermes / Pi / MiniMax Code オールインワンアシスタント

## 目次構成

```
📚 CC Switch ユーザーマニュアル
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
│   ├── 2.5 使用量クエリ
│   └── 2.6 Claude Desktop
│
├── 3. 拡張機能
│   ├── 3.1 MCP サーバー管理
│   ├── 3.2 Prompts プロンプト管理
│   ├── 3.3 Skills スキル管理
│   ├── 3.4 セッションマネージャー
│   └── 3.5 ワークスペースとメモリー
│
├── 4. ローカルルーティングと高可用性
│   ├── 4.1 ローカルルーティングサービス
│   ├── 4.2 アプリケーションルーティング
│   ├── 4.3 フェイルオーバー
│   ├── 4.4 使用量統計
│   ├── 4.5 接続チェック
│   └── 4.6 集約モード
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
| [1.3-interface.md](./1-getting-started/1.3-interface.md) | サイドバーのレイアウト、接続モードのタブ、プロバイダーカードの説明 |
| [1.4-quickstart.md](./1-getting-started/1.4-quickstart.md) | 5 分でできるクイックスタートチュートリアル |
| [1.5-settings.md](./1-getting-started/1.5-settings.md) | 設定の 6 グループ：一般、アプリ設定、ローカルルーティング、ネットワーク、データ、情報 |

### 2. プロバイダー管理

| ファイル | 内容 |
|------|------|
| [2.1-add.md](./2-providers/2.1-add.md) | プリセットの使用、カスタム設定、統一プロバイダー |
| [2.2-switch.md](./2-providers/2.2-switch.md) | 直接接続／ルーティング／集約、トレイでの切り替え、反映方法 |
| [2.3-edit.md](./2-providers/2.3-edit.md) | 設定の編集、API Key の変更、グローバル設定と編集の競合 |
| [2.4-sort-duplicate.md](./2-providers/2.4-sort-duplicate.md) | ドラッグで並べ替え、プロバイダーの複製、削除 |
| [2.5-usage-query.md](./2-providers/2.5-usage-query.md) | 使用量クエリ、残額表示、複数プラン表示 |
| [2.6-claude-desktop.md](./2-providers/2.6-claude-desktop.md) | Claude Desktop サードパーティプロバイダー、直結モード、モデルマッピング |

### 3. 拡張機能

| ファイル | 内容 |
|------|------|
| [3.1-mcp.md](./3-extensions/3.1-mcp.md) | MCP プロトコル、サーバーの追加、アプリバインド |
| [3.2-prompts.md](./3-extensions/3.2-prompts.md) | プロンプトの作成、有効化の切り替え、対象ファイル |
| [3.3-skills.md](./3-extensions/3.3-skills.md) | スキルの発見、インストール・アンインストール、リポジトリ管理 |
| [3.4-sessions.md](./3-extensions/3.4-sessions.md) | セッションマネージャー：閲覧、検索、再開、削除 |
| [3.5-workspace.md](./3-extensions/3.5-workspace.md) | ワークスペースファイルとデイリーメモリー（OpenClaw） |

### 4. ローカルルーティングと高可用性

| ファイル | 内容 |
|------|------|
| [4.1-service.md](./4-proxy/4.1-service.md) | ローカルルーティングの起動、設定項目、API フォーマット変換 |
| [4.2-routing.md](./4-proxy/4.2-routing.md) | アプリケーションルーティング、設定変更、ステータス表示 |
| [4.3-failover.md](./4-proxy/4.3-failover.md) | フェイルオーバーキュー、サーキットブレーカー、ヘルスステータス |
| [4.4-usage.md](./4-proxy/4.4-usage.md) | 使用量統計、トレンドグラフ、料金設定 |
| [4.5-model-test.md](./4-proxy/4.5-model-test.md) | 接続チェック、チェックパラメータ |
| [4.6-aggregation.md](./4-proxy/4.6-aggregation.md) | 集約モード、デフォルトプロバイダー、ccs- プレフィックス、Codex の再起動 |

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
- **Claude Desktop の利用**：[2.6 Claude Desktop](./2-providers/2.6-claude-desktop.md) をご確認ください
- **ローカルルーティングの使用**：[4.1 ローカルルーティングサービス](./4-proxy/4.1-service.md) をご確認ください
- **お困りの方**：[5.2 FAQ](./5-faq/5.2-questions.md) をご確認ください

## バージョン情報

- ドキュメントバージョン：v4.0.4
- 最終更新：2026-10-07
- CC Switch v4.0.4+ 対応

### 最近の主な変更（v4.0）

- **インターフェースの刷新**：上部のナビゲーションが左サイドバーになり、使用量、アカウント、MCP、Skills、プロンプト、セッション、アプリがそれぞれ独立したページになりました。設定は一般、アプリ設定、ローカルルーティング、ネットワーク、データ、情報の 6 グループに分かれています — 詳細は [1.3 インターフェース概要](./1-getting-started/1.3-interface.md)、[1.5 個人設定](./1-getting-started/1.5-settings.md)
- **直接接続 / ルーティング / 集約の 3 つの接続モード**：各アプリページの上部で選び、直接接続へは 1 ステップで戻せます — 詳細は [2.2 プロバイダーの切り替え](./2-providers/2.2-switch.md)
- **集約モードを追加**：Claude Code と Codex で、1 つのセッションの中で複数のプロバイダーのモデルを使い分けられます — 詳細は [4.6 集約モード](./4-proxy/4.6-aggregation.md)
- **切り替えはキーとなるフィールドだけを変更**：アドレス、キー、モデルなど以外の設定はそのまま残り、共通設定スニペットは使われなくなりました — 詳細は [2.3 プロバイダーの編集](./2-providers/2.3-edit.md)
- **アカウント**：ChatGPT、GitHub Copilot、xAI などのログインをサイドバーの「アカウント」でまとめて管理します — 詳細は [1.5 個人設定](./1-getting-started/1.5-settings.md)
- **「アプリ」ページ**：各 CLI ツールのインストールとアップグレード、サイドバーに表示するアプリの選択ができます — 詳細は [1.2 インストールガイド](./1-getting-started/1.2-installation.md)
- すべての変更点は [v4.0 リリースノート](../../release-notes/v4.0.4-ja.md) をご覧ください

## コントリビュート

Issue や PR でドキュメントの改善にご協力ください：

- [GitHub Issues](https://github.com/farion1231/cc-switch/issues)
- [GitHub Repository](https://github.com/farion1231/cc-switch)
