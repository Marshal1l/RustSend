fn main() -> Result<(), Box<dyn std::error::Error>> {
    tauri_build::build();
    tonic_prost_build::configure()
        // 告诉构建器生成服务端代码
        .build_server(true)
        // 告诉构建器生成客户端代码
        .build_client(true)
        // 编译 proto 文件
        // 1. &["proto/file_rpc.proto"]: 要编译的 proto 文件列表
        // 2. &["proto"]: 查找 proto 文件时要搜索的目录（包含文件依赖）
        .compile_protos(&["proto/file_rpc.proto"], &["proto"])?;
    Ok(())
}
