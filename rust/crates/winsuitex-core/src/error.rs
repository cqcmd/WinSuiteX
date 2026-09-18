//! 统一错误类型与「最佳努力」采集语义。
//!
//! 与 C++ 版的关键差异：C++ 把采集失败**静默吞掉**（`pInit() != 0 → return`，
//! 无任何记录）。本设计同样「不因单项失败而崩溃」，但**必须留下痕迹**：
//! 失败项进入 `CollectResult::diagnostics`，调用方可观测、可提示。

use serde::Serialize;

use crate::model::HardwareInfo;
use crate::model::Volume;

pub type Result<T> = std::result::Result<T, Error>;

/// 全 crate 统一的错误类型。
// 刻意**不**派生 Serialize：`Error::Io` 含 `std::io::Error`（不可序列化），
// 而 JSON 快照只需要 `CollectResult` 里的字符串诊断，不需要错误本体。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("注册表操作失败 [{op}]: {message}")]
    Registry { op: &'static str, message: String },

    /// 注意：字段名不能叫 `source`——thiserror 会把同名域自动当作错误源
    /// （要求实现 `Error`），而这里是「数据源名称」。
    #[error("硬件采集失败 [{source_name}]: {message}")]
    Collect {
        source_name: &'static str,
        message: String,
    },

    #[error("资源（图标/DLL）操作失败: {0}")]
    Resource(String),

    #[error("IO 失败: {0}")]
    Io(#[from] std::io::Error),

    #[error("参数错误: {0}")]
    Args(String),
}

impl Error {
    /// 构造一个采集错误。
    pub fn collect(source: &'static str, message: impl Into<String>) -> Self {
        Self::Collect {
            source_name: source,
            message: message.into(),
        }
    }

    /// 构造一个注册表错误。
    pub fn registry(op: &'static str, message: impl Into<String>) -> Self {
        Self::Registry {
            op,
            message: message.into(),
        }
    }
}

/// 单条采集诊断：哪个源失败了、为什么。
// 只有 Serialize：`source: &'static str` 无法从借用的输入反序列化。
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub source: &'static str,
    pub message: String,
}

/// 采集结果。**永不因硬件采集失败而返回 `Err`**（与 C++ 行为一致），
/// 但 `diagnostics` 非空即表示存在降级。
#[derive(Serialize, Debug, Clone, Default)]
pub struct CollectResult {
    /// 部分成功的硬件信息
    pub info: HardwareInfo,
    /// 分区信息（磁盘页 / 报告用）
    pub volumes: Vec<Volume>,
    /// 失败项
    pub diagnostics: Vec<Diagnostic>,
}

impl CollectResult {
    /// 是否所有源都成功。
    pub fn is_complete(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub fn push_diagnostic(&mut self, source: &'static str, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            source,
            message: message.into(),
        });
    }
}
