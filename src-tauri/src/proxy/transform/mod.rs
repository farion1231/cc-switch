//! 通用格式转换模块
//!
//! 提供 API 格式之间的双向转换，支持：
//! - Anthropic ↔ OpenAI
//! - Gemini ↔ OpenAI（预留）
//!
//! ## 使用方式
//!
//! ```rust,ignore
//! use crate::proxy::transform::{config::TransformConfig, registry::get_transformer};
//!
//! let config = TransformConfig::from_provider(&provider);
//! if config.needs_transform() {
//!     if let Some(transformer) = get_transformer(config.source_format, config.target_format) {
//!         let transformed = transformer.transform_request(body)?;
//!     }
//! }
//! ```

pub mod anthropic_openai;
pub mod config;
pub mod format;
pub mod registry;
pub mod traits;

// 公开导出
pub use config::TransformConfig;
pub use registry::get_transformer;

// 以下导出供外部模块使用（如需扩展转换器）
#[allow(unused_imports)]
pub use format::ApiFormat;
#[allow(unused_imports)]
pub use registry::TRANSFORMER_REGISTRY;
#[allow(unused_imports)]
pub use traits::{BidirectionalTransformer, FormatTransformer};
