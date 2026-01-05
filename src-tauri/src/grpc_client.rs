// src/grpc_client.rs

use anyhow::Context;
use log::{error, info};
use parking_lot::Mutex; // Used for fast, sync State management
use serde::Serialize;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::State;
use tokio::sync::mpsc;
use tokio::task;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Channel;

// FIX: 引入 dirs crate 依赖
use dirs;

// 从原 src/client.rs 复制
const CHUNK_SIZE: usize = 1024 * 64; // 64 KB

// 引入 gRPC 结构 (确保 tonic::include_proto! 在某处被执行，通常在 build.rs 或 main.rs)
pub mod filerpc {
    tonic::include_proto!("filerpc");
}
use filerpc::{file_service_client::FileServiceClient, FileChunk, ListDirRequest};

// --- GUI 数据结构 ---

/// 用于在 Rust 后端和 Web 前端之间传输的文件/目录信息
#[derive(Debug, Serialize)]
pub struct GuiDirEntry {
    pub name: String,
    pub is_dir: bool,
}

// --- 客户端状态管理 ---

/// 在 Tauri 运行时中共享的 gRPC 客户端连接状态
pub struct ClientState(Mutex<Option<FileServiceClient<Channel>>>);

impl ClientState {
    pub fn new() -> Self {
        ClientState(Mutex::new(None))
    }

    /// Helper to get a clone of the client, returning a Tauri::Status error if disconnected.
    fn get_client(&self) -> Result<FileServiceClient<Channel>, tonic::Status> {
        let client_lock = self.0.lock();
        client_lock
            .as_ref()
            .cloned() // ClientServiceClient 实现了 Clone
            .ok_or_else(|| tonic::Status::unavailable("Not connected to server."))
    }
}

// --- Tauri Commands (gRPC 包装器) ---

/// 1. 连接到服务器
#[tauri::command]
pub async fn connect_server(state: State<'_, ClientState>, url: String) -> Result<String, String> {
    info!("Attempting to connect to {}", url);

    // Tonic connection requires a scheme (http:// or https://)
    let server_url = if url.starts_with("http://") || url.starts_with("https://") {
        url
    } else {
        format!("http://{}", url)
    };

    match FileServiceClient::connect(server_url.clone()).await {
        Ok(client) => {
            let mut client_lock = state.0.lock();
            *client_lock = Some(client);
            info!("Successfully connected to Server.");
            Ok(format!("连接成功: {}", server_url))
        }
        Err(e) => {
            error!("Connection failed: {}", e);
            Err(format!("连接失败: {}", e))
        }
    }
}

/// 2. 列出远程目录内容
#[tauri::command]
pub async fn list_remote_dir(
    state: State<'_, ClientState>,
    path: String,
) -> Result<Vec<GuiDirEntry>, String> {
    let mut client = state.get_client().map_err(|e| e.message().to_string())?;

    info!("Attempting to list remote directory: {}", path);

    let request = tonic::Request::new(ListDirRequest { path: path });

    match client.list_dir(request).await {
        Ok(response) => {
            let entries = response
                .into_inner()
                .entries
                .into_iter()
                .map(|e| GuiDirEntry {
                    name: e.name,
                    is_dir: e.is_dir,
                })
                .collect();
            Ok(entries)
        }
        Err(e) => {
            error!("Failed to list directory: {}", e.message());
            Err(format!("列目录失败: {}", e.message()))
        }
    }
}

