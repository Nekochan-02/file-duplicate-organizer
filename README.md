# File Duplicate Organizer 📂

A high-performance finder and organizer for duplicate files. / 高速動作の重複ファイル検索・整理アプリ。  
Built with Tauri + React + Rust.

![File Duplicate Organizer Icon](src-tauri/icons/128x128.png)

## Overview / 概要

This application scans a selected folder, detects duplicate files accurately using a 3-stage algorithm, and allows you to safely delete them. Designed to work blazingly fast even with a large number of files.

指定したフォルダをスキャンし、3段階のアルゴリズムを用いて重複ファイルを正確に検出・安全に削除できるGUIアプリケーションです。大量のファイルがあっても高速に動作するよう設計されています。

- **3-Stage Precise Detection / 3段階の精密検出**: Uses filename → filesize → SHA-256 hashing to guarantee accuracy and boost performance. (ファイル名 → サイズ → SHA-256ハッシュ の順に判定し、高速かつ確実な検出を実現)
- **Built-in Preview / プレビュー内蔵**: Image thumbnails and text-data previews help you confidently sort before deleting. (画像サムネイルやテキストの先頭行プレビューを備え、内容を確認してから削除可能)
- **Safe Deletion / 安全な削除（ゴミ箱へ）**: Files are sent to your OS Recycle Bin/Trash, preventing accidental permanent loss. (削除したファイルはOSの「ゴミ箱」に移動されるため、誤って消しても復元可能)
- **Auto Updater / 自動アップデート**: Automatically downloads and installs the latest version. (最新バージョンが存在する場合、自動的に案内・インストールを実行)

## Installation / インストール方法

Download the latest version from the [Releases](https://github.com/nekochan/file-duplicate-organizer/releases) page.  
[Releases](https://github.com/nekochan/file-duplicate-organizer/releases) ページから、最新バージョンのインストーラをダウンロードしてください。

- **Windows**: Download `File Duplicate Organizer_*_x64-setup.exe` and follow the installer.
  > Note: If Windows SmartScreen shows a warning, please select "More info" and click "Run anyway".  
  > （※注意：WindowsのSmartScreen警告画面が表示された場合は、「詳細情報」をクリックしてから「実行」を選択してインストールを進めてください。）
- **macOS**: Support coming soon (can be built from source). 

## Build from Source / ソースからビルドする

You'll need Node.js and Rust installed on your machine.  
ローカルでビルドする場合は、事前に Node.js と Rust（Cargo）のインストールが必要です。

```bash
# Clone the repository
git clone https://github.com/nekochan/file-duplicate-organizer.git
cd file-duplicate-organizer

# Install dependencies (依存パッケージのインストール)
npm install

# Run in development mode (開発モードでデバッグ起動)
npm run tauri dev

# Build a release binary target (リリース用バイナリのビルド)
npm run tauri build
```

## License / ライセンス
This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.  
当プロジェクトは MIT License の下で公開されています。詳細は [LICENSE](LICENSE) ファイルをご確認ください。
