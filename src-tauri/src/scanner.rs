use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// 個別ファイルの情報
#[derive(Debug, Clone, Serialize)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub hash: String,
    pub extension: String,
}

/// 重複グループ（同一内容を持つファイル群）
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateGroup {
    pub hash: String,
    pub size: u64,
    pub files: Vec<FileInfo>,
}

/// フォルダ内のファイルを再帰的に（または直下のみ）収集するヘルパー
fn collect_files(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    if dir.is_dir() {
        // fs::read_dir がエラーを返した場合はそのディレクトリはスキップ（権限エラー等への配慮）
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    files.push(path);
                } else if path.is_dir() && recursive {
                    if let Ok(mut sub_files) = collect_files(&path, true) {
                        files.append(&mut sub_files);
                    }
                }
            }
        }
    }
    Ok(files)
}

/// 指定フォルダ以下のファイルを走査し、重複グループを返す
/// アルゴリズム：
///   ステージ1: ファイル名でグループ化（同名ファイルの検出）
///   ステージ2: ファイルサイズでグループ化（同サイズのみが候補）
///   ステージ3: SHA-256ハッシュで最終判定 (strict モード時のみ)
pub fn scan_for_duplicates(folder_path: &str, mode: &str, recursive: bool) -> Result<Vec<DuplicateGroup>, String> {
    let path = Path::new(folder_path);
    if !path.exists() {
        return Err(format!("フォルダが存在しません: {}", folder_path));
    }
    if !path.is_dir() {
        return Err(format!("ディレクトリではありません: {}", folder_path));
    }

    // ファイルを収集
    let entries = collect_files(path, recursive)?;

    // ステージ2: ファイルサイズでグループ化
    let mut size_groups: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for file_path in &entries {
        if let Ok(metadata) = fs::metadata(file_path) {
            size_groups
                .entry(metadata.len())
                .or_default()
                .push(file_path.clone());
        }
    }

    // 同サイズのファイルが2つ以上あるグループのみ残す
    let candidates: Vec<(u64, Vec<PathBuf>)> = size_groups
        .into_iter()
        .filter(|(_, files)| files.len() >= 2)
        .collect();

    let mut duplicate_groups: Vec<DuplicateGroup> = Vec::new();

    if mode == "size_only" {
        // ステージ3をスキップし、サイズが同じものをそのままグループ化
        for (size, files) in candidates {
            let file_infos: Vec<FileInfo> = files
                .iter()
                .map(|fp| {
                    let name = fp.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let extension = fp.extension().unwrap_or_default().to_string_lossy().to_string();
                    FileInfo {
                        path: fp.to_string_lossy().to_string(),
                        name,
                        size,
                        hash: format!("size_{}", size), // ダミーハッシュ
                        extension,
                    }
                })
                .collect();

            duplicate_groups.push(DuplicateGroup {
                hash: format!("size_{}", size),
                size,
                files: file_infos,
            });
        }
    } else {
        // ステージ3: strictモードの場合は、SHA-256ハッシュで厳密に最終判定
        for (size, files) in candidates {
            let mut hash_groups: HashMap<String, Vec<PathBuf>> = HashMap::new();

            for file_path in &files {
                match calculate_hash(file_path) {
                    Ok(hash) => {
                        hash_groups.entry(hash).or_default().push(file_path.clone());
                    }
                    Err(_) => continue, // ハッシュ計算に失敗したファイルはスキップ
                }
            }

            // ハッシュが同一のファイルが2つ以上あるグループを重複として登録
            for (hash, matched_files) in hash_groups {
                if matched_files.len() >= 2 {
                    let file_infos: Vec<FileInfo> = matched_files
                        .iter()
                        .map(|fp| {
                            let name = fp.file_name().unwrap_or_default().to_string_lossy().to_string();
                            let extension = fp.extension().unwrap_or_default().to_string_lossy().to_string();
                            FileInfo {
                                path: fp.to_string_lossy().to_string(),
                                name,
                                size,
                                hash: hash.clone(),
                                extension,
                            }
                        })
                        .collect();

                    duplicate_groups.push(DuplicateGroup {
                        hash: hash.clone(),
                        size,
                        files: file_infos,
                    });
                }
            }
        }
    }

    // サイズの大きい順にソート
    duplicate_groups.sort_by(|a, b| b.size.cmp(&a.size));

    Ok(duplicate_groups)
}

/// ファイルのSHA-256ハッシュを計算
fn calculate_hash(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| format!("ファイルを開けません: {}", e))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = file
            .read(&mut buffer)
            .map_err(|e| format!("ファイル読み取りエラー: {}", e))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let result = hasher.finalize();
    Ok(format!("{:x}", result))
}

