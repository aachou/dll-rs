use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum DllError {
    #[error("网络错误: {0}")]
    Network(String),

    #[error("解析失败: {0}")]
    Parse(String),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("未找到 {0}")]
    NotFound(String),

    #[error("无效的 DLL 文件: {0}")]
    InvalidDll(String),
}
