# CC Switch v3.9.0

> ローカル API プロキシ、自動フェイルオーバー、Universal Provider、多アプリ対応の強化

**[English →](release-note-v3.9.0-en.md) | [中文版 →](release-note-v3.9.0-zh.md)**

---

## 概要

CC Switch v3.9.0 は v3.9 ベータ（`3.9.0-1`、`3.9.0-2`、`3.9.0-3`）の安定版です。
ローカル API プロキシ（アプリ別テイクオーバー対応）、自動フェイルオーバー、Universal Provider を追加し、Claude Code / Codex / Gemini CLI の安定性と操作性を大きく改善しました。

**リリース日**：2026-01-07

---

## ハイライト

- ローカル API プロキシ：Claude Code / Codex / Gemini CLI を統一的にプロキシ
- 自動フェイルオーバー：サーキットブレーカーとアプリ別のフェイルオーバーキュー
- Universal Provider：1つの設定を複数アプリへ同期（NewAPI などのゲートウェイ向け）
- Skills の改善：マルチアプリ対応、SSOT + React Query による管理の統一
- 共通設定スニペット：エディタ内容または現在のプロバイダから抽出
- MCP インポート：インストール済みアプリから MCP servers を取り込み
- 使用量の改善：自動更新、キャッシュ指標、タイムゾーン修正
- Linux パッケージ：RPM / Flatpak の成果物を追加

---

## 主要機能

### ローカル API プロキシ（Local API Proxy）

- ローカルで高性能な HTTP プロキシサーバーを起動（Axum ベース）
- Claude Code / Codex / Gemini CLI の API リクエストを統一的に扱う
- アプリ別テイクオーバー：アプリごとにプロキシ経由にするかを個別に切り替え可能
- Live 設定テイクオーバー：有効化時に CLI の live 設定をバックアップし、ローカルプロキシへリダイレクト
- 監視：リクエストログと使用量統計でデバッグとコスト把握を支援
- エラーリクエストのログ：失敗したプロキシリクエストも詳細に記録してデバッグを容易に（#401、@yovinchen に感謝）

### 自動フェイルオーバー（Auto Failover / サーキットブレーカー）

- 障害を検知して保護（サーキットブレーカー）を自動で発動
- 現在のプロバイダが不調な場合、バックアッププロバイダへ自動切り替え
- アプリごとに独立したフェイルオーバーキューとヘルス状態を管理
- フェイルオーバーを無効化している場合、タイムアウト/リトライ関連の設定は通常フローに影響しません

### Skills 管理

- Claude Code と Codex の Skills をマルチアプリで利用可能にし、旧レイアウトからの移行もよりスムーズに（#365、#378、@yovinchen に感謝）
- SSOT + React Query による Skills 管理の統一で、状態の一貫性と更新挙動を改善
- Discovery の体験と性能を改善：
  - スキャン時に隠しディレクトリをスキップ
  - Discoverable skills に長寿命キャッシュを適用して高速化
  - ローディング表示の改善と、インポート/更新などの操作導線を整理
  - Skills リポジトリのブランチ設定を修正（#505、@kjasn に感謝）

### Universal Provider

- 複数アプリで共有できるプロバイダ設定を追加（Claude/Codex/Gemini へ同期）（#348、@Calcium-Ion に感謝）
- NewAPI のような複数プロトコル対応の API ゲートウェイを想定
- 1つのプロバイダ内でアプリ別にデフォルトモデルを割り当て可能

### 共通設定スニペット（Claude/Codex/Gemini）

- 「共通設定スニペット」を保持し、有効化したプロバイダへマージ/追記
- 新しい抽出フロー：
  - エディタの現在内容から抽出（編集している内容）
  - エディタ内容がない場合は、現在アクティブなプロバイダから抽出
- Codex の抽出はより安全：
  - `model_provider`、`model`、および `model_providers` テーブル全体など、プロバイダ固有の設定を除去
  - `[mcp_servers.*]` 配下の `base_url` は保持し、MCP 設定を壊しにくくしています

### MCP 管理

- インストール済みアプリから MCP servers をインポート
- 安定性向上：対象 CLI が未インストールなら同期をスキップし、無効な Codex `config.toml` も適切に扱います（#461、@majiayu000 に感謝）
- Windows 互換性：MCP エクスポート時の npx/npm 呼び出しを `cmd /c` でラップ

### 使用量と価格データ

- 使用量/価格の改善：自動更新、キャッシュ指標、タイムゾーン修正、内蔵価格テーブル更新（#508、@yovinchen に感謝）
- DeepLink 対応：deeplink から使用量クエリ設定をインポート（#400、@qyinter に感謝）
- 使用量統計からモデル情報を抽出（#455、@yovinchen に感謝）
- 使用量クエリ資格情報はプロバイダ設定へフォールバック可能（#360、@Sirhexs に感謝）

---

## 使い勝手の改善

