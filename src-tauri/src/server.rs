// src/server.rs

use dashmap::DashMap;
use dirs;
use log::{error, info, warn};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tonic::{Request, Response, Status};

// Includes the auto-generated gRPC code
pub mod filerpc {
    tonic::include_proto!("filerpc");
}
use filerpc::{
    file_service_server::FileService, DirEntry, FileChunk, ListDirRequest, ListDirResponse,
    UploadStatus,
};

// --- Static Path Lock Manager ---
type PathLockMap = Arc<DashMap<PathBuf, ()>>;

// --- FileService Implementation Struct ---
#[derive(Debug)]
pub struct MyFileService {
    // The base directory where all files will be stored.
    base_path: PathBuf,
    // FIX: Fine-grained lock manager for path conflict resolution
    active_uploads: PathLockMap,
}

// Custom implementation of Default to initialize base_path
impl Default for MyFileService {
    fn default() -> Self {
        let default_base_path = match dirs::home_dir() {
            Some(path) => path,
            None => PathBuf::from("/tmp"), // 使用 /tmp 作为最后的 fallback
        };

        MyFileService {
            // 确保 default 也使用 Home 目录
            base_path: default_base_path,
            active_uploads: Arc::new(DashMap::new()),
        }
    }
}

// Implementation of the constructor required by integration tests
impl MyFileService {
    /// Creates a new MyFileService instance using the specified base directory.
    pub fn new(base_path: PathBuf) -> Self {
        MyFileService {
            base_path,
            active_uploads: Arc::new(DashMap::new()),
        }
    }
}

#[tonic::async_trait]
impl FileService for MyFileService {
    /// 1. Stream file upload (Client Streaming RPC)
    async fn upload_file(
        &self,
        request: Request<tonic::Streaming<FileChunk>>,
    ) -> Result<Response<UploadStatus>, Status> {
        info!("Received file upload request...");
        let mut stream = request.into_inner();
        let mut filename: Option<String> = None;
        let mut file_data: Option<fs::File> = None;
        let mut bytes_written = 0;

        let mut canonical_final_path: Option<PathBuf> = None;

        while let Some(chunk) = stream.message().await? {
            // First chunk setup: determine path, acquire lock, and create file
            if filename.is_none() {
                if chunk.filename.is_empty() {
                    return Err(Status::invalid_argument("Filename cannot be empty"));
                }

                // --- FIX START: 路径拼接修正 ---
                // 客户端发来的 target_dir 可能包含前导 '/'，这将导致 PathBuf::join 覆盖 self.base_path。
                let target_rel_path = chunk.target_dir.trim_start_matches('/');

                let upload_dir = self.base_path.join(target_rel_path);
                let final_path = upload_dir.join(&chunk.filename);
                // --- FIX END ---

                // --- CONCURRENCY LOCK START ---

                // 1. Calculate the canonical path for robust locking
                // 尝试规范化路径，如果失败（例如目录不存在），则使用原始路径
                let path_to_lock = final_path.canonicalize().unwrap_or(final_path.clone());

                // 2. Try to insert into the DashMap. If it returns Some, the path is already locked.
                if self.active_uploads.contains_key(&path_to_lock) {
                    error!(
                        "Concurrent write attempt detected for: {}",
                        path_to_lock.display()
                    );
                    return Err(Status::unavailable(
                        "File is currently being written by another client. Try again later.",
                    ));
                }
                self.active_uploads.insert(path_to_lock.clone(), ());
                canonical_final_path = Some(path_to_lock);

                // --- CONCURRENCY LOCK ACQUIRED ---

                // FIX: Use tokio::fs::create_dir_all (asynchronous)
                if let Err(e) = fs::create_dir_all(&upload_dir).await {
                    error!("Failed to create target directory: {}", e);
                    // Lock must be released on failure
                    if let Some(p) = canonical_final_path.as_ref() {
                        self.active_uploads.remove(p);
                    }
                    return Err(Status::internal(format!(
                        "Failed to create directory: {}",
                        e
                    )));
                }

                info!(
                    "Starting to receive file: {} to directory: {}",
                    chunk.filename,
                    upload_dir.display()
                );

                // FIX: Use tokio::fs::File::create (asynchronous)
                match fs::File::create(&final_path).await {
                    Ok(f) => {
                        file_data = Some(f);
                        filename = Some(chunk.filename.clone());
                    }
                    Err(e) => {
                        error!("Failed to create file: {}", e);
                        // Lock must be released on failure
                        if let Some(p) = canonical_final_path.as_ref() {
                            self.active_uploads.remove(p);
                        }
                        return Err(Status::internal(format!("Could not create file: {}", e)));
                    }
                }
            }

            // Write data chunk
            if let Some(ref mut file) = file_data {
                // FIX: Use AsyncWriteExt::write_all(file, &chunk.data).await (asynchronous)
                if let Err(e) = AsyncWriteExt::write_all(file, &chunk.data).await {
                    error!("Failed to write file data: {}", e);
                    // Lock must be released on failure
                    if let Some(p) = canonical_final_path.as_ref() {
                        self.active_uploads.remove(p);
                    }
                    return Err(Status::internal(format!("Failed to write data: {}", e)));
                }
                bytes_written += chunk.data.len();
            }

            // Check for EOF flag
            if chunk.eof {
                break;
            }
        }

        // --- CONCURRENCY LOCK RELEASE ---
        if let Some(p) = canonical_final_path.as_ref() {
            self.active_uploads.remove(p);
        } else {
            // Handle case where stream ended before first chunk (unlikely but safe)
            return Err(Status::internal(
                "File stream ended without receiving first chunk metadata.",
            ));
        }

        info!(
            "File {} upload successful. Total size: {} bytes.",
            filename.unwrap_or_else(|| "unknown file".to_string()),
            bytes_written
        );

        let reply = UploadStatus {
            success: true,
            message: format!(
                "File uploaded successfully. Total bytes written: {}.",
                bytes_written
            ),
        };

        Ok(Response::new(reply))
    }