#[cfg(target_os = "windows")]
fn extract_thumbnail_windows(file_path: &str, max_size: i32) -> Result<String, String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDIBits, GetDC, ReleaseDC, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS, RGBQUAD,
    };
    use windows::Win32::System::Com::{
        CoInitializeEx, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    use windows::Win32::UI::Shell::{
        IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
        SIIGBF_RESIZETOFIT,
    };

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    }

    let wide_path: Vec<u16> = OsStr::new(file_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let hbitmap = unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide_path.as_ptr()), None)
            .map_err(|e| format!("SHCreateItemFromParsingName failed: {}", e))?;
        let factory: IShellItemImageFactory = item
            .cast()
            .map_err(|e| format!("Failed to cast to IShellItemImageFactory: {}", e))?;
        let size = SIZE {
            cx: max_size,
            cy: max_size,
        };
        let flags = SIIGBF_RESIZETOFIT | SIIGBF_BIGGERSIZEOK;
        factory
            .GetImage(size, flags)
            .map_err(|e| format!("GetImage failed: {}", e))?
    };

    let mut bm = BITMAP::default();
    unsafe {
        GetObjectW(
            hbitmap,
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut _),
        );
    }

    let width = bm.bmWidth;
    let height = bm.bmHeight;

    if width <= 0 || height <= 0 {
        unsafe {
            let _ = DeleteObject(hbitmap);
        }
        return Err("Invalid bitmap dimensions".to_string());
    }

    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: (width * height * 4) as u32,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [RGBQUAD::default()],
    };

    let image_data_size = (width * height * 4) as usize;
    let mut pixel_data = vec![0u8; image_data_size];

    unsafe {
        let hdc = GetDC(None);
        GetDIBits(
            hdc,
            hbitmap,
            0,
            height as u32,
            Some(pixel_data.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, hdc);
        let _ = DeleteObject(hbitmap);
    }

    // BMPファイル構造の作成
    let file_header_size = 14u32;
    let info_header_size = 40u32;
    let total_file_size = file_header_size + info_header_size + (image_data_size as u32);
    let bf_off_bits = file_header_size + info_header_size;

    let mut bmp_bytes = Vec::with_capacity(total_file_size as usize);

    // BITMAPFILEHEADER
    bmp_bytes.extend_from_slice(&0x4D42u16.to_le_bytes()); // 'BM'
    bmp_bytes.extend_from_slice(&total_file_size.to_le_bytes());
    bmp_bytes.extend_from_slice(&0u16.to_le_bytes()); // reserved1
    bmp_bytes.extend_from_slice(&0u16.to_le_bytes()); // reserved2
    bmp_bytes.extend_from_slice(&bf_off_bits.to_le_bytes());

    // BITMAPINFOHEADER
    bmp_bytes.extend_from_slice(&40u32.to_le_bytes()); // biSize
    bmp_bytes.extend_from_slice(&(width as i32).to_le_bytes()); // biWidth
    bmp_bytes.extend_from_slice(&(height as i32).to_le_bytes()); // biHeight
    bmp_bytes.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    bmp_bytes.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    bmp_bytes.extend_from_slice(&0u32.to_le_bytes()); // biCompression (BI_RGB)
    bmp_bytes.extend_from_slice(&(image_data_size as u32).to_le_bytes()); // biSizeImage
    bmp_bytes.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
    bmp_bytes.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
    bmp_bytes.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    bmp_bytes.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    // ピクセルデータ (BGRA)
    bmp_bytes.extend_from_slice(&pixel_data);

    let base64_str = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bmp_bytes);
    Ok(format!("data:image/bmp;base64,{}", base64_str))
}

#[cfg(not(target_os = "windows"))]
fn extract_thumbnail_windows(_file_path: &str, _max_size: i32) -> Result<String, String> {
    Err("Thumbnail extraction is only supported on Windows".to_string())
}

/// ファイルのプレビューデータを取得
/// 画像の場合はbase64エンコード、動画の場合はサムネイル画像、テキストの場合は先頭行を返す
pub fn get_preview(file_path: &str) -> Result<FilePreview, String> {
    let path = Path::new(file_path);
    if !path.exists() {
        return Err("ファイルが存在しません".to_string());
    }

    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();

    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico" | "svg" => {
            let data = fs::read(path).map_err(|e| format!("画像の読み取りに失敗: {}", e))?;
            let base64_data =
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);
            Ok(FilePreview {
                preview_type: "image".to_string(),
                content: format!("data:image/{};base64,{}", extension, base64_data),
                file_path: file_path.to_string(),
            })
        }
        "mp4" | "webm" | "ogg" | "mov" | "avi" | "mkv" | "wmv" | "flv" | "m4v" | "m2ts" | "mts" | "3gp" => {
            // Windows APIでサムネイルを取得
            match extract_thumbnail_windows(file_path, 400) {
                Ok(thumb_data_url) => Ok(FilePreview {
                    preview_type: "video".to_string(),
                    content: thumb_data_url,
                    file_path: file_path.to_string(),
                }),
                Err(_) => {
                    // サムネイル抽出に失敗した場合でもフォールバック用にパスを返す
                    Ok(FilePreview {
                        preview_type: "video".to_string(),
                        content: "".to_string(),
                        file_path: file_path.to_string(),
                    })
                }
            }
        }
        "txt" | "md" | "rs" | "js" | "ts" | "tsx" | "jsx" | "css" | "html" | "json" | "toml"
        | "yaml" | "yml" | "xml" | "csv" | "log" | "py" | "java" | "c" | "cpp" | "h" | "go"
        | "rb" | "php" | "sh" | "bat" | "ps1" => {
            let content =
                fs::read_to_string(path).map_err(|e| format!("テキストの読み取りに失敗: {}", e))?;
            // 先頭20行のみ返す
            let preview: String = content.lines().take(20).collect::<Vec<_>>().join("\n");
            Ok(FilePreview {
                preview_type: "text".to_string(),
                content: preview,
                file_path: file_path.to_string(),
            })
        }
        _ => Ok(FilePreview {
            preview_type: "unsupported".to_string(),
            content: format!("プレビュー非対応の形式: .{}", extension),
            file_path: file_path.to_string(),
        }),
    }
}

