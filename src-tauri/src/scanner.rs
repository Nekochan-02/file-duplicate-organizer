use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_SIZE_TOLERANCE_BYTES: u64 = 1024 * 1024;
const THUMBNAIL_REQUEST_SIZE: i32 = 256;
const PHASH_INPUT_SIZE: usize = 32;
const PHASH_LOW_FREQUENCY_SIZE: usize = 8;
const PHASH_MAX_HAMMING_DISTANCE: u32 = 8;

/// サムネイル類似判定の状態
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SimilarityStatus {
    NotRequested,
    NotSimilar,
    Similar,
    Unavailable,
}

/// 個別ファイルの情報
#[derive(Debug, Clone, Serialize)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub hash: Option<String>,
    pub extension: String,
    pub similar_set_id: Option<u32>,
    pub similarity_status: SimilarityStatus,
}

/// 重複または近似サイズ候補のグループ
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateGroup {
    pub hash: Option<String>,
    pub size: u64,
    pub min_size: u64,
    pub max_size: u64,
    pub max_size_delta: u64,
    pub comparison_type: String,
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

/// サイズ順に並んだファイルを、グループ内の最大・最小サイズ差が許容差以内になるようにまとめる。
/// グループ同士は重ならないため、サイズ差の連鎖による過剰なグループ化を避けられる。
fn group_files_by_size_tolerance(
    file_sizes: &[(u64, PathBuf)],
    tolerance_bytes: u64,
) -> Vec<(u64, u64, Vec<(u64, PathBuf)>)> {
    let mut groups = Vec::new();
    let mut start = 0;

    while start < file_sizes.len() {
        let min_size = file_sizes[start].0;
        let mut end = start + 1;

        while end < file_sizes.len()
            && file_sizes[end].0.saturating_sub(min_size) <= tolerance_bytes
        {
            end += 1;
        }

        if end - start >= 2 {
            let max_size = file_sizes[end - 1].0;
            groups.push((min_size, max_size, file_sizes[start..end].to_vec()));
        }

        start = end;
    }

    groups
}

/// 指定フォルダ以下のファイルを走査し、重複グループを返す。
/// strict は同一サイズ候補を SHA-256 で厳密比較し、size_only は
/// 指定されたサイズ許容差以内のファイルを近似サイズ候補として返す。
pub fn scan_for_duplicates(
    folder_path: &str,
    mode: &str,
    recursive: bool,
    size_tolerance_bytes: u64,
    highlight_similar_thumbnails: bool,
) -> Result<Vec<DuplicateGroup>, String> {
    let path = Path::new(folder_path);
    if !path.exists() {
        return Err(format!("フォルダが存在しません: {}", folder_path));
    }
    if !path.is_dir() {
        return Err(format!("ディレクトリではありません: {}", folder_path));
    }
    if mode != "strict" && mode != "size_only" {
        return Err(format!("不正なスキャンモードです: {}", mode));
    }
    if mode == "size_only" && size_tolerance_bytes > MAX_SIZE_TOLERANCE_BYTES {
        return Err(format!(
            "サイズ許容差は {} bytes 以下にしてください",
            MAX_SIZE_TOLERANCE_BYTES
        ));
    }

    // ファイルを収集
    let entries = collect_files(path, recursive)?;

    // ファイルサイズとパスを保持し、結果を安定させるためサイズ・パス順に並べる
    let mut file_sizes: Vec<(u64, PathBuf)> = entries
        .iter()
        .filter_map(|file_path| {
            fs::metadata(file_path)
                .ok()
                .map(|metadata| (metadata.len(), file_path.clone()))
        })
        .collect();
    file_sizes.sort_by(|(size_a, path_a), (size_b, path_b)| {
        size_a.cmp(size_b).then_with(|| path_a.cmp(path_b))
    });

    let mut duplicate_groups: Vec<DuplicateGroup> = Vec::new();

    if mode == "size_only" {
        // SHA-256を計算せず、実サイズの差が許容範囲内の候補を返す。
        for (min_size, max_size, files) in
            group_files_by_size_tolerance(&file_sizes, size_tolerance_bytes)
        {
            let file_infos: Vec<FileInfo> = files
                .iter()
                .map(|(file_size, file_path)| {
                    let name = file_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let extension = file_path
                        .extension()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    FileInfo {
                        path: file_path.to_string_lossy().to_string(),
                        name,
                        size: *file_size,
                        hash: None,
                        extension,
                        similar_set_id: None,
                        similarity_status: SimilarityStatus::NotRequested,
                    }
                })
                .collect();

            duplicate_groups.push(DuplicateGroup {
                hash: None,
                size: min_size,
                min_size,
                max_size,
                max_size_delta: max_size - min_size,
                comparison_type: if size_tolerance_bytes == 0 {
                    "size".to_string()
                } else {
                    "size_tolerance".to_string()
                },
                files: file_infos,
            });
        }
    } else {
        // strictモード: 同一サイズ候補をSHA-256ハッシュで厳密に最終判定
        let mut size_groups: HashMap<u64, Vec<PathBuf>> = HashMap::new();
        for (size, file_path) in &file_sizes {
            size_groups
                .entry(*size)
                .or_default()
                .push(file_path.clone());
        }

        // 同サイズのファイルが2つ以上あるグループのみ残す
        let candidates: Vec<(u64, Vec<PathBuf>)> = size_groups
            .into_iter()
            .filter(|(_, files)| files.len() >= 2)
            .collect();

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
                            let name = fp
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            let extension = fp
                                .extension()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            FileInfo {
                                path: fp.to_string_lossy().to_string(),
                                name,
                                size,
                                hash: Some(hash.clone()),
                                extension,
                                similar_set_id: None,
                                similarity_status: SimilarityStatus::NotRequested,
                            }
                        })
                        .collect();

                    duplicate_groups.push(DuplicateGroup {
                        hash: Some(hash.clone()),
                        size,
                        min_size: size,
                        max_size: size,
                        max_size_delta: 0,
                        comparison_type: "sha256".to_string(),
                        files: file_infos,
                    });
                }
            }
        }
    }

    if mode == "size_only" && highlight_similar_thumbnails {
        for group in &mut duplicate_groups {
            analyze_similarity_group(group);
        }
    }

    // サイズの大きい順にソート
    duplicate_groups.sort_by(|a, b| {
        b.max_size
            .cmp(&a.max_size)
            .then_with(|| a.min_size.cmp(&b.min_size))
    });

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

