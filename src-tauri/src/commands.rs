// src-tauri/src/commands.rs (新增)

use log::{error, info};
use std::path::PathBuf;
use tauri::State;

// 用于前端显示的文件/目录结构
#[derive(serde::Serialize, Clone)]
pub struct LocalDirEntry {
    name: String,
    is_dir: bool,
    size: u64, // 新增文件大小字段
}

// 假设我们使用 dirs 库获取 Home 目录，需要确保在 Cargo.toml 中已添加 dirs = "5.0"
use dirs;

fn get_local_base_path() -> PathBuf {
    // 默认使用 Home 目录作为本地根目录
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

#[tauri::command]
pub async fn list_local_dir(path: String) -> Result<(Vec<LocalDirEntry>, String), String> {
    let base = get_local_base_path();
    let requested_path = path.trim_start_matches('/');

    // 路径拼接
    let full_path = base.join(requested_path);

    info!("Listing local directory: {}", full_path.display());

    // 路径遍历检查 (防止访问 Home 目录以外的系统路径)
    // 规范化 base path
    let canonical_base = base
        .canonicalize()
        .map_err(|e| format!("Invalid base path: {}", e))?;

    // 规范化 requested path
    let canonical_path = full_path
        .canonicalize()
        .map_err(|e| format!("Directory not found or inaccessible: {}", e))?;

    if !canonical_path.starts_with(&canonical_base) {
        error!(
            "Local path traversal attempt detected: {}",
            canonical_path.display()
        );
        return Err("Access to this local path is restricted.".to_string());
    }

    let mut entries = Vec::new();

    match tokio::fs::read_dir(&canonical_path).await {
        Ok(mut dir) => {
            while let Some(entry_result) = dir.next_entry().await.transpose() {
                match entry_result {
                    Ok(entry) => {
                        let metadata = match entry.metadata().await {
                            Ok(m) => m,
                            Err(_) => continue, // 无法获取元数据则跳过
                        };

                        let name = entry.file_name().to_string_lossy().into_owned();

                        // 忽略隐藏文件/目录
                        if name.starts_with('.') && name != "." && name != ".." {
                            continue;
                        }

                        entries.push(LocalDirEntry {
                            name,
                            is_dir: metadata.is_dir(),
                            size: metadata.len(),
                        });
                    }
                    Err(e) => {
                        error!("Failed to read local directory entry: {}", e);
                    }
                }
            }
        }
        Err(e) => {
            return Err(format!("Could not read local directory: {}", e));
        }
    }

    // 返回文件列表和当前规范化路径
    Ok((entries, canonical_path.to_string_lossy().to_string()))
}
