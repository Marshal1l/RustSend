// src/server_starter.rs

use crate::server;
use log::{error, info};
use std::fs;
use std::path::{Path, PathBuf};
use tonic::transport::Server;
// --- Configuration Constants ---
const BASE_UPLOAD_DIR: &str = "./data";
const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:50051";

/// 初始化并启动 gRPC 文件服务，在后台运行。
pub async fn start_background_server() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = DEFAULT_LISTEN_ADDR.parse()?;

    // 确保根上传目录存在
    let base_dir = Path::new(BASE_UPLOAD_DIR);
    if !base_dir.exists() {
        // 使用 tokio::fs 异步创建目录
        tokio::fs::create_dir_all(base_dir).await.map_err(|e| {
            error!("Failed to create base upload directory: {}", e);
            e
        })?;
        info!("Created base upload directory: {}", BASE_UPLOAD_DIR);
    }

    // 实例化 gRPC 服务实现
    let file_service = server::MyFileService::new(base_dir.into());

    info!("gRPC File service is listening on: {}", DEFAULT_LISTEN_ADDR);

    // 启动 gRPC Server
    Server::builder()
        .add_service(server::filerpc::file_service_server::FileServiceServer::new(file_service))
        .serve(addr)
        .await?;

    Ok(())
}