    /// 2. List directory contents (Unary RPC)
    async fn list_dir(
        &self,
        request: Request<ListDirRequest>,
    ) -> Result<Response<ListDirResponse>, Status> {
        let req = request.into_inner();
        let path_str = req.path; // 客户端请求的路径，例如 "/" 或 "Documents/Photos"

        // --- FIX START: 路径构建修正 ---
        let mut full_path = self.base_path.clone();

        // 如果客户端请求的不是根路径，则将其附加到 base_path
        if path_str != "/" && !path_str.is_empty() {
            // 移除前导的 '/'，确保路径被正确地 join 到 base_path 后面
            full_path.push(path_str.trim_start_matches('/'));
        }
        // --- FIX END ---

        // 1. 规范化 base_path (沙箱根目录)
        let canonical_base = self.base_path.canonicalize().map_err(|e| {
            error!("Failed to canonicalize server base path: {}", e);
            Status::internal("Server base directory is invalid or inaccessible")
        })?;

        // 2. 规范化请求路径
        let canonical_path = full_path.canonicalize().map_err(|e| {
            warn!(
                "Directory query failed (path invalid/not found): {} -> {}",
                path_str, e
            );
            Status::not_found(format!("Directory not found or inaccessible: {}", path_str))
        })?;

        // --- 修复点 B: 路径遍历检查 (沙箱机制) ---
        // 检查 canonical_path 是否以 canonical_base 开头。
        if !canonical_path.starts_with(&canonical_base) {
            error!(
                "Path traversal attempt detected: {} (Base: {})",
                canonical_path.display(),
                canonical_base.display()
            );
            return Err(Status::permission_denied("Access to this path is denied"));
        }

        info!("Querying directory: {}", canonical_path.display());

        let mut entries = Vec::new();

        // FIX: 使用 tokio::fs::read_dir 异步读取目录
        match tokio::fs::read_dir(&canonical_path).await {
            Ok(mut dir) => {
                // FIX: 迭代使用 async iteration
                while let Some(entry_result) = dir.next_entry().await.transpose() {
                    match entry_result {
                        Ok(entry) => {
                            // FIX: 使用 entry.metadata().await 异步获取元数据
                            let metadata = entry.metadata().await.map_err(|e| {
                                error!("Failed to get directory entry metadata: {}", e);
                                Status::internal("Could not get file metadata")
                            })?;

                            let name = entry.file_name().to_string_lossy().into_owned();

                            entries.push(DirEntry {
                                name,
                                is_dir: metadata.is_dir(),
                            });
                        }
                        Err(e) => {
                            error!("Failed to read directory entry: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                error!(
                    "Failed to read directory: {} -> {}",
                    canonical_path.display(),
                    e
                );
                return Err(Status::internal(format!("Could not read directory: {}", e)));
            }
        }

        let reply = ListDirResponse { entries };
        Ok(Response::new(reply))
    }
}