- プロバイダ検索フィルター（名前で素早く検索）（#435、@TinsFox に感謝）
- プロバイダのアイコン色：アイコンに任意の色を設定して見分けやすく（#385、@yovinchen に感謝）
- ショートカット：`Cmd/Ctrl + ,` で設定を開く（#436、@TinsFox に感謝）
- Claude Code の初回確認ダイアログをスキップ可能（任意）
- トースト通知のクローズボタン：切り替え通知と成功通知を閉じられるように（#350、@ForteScarlet に感謝）
- 更新バッジをクリックすると About タブへ移動
- 設定ページのタブスタイル改善（#342、@wenyuanw に感謝）
- アプリ/ビュー切り替えのフェードとパネル終了アニメーション
- プロキシテイクオーバー中はエメラルド系テーマを適用して状態を分かりやすく
- ダークモードの視認性改善
- FullScreenPanel のウィンドウドラッグ領域を改善（#525、@zerob13 に感謝）

---

## プラットフォーム別メモ

### Windows

- バージョンチェック時にターミナルが表示されないよう改善
- ウィンドウ最小サイズのデフォルトを調整
- 起動時の黒画面を避けるため、システムタイトルバー方式を採用
- 古い WebView 向けに `crypto.randomUUID()` のフォールバックを追加

### macOS

- 自動起動で `.app` バンドルパスを使用し、ターミナル表示を回避（#462、@majiayu000 に感謝）
- トレイとヘッダー周りの体験を改善

---

## パッケージ

- Linux：RPM と Flatpak のパッケージングを追加し、リリース成果物の生成に対応

---

## 注意事項

- セキュリティ強化：JavaScript 実行器と使用量スクリプト実行に関するセキュリティ問題を修正（#151、@luojiyin1987 に感謝）。
- SQL インポートは CC Switch がエクスポートしたバックアップのみに制限されます（安全性のため）。
- プロキシのテイクオーバーは CLI の live 設定を変更します。CC Switch はリダイレクト前に live 設定をバックアップします。元に戻す場合はテイクオーバー無効化/プロキシ停止を行い、必要に応じてバックアップから復元してください。

## 特別な謝辞

@xunyu @deijing @su-fen の皆様のサポートと貢献に特別な感謝を申し上げます。皆様なしではこのリリースは実現しませんでした！

## ダウンロード & インストール

[Releases](https://github.com/farion1231/cc-switch/releases/latest) から該当するバージョンをダウンロードしてください。

### システム要件

| システム | 最低バージョン                | アーキテクチャ                      |
| -------- | ----------------------------- | ----------------------------------- |
| Windows  | Windows 10 以降               | x64                                 |
| macOS    | macOS 10.15 (Catalina) 以降   | Intel (x64) / Apple Silicon (arm64) |
| Linux    | 下表参照                      | x64                                 |

### Windows

| ファイル                                | 説明                                         |
| --------------------------------------- | -------------------------------------------- |
| `CC-Switch-v3.9.0-Windows.msi`          | **推奨** - MSI インストーラー、自動更新対応  |
| `CC-Switch-v3.9.0-Windows-Portable.zip` | ポータブル版、インストール不要               |

### macOS

| ファイル                        | 説明                                                              |
| ------------------------------- | ----------------------------------------------------------------- |
| `CC-Switch-v3.9.0-macOS.zip`    | **推奨** - 解凍して Applications へドラッグ、Universal Binary     |
| `CC-Switch-v3.9.0-macOS.tar.gz` | Homebrew インストールおよび自動更新用                             |

> **注意**: 作者が Apple Developer アカウントを持っていないため、初回起動時に「開発元が未確認」という警告が表示される場合があります。アプリを閉じてから、「システム設定」→「プライバシーとセキュリティ」→「このまま開く」をクリックすると、正常に開けるようになります。

### Homebrew (MacOS)

```bash
brew tap farion1231/ccswitch
brew install --cask cc-switch
```

アップデート:

```bash
brew upgrade --cask cc-switch
```

### Linux

| ディストリビューション                  | 推奨形式    | インストール方法                                                               |
| --------------------------------------- | ----------- | ------------------------------------------------------------------------------ |
| Ubuntu / Debian / Linux Mint / Pop!\_OS | `.deb`      | `sudo dpkg -i CC-Switch-*.deb` または `sudo apt install ./CC-Switch-*.deb`     |
| Fedora / RHEL / CentOS / Rocky Linux    | `.rpm`      | `sudo rpm -i CC-Switch-*.rpm` または `sudo dnf install ./CC-Switch-*.rpm`      |
| openSUSE                                | `.rpm`      | `sudo zypper install ./CC-Switch-*.rpm`                                        |
| Arch Linux / Manjaro                    | `.AppImage` | 実行権限を付与して直接実行、または AUR を使用                                  |
| その他 / 不明                           | `.AppImage` | `chmod +x CC-Switch-*.AppImage && ./CC-Switch-*.AppImage`                      |
| サンドボックスで実行したい場合          | `.flatpak`  | `flatpak install CC-Switch-*.flatpak`                                          |
