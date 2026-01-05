// src/server_starter.rs

use crate::server;
use log::{error, info};
use std::path::{Path, PathBuf};
use tonic::transport::Server;
// 引入 dirs crate
use dirs;

// --- Configuration Constants ---
// 注意：我们将不再使用 BASE_UPLOAD_DIR，而是使用 dirs::home_dir()

const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:50051";

/// 初始化并启动 gRPC 文件服务，在后台运行。
pub async fn start_background_server() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = DEFAULT_LISTEN_ADDR.parse()?;

    // --- 关键修复：获取用户 Home 目录 ---
    let base_path: PathBuf = match dirs::home_dir() {
        Some(path) => {
            info!("Found user home directory: {}", path.display());
            path
        }
        None => {
            // 如果无法获取 Home 目录，回退到默认的 "./data"
            error!("Could not determine user's home directory. Defaulting to './data'.");
            PathBuf::from("./data")
        }
    };
    // ------------------------------------

    // 确保 base_path 存在。对于 Home 目录，它通常是存在的，但为了安全起见，仍然检查。
    if !base_path.exists() {
        tokio::fs::create_dir_all(&base_path).await.map_err(|e| {
            error!(
                "Failed to create base directory (or default data dir): {}",
                e
            );
            e
        })?;
        info!("Base directory created/verified: {}", base_path.display());
    }

    // 实例化 gRPC 服务实现，将 Home 目录作为根路径
    let file_service = server::MyFileService::new(base_path);

    info!("gRPC File service is listening on: {}", DEFAULT_LISTEN_ADDR);

    // 启动 gRPC Server
    Server::builder()
        .add_service(server::filerpc::file_service_server::FileServiceServer::new(file_service))
        .serve(addr)
        .await?;

    Ok(())
}
