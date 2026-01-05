// src/server.rs

use log::{error, info, warn};
use std::path::PathBuf;
use std::sync::Arc; // Needed for Arc<DashMap>
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tonic::{Request, Response, Status};

// FIX: Added DashMap for fine-grained concurrent access control
use dashmap::DashMap;

// Includes the auto-generated gRPC code
pub mod filerpc {
    tonic::include_proto!("filerpc");
}
use filerpc::{
    file_service_server::FileService, DirEntry, FileChunk, ListDirRequest, ListDirResponse,
    UploadStatus,
};

// --- Static Path Lock Manager ---
// This DashMap holds the canonical paths of all files currently being written to.
// The key is the canonical PathBuf, and the value is a placeholder ().
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
        MyFileService {
            base_path: PathBuf::from("data"),
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

        // This will hold the canonical path used for locking
        let mut canonical_final_path: Option<PathBuf> = None;

        while let Some(chunk) = stream.message().await? {
            // First chunk setup: determine path, acquire lock, and create file
            if filename.is_none() {
                if chunk.filename.is_empty() {
                    return Err(Status::invalid_argument("Filename cannot be empty"));
                }

                let upload_dir = self.base_path.join(&chunk.target_dir);
                let final_path = upload_dir.join(&chunk.filename);

                // --- CONCURRENCY LOCK START ---

                // 1. Calculate the canonical path for robust locking
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
        // Ensure the lock is released after successful upload, regardless of how the function exits (via Ok or early ?).
        // Since we are using an asynchronous context, we MUST remove the lock explicitly here,
        // as a standard RAII guard would require complex wrappers (like scopeguard::guard)
        // that are difficult to manage with async code returning Result.
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
        let path_str = req.path;

        // Use the base_path from the struct
        let full_path = self.base_path.join(&path_str);

        // Security check: Canonicalize and verify path is still under the base directory
        let canonical_base = self
            .base_path
            .canonicalize()
            .map_err(|_| Status::internal("Server base directory is invalid"))?;

        // Canonicalize the requested path
        let canonical_path = full_path.canonicalize().map_err(|e| {
            warn!(
                "Directory query failed (path invalid/not found): {} -> {}",
                path_str, e
            );
            Status::not_found(format!("Directory not found or inaccessible: {}", path_str))
        })?;

        // Path traversal check
        if !canonical_path.starts_with(&canonical_base) {
            error!(
                "Path traversal attempt detected: {}",
                canonical_path.display()
            );
            return Err(Status::permission_denied("Access to this path is denied"));
        }

        info!("Querying directory: {}", canonical_path.display());

        let mut entries = Vec::new();

        // FIX: Use tokio::fs::read_dir (asynchronous)
        match fs::read_dir(&canonical_path).await {
            Ok(mut dir) => {
                // FIX: Iterate using async iteration
                while let Some(entry_result) = dir.next_entry().await.transpose() {
                    match entry_result {
                        Ok(entry) => {
                            // FIX: Use entry.metadata().await (asynchronous)
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
