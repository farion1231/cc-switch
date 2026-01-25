//! 格式转换器 Trait 定义
//!
//! 定义通用的格式转换器接口

use super::format::ApiFormat;
use crate::proxy::error::ProxyError;
use bytes::Bytes;
use futures::stream::Stream;
use serde_json::Value;
use std::pin::Pin;

/// 格式转换器 Trait
pub trait FormatTransformer: Send + Sync {
    /// 转换器名称（用于日志）
    #[allow(dead_code)]
    fn name(&self) -> &'static str;

    /// 源格式
    fn source_format(&self) -> ApiFormat;

    /// 目标格式
    fn target_format(&self) -> ApiFormat;

    /// 转换请求体
    fn transform_request(&self, body: Value) -> Result<Value, ProxyError>;

    /// 转换非流式响应体
    fn transform_response(&self, body: Value) -> Result<Value, ProxyError>;

    /// 转换流式响应
    fn transform_stream(
        &self,
        stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    ) -> Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

    /// 获取转换后的端点路径
    fn transform_endpoint(&self, endpoint: &str) -> String {
        endpoint.to_string()
    }
}

/// 双向转换器 Trait（可选实现）
#[allow(dead_code)]
pub trait BidirectionalTransformer: FormatTransformer {
    /// 获取反向转换器
    fn reverse(&self) -> Box<dyn FormatTransformer>;
}