// 3. 上传文件 (核心逻辑源自原 src/client.rs::upload_file)
#[tauri::command]
pub async fn upload_local_file(
    state: State<'_, ClientState>,
    local_path: String,
    target_dir: String,
) -> Result<String, String> {
    // [LOG A: 初始日志]
    info!(
        "upload_local_file attempt from {:?} to {:?}",
        &local_path, &target_dir
    );

    // 1. 获取 gRPC 客户端
    let mut client = state.get_client().map_err(|e| {
        error!(
            "UPLOAD ERROR (Step 1): Failed to get gRPC client. Status: {}",
            e.message()
        );
        e.message().to_string()
    })?;

    // 2. 验证本地文件路径和提取文件名

    // FIX START: 强制修正路径逻辑
    let home_dir = dirs::home_dir().ok_or_else(|| {
        error!("UPLOAD ERROR (Step 2.1): Could not determine user home directory.");
        "无法确定用户主目录".to_string()
    })?;

    let relative_path = Path::new(&local_path);

    // 尝试移除路径前导的 '/'，如果存在的话，确保路径是相对于 Home 目录的。
    let corrected_path = if let Ok(stripped) = relative_path.strip_prefix("/") {
        stripped
    } else if let Ok(stripped) = relative_path.strip_prefix("\\") {
        // 兼容 Windows 路径
        stripped
    } else {
        relative_path
    };

    // 最终的绝对路径 = Home 目录 + 修正后的相对路径
    let actual_path = home_dir.join(corrected_path);

    info!("Path constructed: {:?}", actual_path);
    // FIX END

    let filename = actual_path
        .file_name()
        .ok_or_else(|| {
            error!(
                "UPLOAD ERROR (Step 2): Local path invalid or missing filename: {:?}",
                actual_path
            );
            "本地文件路径无效或缺少文件名".to_string()
        })?
        .to_string_lossy()
        .into_owned();

    // 3. 打开本地文件
    // 在主异步函数中打开文件，以进行错误处理
    let file = std::fs::File::open(&actual_path).map_err(|e| {
        error!(
            "UPLOAD ERROR (Step 3): Failed to open local file {:?}. Error: {}",
            actual_path, e
        );
        format!("打开本地文件失败: {}", e)
    })?;

    // (tx_main, rx) - 主线程持有 tx_main
    let (tx_main, rx) = mpsc::channel(4);

    let filename_owned = filename.clone();
    let target_dir_owned = target_dir.to_string();

    // 将 tx_main 克隆给 spawn_blocking 任务
    let tx_blocking = tx_main.clone();

    let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);

    // [LOG B: 文件信息日志]
    info!("Starting upload for: {} ({} bytes)", filename, file_size);

    // 4. 启动阻塞任务进行 I/O
    task::spawn_blocking(move || {
        let mut file = file;
        let mut buffer = vec![0u8; CHUNK_SIZE];
        let mut eof = false;

        loop {
            let bytes_read = match file.read(&mut buffer) {
                Ok(0) => {
                    eof = true;
                    0
                }
                Ok(n) => n,
                Err(e) => {
                    error!(
                        "UPLOAD ERROR (Step 4.1): Failed to read local file chunk: {}",
                        e
                    );
                    // 如果读取失败，也应该 break，让 tx_blocking 自动 drop
                    break;
                }
            };

            // ... (chunk 构造逻辑不变) ...

            let chunk_data = if bytes_read > 0 {
                &buffer[..bytes_read]
            } else {
                &[]
            };

            let chunk = FileChunk {
                filename: filename_owned.clone(),
                target_dir: target_dir_owned.clone(),
                data: chunk_data.to_vec(),
                eof,
            };

            // Use blocking_send inside spawn_blocking
            if tx_blocking.blocking_send(chunk).is_err() {
                // 这个错误通常是因为接收端 rx 提前关闭，这意味着 gRPC 调用已经失败或取消
                error!("UPLOAD ERROR (Step 4.2): Failed to send chunk to gRPC stream (receiver closed)");
                break;
            }

            if eof {
                break;
            }
        }
        // 当此 spawn_blocking 任务结束时，tx_blocking 被 drop
    });

    // 5. 丢弃主线程的 Sender，允许流终止
    drop(tx_main);

    let request_stream = tonic::Request::new(ReceiverStream::new(rx));

    // 6. 发起 gRPC 调用
    match client.upload_file(request_stream).await {
        Ok(response) => {
            let inner = response.into_inner();
            if inner.success {
                info!("UPLOAD SUCCESS: Server returned success status.");
                Ok(format!("✅ 上传成功: {}", inner.message))
            } else {
                // 如果服务器返回 success: false
                error!(
                    "UPLOAD FAILED (Step 6.1): Server returned failure status: {}",
                    inner.message
                );
                Err(format!("❌ 上传失败: {}", inner.message))
            }
        }
        Err(e) => {
            // gRPC 调用失败，可能是网络问题或服务器内部错误
            error!("UPLOAD FAILED (Step 6.2): gRPC call failed. Error: {}", e);
            Err(format!("gRPC 调用失败: {}", e.message()))
        }
    }
}
