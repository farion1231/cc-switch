<div align="center">

# CC Switch Legacy

### macOS 10.15 Catalina 対応のための CC Switch フォーク

[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](#compatibility)
[![macOS](https://img.shields.io/badge/macOS-10.15%2B-blue.svg)](#compatibility)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)

[English](README.md) | [中文](README_ZH.md) | 日本語 | [互換性メモ](docs/macos-10.15-compat.md) | [Upstream Project](https://github.com/farion1231/cc-switch) | [Changelog](CHANGELOG.md)

</div>

## Overview

このリポジトリは [CC Switch](https://github.com/farion1231/cc-switch) をベースにした互換性重視のフォークです。

目的は明確です。元の CC Switch の使い勝手をできるだけ維持したまま、デスクトップアプリを **macOS 10.15 Catalina** でもビルド・実行できるようにすることです。上流プロジェクトは現在 macOS 10.15 を主なサポート対象にしていないため、この legacy フォークを分けて管理しています。

macOS 12 以降を使っていて Catalina 対応が不要な場合は、基本的に上流版を使うほうが適切です。

## What Changed In This Fork

上流版 CC Switch と比べて、このフォークでは主に以下の互換性対応を入れています。

- macOS の deployment target を `12.0` から `10.15` へ変更
- Tauri の bundle 設定を調整し、`macOS 10.15+` で動作できるように変更
- 旧 WebKit に存在しない protocol method に対する `objc2` の dev モード対策を追加
- より新しい macOS 専用シンボルを避けるため `esbuild` を `0.21.5` に固定
- 旧 Safari / WKWebView 向けにフロントエンド build target を引き下げ
- Safari 13 の `BigInt` 構文問題を避けるため `smol-toml` を `@iarna/toml` に置換
- テーマ監視用に `MediaQueryList.addListener` フォールバックを追加

詳しい内容は [docs/macos-10.15-compat.md](docs/macos-10.15-compat.md) を参照してください。

## Features

このフォークでも CC Switch の主要機能は維持されています。

- **Claude Code**、**Codex**、**Gemini CLI**、**OpenCode**、**OpenClaw** を 1 つのアプリで管理
- JSON、TOML、`.env` を手動編集せずに provider を追加・切り替え
- **MCP**、**Prompts**、**Skills** を統合管理
- システムトレイから素早く provider を切り替え
- 使用量やコストの可視化
- カスタム設定ディレクトリや WebDAV による同期
- 各 CLI ツールのセッション履歴の閲覧と復元

## Screenshots

| Main Interface | Add Provider |
| :---: | :---: |
| ![Main Interface](assets/screenshots/main-en.png) | ![Add Provider](assets/screenshots/add-en.png) |

## Compatibility

### Primary Target

- macOS 10.15 Catalina

### Also Expected To Work

- macOS 11+
- Windows
- Linux

このリポジトリは Catalina 互換性の維持を主目的としており、他プラットフォームでは可能な限り上流版に近い挙動を維持します。

## Quick Start

### Basic Usage

1. メイン画面で provider を追加
2. 使用したい provider を有効化
3. 必要に応じて対象 CLI またはターミナルを再起動
4. 素早く切り替えたい場合はトレイメニューを利用

### Documentation

- [macOS 10.15 互換性メモ](docs/macos-10.15-compat.md)
- [User Manual (English)](docs/user-manual/en/README.md)
- [ユーザーマニュアル 日本語](docs/user-manual/ja/README.md)
- [用户手册 中文](docs/user-manual/zh/README.md)

## Build From Source

### Requirements

- Node.js 18+
- pnpm 8+
- Rust 1.85+
- Tauri CLI 2.8+

### Commands

```bash
pnpm install
pnpm dev
pnpm typecheck
pnpm test:unit
pnpm build
```

### Rust Backend

```bash
cd src-tauri
cargo fmt
cargo clippy
cargo test
```

## macOS 10.15 Notes

このフォークには Catalina 向けの主要設定がすでに含まれています。

- `.cargo/config.toml` で `MACOSX_DEPLOYMENT_TARGET=10.15` を設定
- `src-tauri/tauri.conf.json` で `minimumSystemVersion` を `10.15` に設定
- `vite.config.ts` で旧 Safari 互換の build target を設定
- `package.json` で Catalina 互換の `esbuild` バージョンを固定

Catalina 固有の問題を調べる場合は、まず [docs/macos-10.15-compat.md](docs/macos-10.15-compat.md) を確認してください。

## Project Structure

```text
src/              Frontend (React + TypeScript)
src-tauri/        Backend (Tauri + Rust)
assets/           Screenshots and static assets
docs/             Compatibility notes, manuals, and release notes
tests/            Frontend tests
```

## Acknowledgements

- Original project: [farion1231/cc-switch](https://github.com/farion1231/cc-switch)
- This repository is a compatibility fork, not the official upstream release channel

## License

This project continues to use the [MIT License](LICENSE).