/// ファイルをゴミ箱に移動して削除
pub fn delete_files_to_trash(file_paths: &[String]) -> Result<DeleteResult, String> {
    let mut deleted = Vec::new();
    let mut failed = Vec::new();

    for path_str in file_paths {
        let path = Path::new(path_str);
        if !path.exists() {
            failed.push(DeleteError {
                path: path_str.clone(),
                error: "ファイルが存在しません".to_string(),
            });
            continue;
        }

        match trash::delete(path) {
            Ok(_) => deleted.push(path_str.clone()),
            Err(e) => {
                failed.push(DeleteError {
                    path: path_str.clone(),
                    error: format!("削除に失敗: {}", e),
                });
            }
        }
    }

    Ok(DeleteResult { deleted, failed })
}

#[derive(Debug, Clone, Serialize)]
pub struct FilePreview {
    pub preview_type: String,
    pub content: String,
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteResult {
    pub deleted: Vec<String>,
    pub failed: Vec<DeleteError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteError {
    pub path: String,
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::PathBuf;

    #[test]
    fn test_duplicate_detection() {
        // テスト用のディレクトリを作成
        let test_dir = "test_duplicates_dir";
        let _ = fs::remove_dir_all(test_dir);
        fs::create_dir(test_dir).unwrap();

        // 期待される重複ファイルペア（内容同一）
        let file1 = PathBuf::from(test_dir).join("file1.txt");
        let file2 = PathBuf::from(test_dir).join("file1_copy.txt");
        // 単独ファイル
        let file3 = PathBuf::from(test_dir).join("file3.txt");
        // サイズは同じだが内容が異なる
        let file4 = PathBuf::from(test_dir).join("file4.txt");
        let file5 = PathBuf::from(test_dir).join("file5.txt");

        let mut f1 = File::create(&file1).unwrap();
        f1.write_all(b"Hello World identical").unwrap();

        let mut f2 = File::create(&file2).unwrap();
        f2.write_all(b"Hello World identical").unwrap();

        let mut f3 = File::create(&file3).unwrap();
        f3.write_all(b"Hello World unique here").unwrap();

        let mut f4 = File::create(&file4).unwrap();
        f4.write_all(b"Size identical, but...A").unwrap(); // 23 bytes

        let mut f5 = File::create(&file5).unwrap();
        f5.write_all(b"Size identical, but...B").unwrap(); // 23 bytes

        // スキャン実行 (strict mode, recursive false)
        let groups = scan_for_duplicates(test_dir, "strict", false).unwrap();

        // クリーンアップ
        let _ = fs::remove_dir_all(test_dir);

        // 検証
        // 重複グループは1つのみ見つかるはず（file1 と file2）
        assert_eq!(groups.len(), 1, "Duplicate groups count should be 1");

        let dup_group = &groups[0];
        assert_eq!(
            dup_group.files.len(),
            2,
            "There should be 2 files in the group"
        );

        let paths: Vec<String> = dup_group.files.iter().map(|f| f.path.clone()).collect();
        assert!(paths.contains(&file1.to_string_lossy().to_string()));
        assert!(paths.contains(&file2.to_string_lossy().to_string()));
        assert!(!paths.contains(&file4.to_string_lossy().to_string()));
    }

    #[test]
    fn test_get_preview_types() {
        let test_dir = "test_preview_dir";
        let _ = fs::remove_dir_all(test_dir);
        fs::create_dir(test_dir).unwrap();

        let txt_file = PathBuf::from(test_dir).join("test.txt");
        let mut f = File::create(&txt_file).unwrap();
        f.write_all(b"line 1\nline 2\nline 3").unwrap();

        let preview = get_preview(&txt_file.to_string_lossy()).unwrap();
        assert_eq!(preview.preview_type, "text");
        assert!(preview.content.contains("line 1"));

        // 動画形式のプレビュー（存在しない/ダミーファイルでもクラッシュせずvideo型を返す）
        let video_file = PathBuf::from(test_dir).join("test.mp4");
        let mut f_v = File::create(&video_file).unwrap();
        f_v.write_all(b"dummy mp4 content").unwrap();

        let video_preview = get_preview(&video_file.to_string_lossy()).unwrap();
        assert_eq!(video_preview.preview_type, "video");
        assert_eq!(video_preview.file_path, video_file.to_string_lossy());

        let _ = fs::remove_dir_all(test_dir);
    }
}
