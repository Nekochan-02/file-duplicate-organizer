import { useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { check } from "@tauri-apps/plugin-updater";
import { ask, message } from "@tauri-apps/plugin-dialog";
import { getVersion } from "@tauri-apps/api/app";
import "./App.css";

// Rust側の型定義
interface FileInfo {
  path: string;
  name: string;
  size: number;
  hash: string;
  extension: string;
}

interface DuplicateGroup {
  hash: string;
  size: number;
  files: FileInfo[];
}

interface FilePreview {
  preview_type: string;
  content: string;
  file_path: string;
}

interface DeleteResult {
  deleted: string[];
  failed: { path: string; error: string }[];
}

function formatSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(i > 0 ? 1 : 0)} ${units[i]}`;
}

function App() {
  const [folderPath, setFolderPath] = useState<string>("");
  const [groups, setGroups] = useState<DuplicateGroup[]>([]);
  const [selectedFiles, setSelectedFiles] = useState<Set<string>>(new Set());
  const [preview, setPreview] = useState<FilePreview | null>(null);
  const [isScanning, setIsScanning] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [scanComplete, setScanComplete] = useState(false);
  const [appVersion, setAppVersion] = useState("");
  const [scanMode, setScanMode] = useState<"strict" | "size_only">("strict");

  // 初期化時にバージョン取得
  useEffect(() => {
    getVersion().then(setAppVersion);
  }, []);

  // アップデート確認
  const checkForUpdates = async () => {
    try {
      const update = await check();
      if (update) {
        const yes = await ask(
          `新しいバージョン (${update.version}) が利用可能です。\nリリースノート:\n${update.body}\n\n今すぐダウンロードしてインストールしますか？`,
          { title: 'アップデートの確認', kind: 'info' }
        );
        if (yes) {
          showToast("アップデートをダウンロード中...");
          await update.downloadAndInstall((event) => {
            switch (event.event) {
              case 'Started':
                showToast(`ダウンロード開始...`);
                break;
              case 'Progress':
                // ignore progress for now to avoid toast spam
                break;
              case 'Finished':
                showToast('ダウンロード完了。再起動します。');
                break;
            }
          });
          // インストール後はアプリ再起動等の仕組みが必要な場合があります
        }
      } else {
        await message('現在最新バージョンを使用しています。', { title: 'アップデートの確認', kind: 'info' });
      }
    } catch (e) {
      showToast(`アップデート確認エラー: ${e}`);
    }
  };

  // トースト表示
  const showToast = useCallback((msg: string) => {
    setToast(msg);
    setTimeout(() => setToast(null), 3000);
  }, []);

  // フォルダ選択
  const pickFolder = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (selected) {
      setFolderPath(selected as string);
      setScanComplete(false);
      setGroups([]);
      setSelectedFiles(new Set());
      setPreview(null);
    }
  };

  // スキャン実行
  const startScan = async () => {
    if (!folderPath) return;
    setIsScanning(true);
    setScanComplete(false);
    setGroups([]);
    setSelectedFiles(new Set());
    setPreview(null);
    try {
      const result = await invoke<DuplicateGroup[]>("scan_folder", {
        path: folderPath,
        mode: scanMode
      });
      setGroups(result);
      setScanComplete(true);
      if (result.length === 0) {
        showToast("重複ファイルは見つかりませんでした");
      }
    } catch (e) {
      showToast(`エラー: ${e}`);
    } finally {
      setIsScanning(false);
    }
  };

  // ファイル選択の切り替え
  const toggleFile = (path: string) => {
    setSelectedFiles((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  };

  // 全選択 / 全解除 / 一つを残して選択
  const selectAll = () => {
    const allPaths = new Set<string>();
    groups.forEach((g) => g.files.forEach((f) => allPaths.add(f.path)));
    setSelectedFiles(allPaths);
  };

  const selectAllButOne = () => {
    const pathsToSelect = new Set<string>();
    groups.forEach((group) => {
      if (group.files.length > 1) {
        // Find the file with the maximum size
        let maxSizeFileIndex = 0;
        let maxSize = group.files[0].size;
        for (let i = 1; i < group.files.length; i++) {
          if (group.files[i].size > maxSize) {
            maxSize = group.files[i].size;
            maxSizeFileIndex = i;
          }
        }

        // Add all files except the one with the maximum size
        group.files.forEach((file, index) => {
          if (index !== maxSizeFileIndex) {
            pathsToSelect.add(file.path);
          }
        });
      }
    });
    setSelectedFiles(pathsToSelect);
  };

  const deselectAll = () => {
    setSelectedFiles(new Set());
  };

  // プレビュー取得
  const loadPreview = async (path: string) => {
    try {
      const result = await invoke<FilePreview>("get_file_preview", { path });
      setPreview(result);
    } catch {
      setPreview({
        preview_type: "unsupported",
        content: "プレビューの読み込みに失敗しました",
        file_path: path,
      });
    }
  };

  // 削除実行
  const executeDelete = async () => {
    setShowConfirm(false);
    const paths = Array.from(selectedFiles);
    try {
      const result = await invoke<DeleteResult>("delete_files", { paths });
      const deletedCount = result.deleted.length;
      const failedCount = result.failed.length;

      // 削除済みファイルをリストから除去
      const deletedSet = new Set(result.deleted);
      const updatedGroups = groups
        .map((g) => ({
          ...g,
          files: g.files.filter((f) => !deletedSet.has(f.path)),
        }))
        .filter((g) => g.files.length >= 2);
      setGroups(updatedGroups);
      setSelectedFiles(new Set());
      setPreview(null);

      if (failedCount > 0) {
        showToast(`${deletedCount}件削除、${failedCount}件失敗`);
      } else {
        showToast(`${deletedCount}件のファイルをゴミ箱に移動しました`);
      }
    } catch (e) {
      showToast(`削除エラー: ${e}`);
    }
  };

  const totalDuplicateFiles = groups.reduce((sum, g) => sum + g.files.length, 0);

  return (
    <div className="app">
      {/* Header */}
      <header className="header">
        <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
          <h1>📂 File Duplicate Organizer</h1>
          <span style={{ fontSize: "11px", color: "var(--text-muted)" }}>v{appVersion}</span>
        </div>
        <div className="header-actions">
          <button className="btn btn-ghost" onClick={checkForUpdates} style={{ fontSize: "11px", padding: "4px 8px" }}>
            🔄 更新を確認
          </button>
          {scanComplete && groups.length > 0 && (
            <span className="status-badge warning">
              {groups.length}グループ・{totalDuplicateFiles}ファイル
            </span>
          )}
          {scanComplete && groups.length === 0 && (
            <span className="status-badge success">✓ 重複なし</span>
          )}
        </div>
      </header>

      {/* Folder Picker & Options */}
      <div className="folder-picker">
        <div className="picker-container" style={{ display: "flex", gap: "10px", width: "100%", alignItems: "center" }}>
          <button className="btn btn-ghost" onClick={pickFolder} style={{ flexShrink: 0 }}>
            📁 フォルダ選択
          </button>
          <div className="folder-path" style={{ flexGrow: 1, textOverflow: "ellipsis", overflow: "hidden", whiteSpace: "nowrap" }}>
            {folderPath || "スキャンするフォルダを選択してください..."}
          </div>

          <select
            className="select-input"
            value={scanMode}
            onChange={(e) => setScanMode(e.target.value as "strict" | "size_only")}
            title="検出モード"
            style={{ padding: "8px", borderRadius: "4px", border: "1px solid var(--border-color)", background: "var(--bg-secondary)", color: "var(--text-primary)" }}
          >
            <option value="strict">完全一致 (推奨)</option>
            <option value="size_only">サイズのみ比較 (高速)</option>
          </select>

          <button
            className="btn btn-primary"
            onClick={startScan}
            disabled={!folderPath || isScanning}
            style={{ flexShrink: 0 }}
          >
            {isScanning ? "⏳ スキャン中..." : "🔍 スキャン"}
          </button>
        </div>
      </div>

      {/* Main Content */}
      <div className="main-content">
        {/* Duplicate List */}
        <div className="duplicate-list">
          {isScanning && (
            <div className="loading">
              <div className="spinner" />
              <div className="loading-text">ファイルをスキャンしています...</div>
            </div>
          )}

          {!isScanning && !scanComplete && groups.length === 0 && (
            <div className="empty-state">
              <div className="empty-icon">🔍</div>
              <div className="empty-text">
                フォルダを選択して「スキャン」を押すと
                <br />
                重複ファイルを検出します
              </div>
            </div>
          )}

          {!isScanning && scanComplete && groups.length === 0 && (
            <div className="empty-state">
              <div className="empty-icon">✅</div>
              <div className="empty-text">
                重複ファイルは見つかりませんでした
              </div>
            </div>
          )}

          {groups.map((group, gi) => (
            <div key={group.hash + gi} className="duplicate-group">
              <div className="group-header">
                <div className="group-info">
                  <span className="group-badge">{group.files.length}件</span>
                  <span className="group-size">
                    各 {formatSize(group.size)}
                  </span>
                </div>
                <span className="group-size" title={group.hash}>
                  SHA-256: {group.hash.substring(0, 12)}...
                </span>
              </div>
              {group.files.map((file) => (
                <div
                  key={file.path}
                  className={`file-item ${selectedFiles.has(file.path) ? "selected" : ""}`}
                  onClick={() => loadPreview(file.path)}
                >
                  <input
                    type="checkbox"
                    className="file-checkbox"
                    checked={selectedFiles.has(file.path)}
                    onChange={(e) => {
                      e.stopPropagation();
                      toggleFile(file.path);
                    }}
                    onClick={(e) => e.stopPropagation()}
                  />
                  <div className="file-info">
                    <div className="file-name">{file.name}</div>
                    <div className="file-path">{file.path}</div>
                  </div>
                  <span className="file-size-tag">{formatSize(file.size)}</span>
                </div>
              ))}
            </div>
          ))}
        </div>

        {/* Preview Panel */}
        <div className="preview-panel">
          <div className="preview-header">プレビュー</div>
          <div className="preview-content">
            {!preview && (
              <div className="preview-placeholder">
                ファイルをクリックすると
                <br />
                プレビューが表示されます
              </div>
            )}
            {preview?.preview_type === "image" && (
              <img
                className="preview-image"
                src={preview.content}
                alt="Preview"
              />
            )}
            {preview?.preview_type === "text" && (
              <pre className="preview-text">{preview.content}</pre>
            )}
            {preview?.preview_type === "unsupported" && (
              <div className="preview-placeholder">{preview.content}</div>
            )}
          </div>
        </div>
      </div>

      {/* Action Bar */}
      {groups.length > 0 && (
        <div className="action-bar">
          <div className="action-info">
            <strong>{selectedFiles.size}</strong> / {totalDuplicateFiles} 件選択中
          </div>
          <div className="action-buttons">
            <button className="btn btn-ghost" onClick={selectAll}>
              全選択
            </button>
            <button className="btn btn-ghost" onClick={selectAllButOne}>
              サイズ最大を残して選択
            </button>
            <button className="btn btn-ghost" onClick={deselectAll}>
              選択解除
            </button>
            <button
              className="btn btn-danger"
              disabled={selectedFiles.size === 0}
              onClick={() => setShowConfirm(true)}
            >
              🗑️ 選択ファイルを削除
            </button>
          </div>
        </div>
      )}

      {/* Confirm Dialog */}
      {showConfirm && (
        <div className="dialog-overlay" onClick={() => setShowConfirm(false)}>
          <div className="dialog" onClick={(e) => e.stopPropagation()}>
            <h3>ファイルの削除確認</h3>
            <p>
              選択された <strong>{selectedFiles.size}件</strong>{" "}
              のファイルをゴミ箱に移動します。
              <br />
              この操作はゴミ箱から復元できます。続行しますか？
            </p>
            <div className="dialog-actions">
              <button
                className="btn btn-ghost"
                onClick={() => setShowConfirm(false)}
              >
                キャンセル
              </button>
              <button className="btn btn-danger" onClick={executeDelete}>
                🗑️ 削除する
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Toast */}
      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}

export default App;
