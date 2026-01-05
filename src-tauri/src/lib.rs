// src/main.rs (或您的 Tauri 主入口文件)

use log::{error, info};
// 移除 tokio::runtime::Runtime 引用，因为我们不再手动创建运行时。
// use tokio::runtime::Runtime;

// 引入 ClientState 和 gRPC 命令
use crate::grpc_client::{connect_server, list_remote_dir, upload_local_file, ClientState};
// 引入 Tauri 的专用异步运行时
use tauri::{async_runtime, Emitter}; // <-- 新增!

// 确保您的 server 和 client 模块已被引入
mod grpc_client;
mod server;
mod server_starter;

// 引入 gRPC 结构 (如果未通过 build.rs 引入)
pub mod filerpc {
    tonic::include_proto!("filerpc");
}

// 示例命令：保留 greet (可选)
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    // ----------------------------------------------------------------------
    // 核心修改：使用 tauri::async_runtime::spawn 替换所有 tokio::spawn
    // ----------------------------------------------------------------------

    tauri::Builder::default()
        // 核心：使用 setup hook
        .setup(|app| {
            // 在这里，我们处于 Tauri 内部的 Tokio 运行时环境，可以安全地使用 async_runtime::spawn
            let handle = app.handle().clone();

            // 启动后台 gRPC Server
            async_runtime::spawn(async move {
                // <-- 关键修改：使用 async_runtime::spawn
                if let Err(e) = crate::server_starter::start_background_server().await {
                    error!("Background gRPC server failed: {:?}", e);
                    // 理论上可以在这里发送事件通知前端
                    let _ = handle.emit("server-error", format!("Server failed: {:?}", e));
                }
            });
            info!("Background gRPC server spawned successfully inside Tauri's setup hook.");

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .manage(ClientState::new()) // 客户端状态管理
        .invoke_handler(tauri::generate_handler![
            connect_server,
            list_remote_dir,
            upload_local_file,
            greet
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
