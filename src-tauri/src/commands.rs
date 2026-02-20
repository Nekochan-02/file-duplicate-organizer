use crate::scanner;
use tauri::command;

/// フォルダをスキャンして重複グループを返す
#[command]
pub fn scan_folder(path: String) -> Result<Vec<scanner::DuplicateGroup>, String> {
    scanner::scan_for_duplicates(&path)
}

/// ファイルのプレビューを取得
#[command]
pub fn get_file_preview(path: String) -> Result<scanner::FilePreview, String> {
    scanner::get_preview(&path)
}

/// 選択されたファイルをゴミ箱に移動
#[command]
pub fn delete_files(paths: Vec<String>) -> Result<scanner::DeleteResult, String> {
    scanner::delete_files_to_trash(&paths)
}