#[derive(Debug, Clone, PartialEq)]
struct ThumbnailBitmap {
    width: usize,
    height: usize,
    /// Top-down BGRA pixels, four bytes per pixel.
    pixels: Vec<u8>,
}

impl ThumbnailBitmap {
    fn to_bmp_data_url(&self) -> Result<String, String> {
        if self.width == 0 || self.height == 0 {
            return Err("Invalid bitmap dimensions".to_string());
        }

        let row_size = self
            .width
            .checked_mul(4)
            .ok_or_else(|| "Bitmap row is too large".to_string())?;
        let expected_size = row_size
            .checked_mul(self.height)
            .ok_or_else(|| "Bitmap is too large".to_string())?;
        if self.pixels.len() != expected_size {
            return Err("Invalid bitmap pixel data".to_string());
        }

        let file_header_size = 14u32;
        let info_header_size = 40u32;
        let image_data_size = expected_size as u32;
        let total_file_size = file_header_size + info_header_size + image_data_size;
        let bf_off_bits = file_header_size + info_header_size;

        let mut bmp_bytes = Vec::with_capacity(total_file_size as usize);

        // BITMAPFILEHEADER
        bmp_bytes.extend_from_slice(&0x4D42u16.to_le_bytes());
        bmp_bytes.extend_from_slice(&total_file_size.to_le_bytes());
        bmp_bytes.extend_from_slice(&0u16.to_le_bytes());
        bmp_bytes.extend_from_slice(&0u16.to_le_bytes());
        bmp_bytes.extend_from_slice(&bf_off_bits.to_le_bytes());

        // BITMAPINFOHEADER
        bmp_bytes.extend_from_slice(&40u32.to_le_bytes());
        bmp_bytes.extend_from_slice(&(self.width as i32).to_le_bytes());
        bmp_bytes.extend_from_slice(&(self.height as i32).to_le_bytes());
        bmp_bytes.extend_from_slice(&1u16.to_le_bytes());
        bmp_bytes.extend_from_slice(&32u16.to_le_bytes());
        bmp_bytes.extend_from_slice(&0u32.to_le_bytes());
        bmp_bytes.extend_from_slice(&image_data_size.to_le_bytes());
        bmp_bytes.extend_from_slice(&0i32.to_le_bytes());
        bmp_bytes.extend_from_slice(&0i32.to_le_bytes());
        bmp_bytes.extend_from_slice(&0u32.to_le_bytes());
        bmp_bytes.extend_from_slice(&0u32.to_le_bytes());

        // A positive BMP height uses bottom-up row order.
        for row in self.pixels.chunks_exact(row_size).rev() {
            bmp_bytes.extend_from_slice(row);
        }

        let base64_str =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bmp_bytes);
        Ok(format!("data:image/bmp;base64,{}", base64_str))
    }
}

