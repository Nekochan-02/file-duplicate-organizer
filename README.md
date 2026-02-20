# File Duplicate Organizer 📂

A high-performance file duplicated files finder and organizer.
Built with Tauri + React + Rust.

![File Duplicate Organizer Icon](src-tauri/icons/128x128.png)

## Overview
This application scans a selected folder, detects duplicate files accurately using a 3-stage algorithm, and allows you to safely delete them. Designed to work blazingly fast even with a large number of files.

- **3-Stage Precise Detection**: Uses filename → filesize → SHA-256 hashing to guarantee accuracy and boost performance.
- **Built-in Preview**: Image thumbnails and text-data previews help you confidently sort before deleting.
- **Safe Deletion**: Files are sent to your OS Recycle Bin/Trash, preventing accidental permanent loss.
- **Auto Updater**: Automatically downloads and installs the latest version without you lifting a finger.

## Installation

Download the latest version from the [Releases](https://github.com/nekochan/file-duplicate-organizer/releases) page.
- **Windows**: Download `File Duplicate Organizer_*_x64-setup.exe` and follow the installer.
  > Note: If Windows SmartScreen shows a warning, please select "More info" and click "Run anyway".
- **macOS**: Support coming soon (can be built from source).

## Build from Source

You'll need Node.js and Rust installed on your machine.

```bash
# Clone the repository
git clone https://github.com/nekochan/file-duplicate-organizer.git
cd file-duplicate-organizer

# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build a release binary target
npm run tauri build
```

## License
This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