#[cfg(target_os = "windows")]
fn extract_thumbnail_bitmap_windows(
    file_path: &str,
    max_size: i32,
) -> Result<ThumbnailBitmap, String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, RGBQUAD,
    };
    use windows::Win32::System::Com::{
        CoInitializeEx, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    use windows::Win32::UI::Shell::{
        IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
        SIIGBF_RESIZETOFIT, SIIGBF_THUMBNAILONLY,
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
        // Do not allow Windows to return a file-type icon as a thumbnail.
        let flags = SIIGBF_RESIZETOFIT | SIIGBF_BIGGERSIZEOK | SIIGBF_THUMBNAILONLY;
        factory
            .GetImage(size, flags)
            .map_err(|e| format!("GetImage failed: {}", e))?
    };

    let mut bm = BITMAP::default();
    let object_size = unsafe {
        GetObjectW(
            hbitmap,
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut _),
        )
    };
    if object_size == 0 {
        unsafe {
            let _ = DeleteObject(hbitmap);
        }
        return Err("GetObjectW failed".to_string());
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

    let scan_lines = unsafe {
        let hdc = GetDC(None);
        let scan_lines = GetDIBits(
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
        scan_lines
    };
    if scan_lines == 0 {
        return Err("GetDIBits failed".to_string());
    }

    // GetDIBits with a positive height returns bottom-up rows. Normalize the
    // shared representation to top-down so rotation and hashing are stable.
    let row_size = width as usize * 4;
    for top in 0..(height as usize / 2) {
        let bottom = height as usize - 1 - top;
        let top_start = top * row_size;
        let bottom_start = bottom * row_size;
        for offset in 0..row_size {
            pixel_data.swap(top_start + offset, bottom_start + offset);
        }
    }

    Ok(ThumbnailBitmap {
        width: width as usize,
        height: height as usize,
        pixels: pixel_data,
    })
}

#[cfg(not(target_os = "windows"))]
fn extract_thumbnail_bitmap_windows(
    _file_path: &str,
    _max_size: i32,
) -> Result<ThumbnailBitmap, String> {
    Err("Thumbnail extraction is only supported on Windows".to_string())
}

fn extract_thumbnail_windows(file_path: &str, max_size: i32) -> Result<String, String> {
    extract_thumbnail_bitmap_windows(file_path, max_size)?.to_bmp_data_url()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ThumbnailHashes {
    rotations: [u64; 4],
}

/// 1ファイルのサムネイルを4方向のpHashへ変換する。
fn hash_thumbnail_rotations(bitmap: &ThumbnailBitmap) -> Result<ThumbnailHashes, String> {
    if bitmap.width == 0
        || bitmap.height == 0
        || bitmap.pixels.len() != bitmap.width * bitmap.height * 4
    {
        return Err("Invalid thumbnail bitmap".to_string());
    }

    let mut rotations = [0u64; 4];
    for (turns, hash) in rotations.iter_mut().enumerate() {
        let rotated = rotate_thumbnail(bitmap, turns);
        *hash = calculate_phash(&rotated);
    }

    Ok(ThumbnailHashes { rotations })
}

/// 縮小前のピクセルをメモリ上で90度単位に回転する。
fn rotate_thumbnail(bitmap: &ThumbnailBitmap, turns: usize) -> ThumbnailBitmap {
    let turns = turns % 4;
    if turns == 0 {
        return bitmap.clone();
    }

    let (destination_width, destination_height) = if turns % 2 == 0 {
        (bitmap.width, bitmap.height)
    } else {
        (bitmap.height, bitmap.width)
    };
    let mut pixels = vec![0u8; destination_width * destination_height * 4];

    for source_y in 0..bitmap.height {
        for source_x in 0..bitmap.width {
            let (destination_x, destination_y) = match turns {
                1 => (bitmap.height - 1 - source_y, source_x),
                2 => (bitmap.width - 1 - source_x, bitmap.height - 1 - source_y),
                3 => (source_y, bitmap.width - 1 - source_x),
                _ => unreachable!(),
            };
            let source_index = (source_y * bitmap.width + source_x) * 4;
            let destination_index = (destination_y * destination_width + destination_x) * 4;
            pixels[destination_index..destination_index + 4]
                .copy_from_slice(&bitmap.pixels[source_index..source_index + 4]);
        }
    }

    ThumbnailBitmap {
        width: destination_width,
        height: destination_height,
        pixels,
    }
}

/// 64bit pHash。DCTの低周波8x8係数を使い、DC成分を除く中央値で二値化する。
fn calculate_phash(bitmap: &ThumbnailBitmap) -> u64 {
    let grayscale = bitmap
        .pixels
        .chunks_exact(4)
        .map(|pixel| {
            let blue = pixel[0] as f64;
            let green = pixel[1] as f64;
            let red = pixel[2] as f64;
            let alpha = pixel[3] as f64 / 255.0;
            let luminance = 0.114 * blue + 0.587 * green + 0.299 * red;
            luminance * alpha + 255.0 * (1.0 - alpha)
        })
        .collect::<Vec<_>>();

    let normalized = resize_grayscale(&grayscale, bitmap.width, bitmap.height, PHASH_INPUT_SIZE);
    let mut coefficients = [0.0f64; PHASH_LOW_FREQUENCY_SIZE * PHASH_LOW_FREQUENCY_SIZE];
    let input_size = PHASH_INPUT_SIZE as f64;
    let pi = std::f64::consts::PI;

    for v in 0..PHASH_LOW_FREQUENCY_SIZE {
        for u in 0..PHASH_LOW_FREQUENCY_SIZE {
            let scale_u = if u == 0 {
                (1.0 / input_size).sqrt()
            } else {
                (2.0 / input_size).sqrt()
            };
            let scale_v = if v == 0 {
                (1.0 / input_size).sqrt()
            } else {
                (2.0 / input_size).sqrt()
            };
            let mut sum = 0.0;

            for y in 0..PHASH_INPUT_SIZE {
                let cosine_y = (((2 * y + 1) as f64 * v as f64 * pi) / (2.0 * input_size)).cos();
                for x in 0..PHASH_INPUT_SIZE {
                    let cosine_x =
                        (((2 * x + 1) as f64 * u as f64 * pi) / (2.0 * input_size)).cos();
                    sum += normalized[y * PHASH_INPUT_SIZE + x] * cosine_x * cosine_y;
                }
            }

            coefficients[v * PHASH_LOW_FREQUENCY_SIZE + u] = sum * scale_u * scale_v;
        }
    }

    let mut median_values = coefficients[1..].to_vec();
    median_values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = median_values[median_values.len() / 2];

    coefficients
        .iter()
        .enumerate()
        .fold(0u64, |hash, (index, coefficient)| {
            if *coefficient > median {
                hash | (1u64 << index)
            } else {
                hash
            }
        })
}

/// グレースケール画像を正方形へbilinear補間で縮小する。
fn resize_grayscale(
    source: &[f64],
    source_width: usize,
    source_height: usize,
    destination_size: usize,
) -> Vec<f64> {
    let mut destination = vec![0.0; destination_size * destination_size];
    let width_scale = source_width as f64 / destination_size as f64;
    let height_scale = source_height as f64 / destination_size as f64;

    for destination_y in 0..destination_size {
        let source_y = ((destination_y as f64 + 0.5) * height_scale - 0.5)
            .max(0.0)
            .min((source_height - 1) as f64);
        let y0 = source_y.floor() as usize;
        let y1 = (y0 + 1).min(source_height - 1);
        let y_weight = source_y - y0 as f64;

        for destination_x in 0..destination_size {
            let source_x = ((destination_x as f64 + 0.5) * width_scale - 0.5)
                .max(0.0)
                .min((source_width - 1) as f64);
            let x0 = source_x.floor() as usize;
            let x1 = (x0 + 1).min(source_width - 1);
            let x_weight = source_x - x0 as f64;

            let top_left = source[y0 * source_width + x0];
            let top_right = source[y0 * source_width + x1];
            let bottom_left = source[y1 * source_width + x0];
            let bottom_right = source[y1 * source_width + x1];
            let top = top_left + (top_right - top_left) * x_weight;
            let bottom = bottom_left + (bottom_right - bottom_left) * x_weight;
            destination[destination_y * destination_size + destination_x] =
                top + (bottom - top) * y_weight;
        }
    }

    destination
}

fn min_rotation_distance(left: ThumbnailHashes, right: ThumbnailHashes) -> u32 {
    left.rotations
        .iter()
        .flat_map(|left_hash| {
            right
                .rotations
                .iter()
                .map(move |right_hash| (*left_hash ^ *right_hash).count_ones())
        })
        .min()
        .unwrap_or(u32::MAX)
}

fn assign_similarity_sets(
    group: &mut DuplicateGroup,
    thumbnail_hashes: &[Option<ThumbnailHashes>],
) {
    let mut assigned = vec![false; group.files.len()];
    let mut next_set_id = 1u32;

    for seed in 0..group.files.len() {
        if assigned[seed] || thumbnail_hashes[seed].is_none() {
            continue;
        }

        assigned[seed] = true;
        let mut members = vec![seed];

        for candidate in (seed + 1)..group.files.len() {
            if assigned[candidate] {
                continue;
            }
            let Some(candidate_hashes) = thumbnail_hashes[candidate] else {
                continue;
            };
            if members.iter().all(|member| {
                min_rotation_distance(thumbnail_hashes[*member].unwrap(), candidate_hashes)
                    <= PHASH_MAX_HAMMING_DISTANCE
            }) {
                assigned[candidate] = true;
                members.push(candidate);
            }
        }

        if members.len() >= 2 {
            for member in members {
                group.files[member].similar_set_id = Some(next_set_id);
                group.files[member].similarity_status = SimilarityStatus::Similar;
            }
            next_set_id += 1;
        }
    }
}

fn analyze_similarity_group_with_provider<F>(group: &mut DuplicateGroup, mut provider: F)
where
    F: FnMut(&str) -> Result<ThumbnailBitmap, String>,
{
    let mut thumbnail_hashes = vec![None; group.files.len()];

    for (index, file) in group.files.iter_mut().enumerate() {
        file.similar_set_id = None;
        match provider(&file.path).and_then(|bitmap| hash_thumbnail_rotations(&bitmap)) {
            Ok(hashes) => {
                file.similarity_status = SimilarityStatus::NotSimilar;
                thumbnail_hashes[index] = Some(hashes);
            }
            Err(_) => {
                file.similarity_status = SimilarityStatus::Unavailable;
            }
        }
    }

    assign_similarity_sets(group, &thumbnail_hashes);
}

fn analyze_similarity_group(group: &mut DuplicateGroup) {
    analyze_similarity_group_with_provider(group, |file_path| {
        extract_thumbnail_bitmap_windows(file_path, THUMBNAIL_REQUEST_SIZE)
    });
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
        "mp4" | "webm" | "ogg" | "mov" | "avi" | "mkv" | "wmv" | "flv" | "m4v" | "m2ts" | "mts"
        | "3gp" => {
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

    fn test_group(paths: &[&str]) -> DuplicateGroup {
        DuplicateGroup {
            hash: None,
            size: 1_000,
            min_size: 1_000,
            max_size: 1_000,
            max_size_delta: 0,
            comparison_type: "size_tolerance".to_string(),
            files: paths
                .iter()
                .map(|path| FileInfo {
                    path: (*path).to_string(),
                    name: (*path).to_string(),
                    size: 1_000,
                    hash: None,
                    extension: "bin".to_string(),
                    similar_set_id: None,
                    similarity_status: SimilarityStatus::NotRequested,
                })
                .collect(),
        }
    }

    fn hashes(value: u64) -> ThumbnailHashes {
        ThumbnailHashes {
            rotations: [value; 4],
        }
    }

    fn test_bitmap(pattern: u8) -> ThumbnailBitmap {
        let width = 64;
        let height = 48;
        let mut pixels = Vec::with_capacity(width * height * 4);

        for y in 0..height {
            for x in 0..width {
                let bright = match pattern {
                    0 => ((x / 8) + (y / 8)) % 2 == 0,
                    1 => (y / 6) % 2 == 0,
                    _ => (x + y) % 3 == 0,
                };
                let value = if bright { 255 } else { 0 };
                pixels.extend_from_slice(&[value, value, value, 255]);
            }
        }

        ThumbnailBitmap {
            width,
            height,
            pixels,
        }
    }

    #[test]
    fn test_phash_is_stable_for_identical_thumbnail() {
        let bitmap = test_bitmap(0);
        let left = hash_thumbnail_rotations(&bitmap).unwrap();
        let right = hash_thumbnail_rotations(&bitmap).unwrap();

        assert_eq!(left, right);
        assert_eq!(min_rotation_distance(left, right), 0);
    }

    #[test]
    fn test_phash_accepts_quarter_turn_rotations() {
        let bitmap = test_bitmap(2);
        let left = hash_thumbnail_rotations(&bitmap).unwrap();

        for turns in 1..=3 {
            let rotated = rotate_thumbnail(&bitmap, turns);
            let right = hash_thumbnail_rotations(&rotated).unwrap();
            assert!(
                min_rotation_distance(left, right) <= PHASH_MAX_HAMMING_DISTANCE,
                "rotation {} should remain similar",
                turns * 90
            );
        }
    }

    #[test]
    fn test_phash_rejects_distinct_thumbnail_pattern() {
        let left = hash_thumbnail_rotations(&test_bitmap(0)).unwrap();
        let right = hash_thumbnail_rotations(&test_bitmap(1)).unwrap();

        assert!(min_rotation_distance(left, right) > PHASH_MAX_HAMMING_DISTANCE);
    }

    #[test]
    fn test_similarity_sets_do_not_chain() {
        let mut group = test_group(&["a", "b", "c"]);
        for file in &mut group.files {
            file.similarity_status = SimilarityStatus::NotSimilar;
        }

        // a-b is distance 1, b-c is distance 8, but a-c is distance 9.
        let thumbnail_hashes = vec![Some(hashes(0)), Some(hashes(1)), Some(hashes(511))];
        assign_similarity_sets(&mut group, &thumbnail_hashes);

        assert_eq!(group.files[0].similar_set_id, Some(1));
        assert_eq!(group.files[1].similar_set_id, Some(1));
        assert_eq!(group.files[2].similar_set_id, None);
        assert_eq!(
            group.files[2].similarity_status,
            SimilarityStatus::NotSimilar
        );
    }

    #[test]
    fn test_similarity_sets_support_multiple_sets() {
        let mut group = test_group(&["a", "b", "c", "d", "e"]);
        for file in &mut group.files {
            file.similarity_status = SimilarityStatus::NotSimilar;
        }

        let thumbnail_hashes = vec![
            Some(hashes(0)),
            Some(hashes(1)),
            Some(hashes(0x1FF << 20)),
            Some(hashes((0x1FF << 20) | 1)),
            Some(hashes(0x1FF << 40)),
        ];
        assign_similarity_sets(&mut group, &thumbnail_hashes);

        assert_eq!(group.files[0].similar_set_id, Some(1));
        assert_eq!(group.files[1].similar_set_id, Some(1));
        assert_eq!(group.files[2].similar_set_id, Some(2));
        assert_eq!(group.files[3].similar_set_id, Some(2));
        assert_eq!(group.files[4].similar_set_id, None);
    }

    #[test]
    fn test_thumbnail_failure_keeps_file_and_requests_each_file_once() {
        let mut group = test_group(&["first", "second", "missing"]);
        let bitmap = test_bitmap(0);
        let mut requested = Vec::new();

        analyze_similarity_group_with_provider(&mut group, |path| {
            requested.push(path.to_string());
            if path == "missing" {
                Err("thumbnail unavailable".to_string())
            } else {
                Ok(bitmap.clone())
            }
        });

        assert_eq!(requested, vec!["first", "second", "missing"]);
        assert_eq!(group.files.len(), 3);
        assert_eq!(group.files[0].similarity_status, SimilarityStatus::Similar);
        assert_eq!(group.files[1].similarity_status, SimilarityStatus::Similar);
        assert_eq!(
            group.files[2].similarity_status,
            SimilarityStatus::Unavailable
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires the Windows thumbnail provider"]
    fn test_windows_thumbnail_similarity_smoke() {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("app-icon.png");
        assert!(
            source.exists(),
            "test source image must exist: {}",
            source.display()
        );

        let test_dir = std::env::temp_dir().join(format!(
            "file-duplicate-organizer-thumbnail-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&test_dir);
        fs::create_dir_all(&test_dir).unwrap();
        let first = test_dir.join("first.png");
        let second = test_dir.join("second.png");
        fs::copy(&source, &first).unwrap();
        fs::copy(&source, &second).unwrap();

        let groups =
            scan_for_duplicates(&test_dir.to_string_lossy(), "size_only", false, 0, true).unwrap();

        let _ = fs::remove_dir_all(&test_dir);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].files.len(), 2);
        assert_eq!(
            groups[0].files[0].similarity_status,
            SimilarityStatus::Similar
        );
        assert_eq!(
            groups[0].files[1].similarity_status,
            SimilarityStatus::Similar
        );
        assert_eq!(groups[0].files[0].similar_set_id, Some(1));
        assert_eq!(groups[0].files[1].similar_set_id, Some(1));
    }

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
        let groups = scan_for_duplicates(test_dir, "strict", false, 0, false).unwrap();

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
    fn test_size_tolerance_grouping() {
        let file_sizes = vec![
            (100, PathBuf::from("a.bin")),
            (900, PathBuf::from("b.bin")),
            (1_500, PathBuf::from("c.bin")),
            (4_000, PathBuf::from("d.bin")),
            (4_900, PathBuf::from("e.bin")),
        ];

        let groups = group_files_by_size_tolerance(&file_sizes, 1_024);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, 100);
        assert_eq!(groups[0].1, 900);
        assert_eq!(groups[0].2.len(), 2);
        assert_eq!(groups[1].0, 4_000);
        assert_eq!(groups[1].1, 4_900);
        assert_eq!(groups[1].2.len(), 2);
    }

    #[test]
    fn test_size_only_scan_with_tolerance() {
        let test_dir = "test_size_tolerance_dir";
        let _ = fs::remove_dir_all(test_dir);
        fs::create_dir(test_dir).unwrap();

        let file1 = PathBuf::from(test_dir).join("file1.bin");
        let file2 = PathBuf::from(test_dir).join("file2.bin");
        let file3 = PathBuf::from(test_dir).join("file3.bin");

        File::create(&file1)
            .unwrap()
            .write_all(&vec![b'a'; 1_000])
            .unwrap();
        File::create(&file2)
            .unwrap()
            .write_all(&vec![b'b'; 1_900])
            .unwrap();
        File::create(&file3)
            .unwrap()
            .write_all(&vec![b'c'; 3_000])
            .unwrap();

        let groups = scan_for_duplicates(test_dir, "size_only", false, 1_024, false).unwrap();

        let _ = fs::remove_dir_all(test_dir);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].comparison_type, "size_tolerance");
        assert_eq!(groups[0].max_size_delta, 900);
        assert!(groups[0].hash.is_none());
        assert_eq!(groups[0].files.len(), 2);
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
