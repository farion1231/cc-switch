//! HTTP content-encoding 工具。
//!
//! reqwest 的自动解压已禁用（为了透传 accept-encoding），需要手动解压。
//! 请求侧（如 Codex Desktop 在登录态发压缩请求体）与响应侧（上游压缩响应体）
//! 共用同一套解压逻辑。

use axum::http::header::HeaderMap;
use std::io::Read;

/// 把 content-encoding 值拆成有序 coding 列表（去掉 identity 与空值）。
///
/// HTTP 允许堆叠编码（如 `gzip, zstd`），各 coding 以逗号分隔；亦允许重复
/// content-encoding 头，语义等同逗号拼接（见 [`get_content_encoding`]）。
fn split_codings(content_encoding: &str) -> Vec<&str> {
    content_encoding
        .split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty() && *c != "identity")
        .collect()
}

/// 单个 coding 是否可被解压。
fn is_single_supported(coding: &str) -> bool {
    matches!(
        coding,
        "gzip" | "x-gzip" | "deflate" | "br" | "zstd" | "zst"
    )
}

/// 解压失败原因。把「输出超预算」与「数据损坏」区分开：前者是安全拒绝信号，
/// 响应侧调用方应据此拒绝响应（502），而不是当成普通解压失败静默回退。
#[derive(Debug)]
pub(crate) enum DecompressError {
    /// 底层解码失败（数据损坏 / 格式不符）。
    Io(std::io::Error),
    /// 解压输出超过 `limit` 字节即中止；此时真实输出大小未知，只会大于 limit。
    TooLarge { limit: usize },
}

impl std::fmt::Display for DecompressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::TooLarge { limit } => write!(f, "解压输出超过上限 {limit} 字节"),
        }
    }
}

impl std::error::Error for DecompressError {}

impl From<std::io::Error> for DecompressError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<DecompressError> for std::io::Error {
    fn from(e: DecompressError) -> Self {
        match e {
            DecompressError::Io(e) => e,
            DecompressError::TooLarge { limit } => {
                std::io::Error::other(format!("decompressed body exceeds {limit} bytes"))
            }
        }
    }
}

/// 从解码器读取解压输出，最多 `max_bytes`；一旦输出超过预算立即中止读取并返回
/// [`DecompressError::TooLarge`] —— 压缩炸弹在预算耗尽处被截停，而不是先在内存里
/// 完整展开再比较大小。
fn read_with_output_limit<R: Read>(
    reader: R,
    max_bytes: usize,
) -> Result<Vec<u8>, DecompressError> {
    // saturating_add：无界调用（max_bytes = usize::MAX）时预算保持 usize::MAX
    let budget = max_bytes.saturating_add(1) as u64;
    let mut limited = reader.take(budget);
    let mut out = Vec::new();
    limited.read_to_end(&mut out)?;
    if out.len() > max_bytes {
        return Err(DecompressError::TooLarge { limit: max_bytes });
    }
    Ok(out)
}

/// 解压单个 content-coding，输出上限 `max_output_bytes`。未知编码返回 `Ok(None)`。
fn decompress_single(
    coding: &str,
    body: &[u8],
    max_output_bytes: usize,
) -> Result<Option<Vec<u8>>, DecompressError> {
    match coding {
        "gzip" | "x-gzip" => {
            let decoder = flate2::read::GzDecoder::new(body);
            Ok(Some(read_with_output_limit(decoder, max_output_bytes)?))
        }
        "deflate" => {
            // RFC 9110: deflate 指 zlib 包裹格式；但部分上游 / 客户端发 raw deflate 流。
            // 先按规范尝试 zlib，失败再回退 raw —— 否则合规来源必然解压失败，
            // 原始压缩字节会被 fail-open 透传给 JSON 解析（#2234 形态 C 之一）。
            let zlib = flate2::read::ZlibDecoder::new(body);
            match read_with_output_limit(zlib, max_output_bytes) {
                Ok(decompressed) => Ok(Some(decompressed)),
                Err(zlib_err) => {
                    // TooLarge 也要回退：raw 流被误判为 zlib 时可能在预算处截停，
                    // 回退后若真是炸弹，raw 解码同样会触发 TooLarge。
                    log::debug!("deflate 按 zlib 解压失败（{zlib_err}），回退 raw deflate");
                    let raw = flate2::read::DeflateDecoder::new(body);
                    Ok(Some(read_with_output_limit(raw, max_output_bytes)?))
                }
            }
        }
        "br" => {
            let decoder = brotli::Decompressor::new(std::io::Cursor::new(body), 4096);
            Ok(Some(read_with_output_limit(decoder, max_output_bytes)?))
        }
        "zstd" | "zst" => {
            // Codex 登录态对请求体启用 zstd（Compression::Zstd）；上游也可能 zstd 压缩响应。
            let decoder = zstd::stream::read::Decoder::new(std::io::Cursor::new(body))?;
            Ok(Some(read_with_output_limit(decoder, max_output_bytes)?))
        }
        _ => Ok(None),
    }
}

/// 根据 content-encoding 解压 body 字节，支持堆叠编码（如 `gzip, zstd`），
/// 且每个 coding 的解压输出（含堆叠编码的中间产物）都受 `max_output_bytes`
/// 限制，超限即中止并返回 [`DecompressError::TooLarge`]，用于防御响应侧压缩炸弹。
///
/// RFC 9110 §8.4：codings 按**应用顺序**列出，故解压须**反向**（最后应用的先解）。
/// 返回 `Ok(None)` 表示存在不受支持的编码、原样透传——此时调用方必须保留
/// content-encoding 头，否则下游（诊断 / 客户端）会把压缩字节误当明文。
pub(crate) fn decompress_body_with_limit(
    content_encoding: &str,
    body: &[u8],
    max_output_bytes: usize,
) -> Result<Option<Vec<u8>>, DecompressError> {
    let codings = split_codings(content_encoding);
    if codings.is_empty() {
        return Ok(None);
    }
    // 任一 coding 不支持就整体放弃解压、保头透传，避免半解码的脏数据。
    if !codings.iter().all(|c| is_single_supported(c)) {
        log::warn!("不支持的 content-encoding: {content_encoding}，跳过解压");
        return Ok(None);
    }

    // 反向解码：列表末尾是最后应用的编码，须最先解。
    let mut data: Option<Vec<u8>> = None;
    for coding in codings.iter().rev() {
        let input = data.as_deref().unwrap_or(body);
        match decompress_single(coding, input, max_output_bytes)? {
            Some(decompressed) => data = Some(decompressed),
            // 上面 is_single_supported 已校验，理论不会发生；防御性兜底。
            None => return Ok(None),
        }
    }
    Ok(data)
}

/// 无输出上限的 [`decompress_body_with_limit`] 版本，供请求侧等已有自身
/// 体积约束的调用方使用。
pub(crate) fn decompress_body(
    content_encoding: &str,
    body: &[u8],
) -> Result<Option<Vec<u8>>, std::io::Error> {
    decompress_body_with_limit(content_encoding, body, usize::MAX).map_err(Into::into)
}

/// 该 content-encoding（含堆叠，如 `gzip, zstd`）是否全部可被解压。
///
/// 请求侧用它做闸门：无法解压的压缩体不能透传给 JSON 解析，需直接拒绝。
pub(crate) fn is_supported_content_encoding(content_encoding: &str) -> bool {
    let codings = split_codings(content_encoding);
    !codings.is_empty() && codings.iter().all(|c| is_single_supported(c))
}

/// 从 header 提取 content-encoding（合并重复头，忽略 identity 与空值）。
///
/// HTTP 允许重复 content-encoding 头，语义等同逗号拼接，故用 `get_all` 合并；
/// 返回值可能含多个逗号分隔的 coding，交由 [`decompress_body`] 反向解码。
pub(crate) fn get_content_encoding(headers: &HeaderMap) -> Option<String> {
    let combined = headers
        .get_all("content-encoding")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
        .to_lowercase();
    if split_codings(&combined).is_empty() {
        return None;
    }
    Some(combined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn decompress_body_deflate_handles_zlib_wrapped_per_rfc9110() {
        // RFC 9110 规范的 deflate = zlib 包裹格式（合规来源发的就是这个）
        let payload = br#"{"ok":true}"#;
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, payload).unwrap();
        let compressed = encoder.finish().unwrap();

        let decompressed = decompress_body("deflate", &compressed).unwrap().unwrap();
        assert_eq!(decompressed, payload);
    }

    #[test]
    fn decompress_body_deflate_falls_back_to_raw_stream() {
        // 部分来源违规发 raw deflate 流，保持兼容
        let payload = br#"{"ok":true}"#;
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, payload).unwrap();
        let compressed = encoder.finish().unwrap();

        let decompressed = decompress_body("deflate", &compressed).unwrap().unwrap();
        assert_eq!(decompressed, payload);
    }

    #[test]
    fn decompress_body_zstd_roundtrip() {
        // Codex 登录态发的就是 zstd 压缩请求体
        let payload = br#"{"hello":"world","n":42}"#;
        let compressed = zstd::stream::encode_all(std::io::Cursor::new(&payload[..]), 0).unwrap();
        let decompressed = decompress_body("zstd", &compressed).unwrap().unwrap();
        assert_eq!(decompressed, payload);
    }

    #[test]
    fn decompress_body_stacked_gzip_then_zstd_decodes_in_reverse() {
        // Content-Encoding: gzip, zstd 表示先 gzip 后 zstd，解压须反向（先 zstd 后 gzip）
        let payload = br#"{"stacked":true}"#;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gz, payload).unwrap();
        let gzipped = gz.finish().unwrap();
        let stacked = zstd::stream::encode_all(std::io::Cursor::new(&gzipped[..]), 0).unwrap();

        let decompressed = decompress_body("gzip, zstd", &stacked).unwrap().unwrap();
        assert_eq!(decompressed, payload);
    }

    #[test]
    fn decompress_body_stacked_with_unsupported_returns_none() {
        // 堆叠里只要有一个不支持，就整体保头透传
        let result = decompress_body("snappy, zstd", b"\x00\x01\x02\x03").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn decompress_body_unknown_encoding_returns_none_to_keep_headers() {
        // 未知编码必须返回 None（而非伪装成"已解码"），否则 content-encoding
        // 头被剥掉，下游诊断会把压缩字节误报成明文
        let result = decompress_body("snappy", b"\x00\x01\x02\x03").unwrap();
        assert!(result.is_none());
    }

    /// 生成确定性伪随机字节（LCG），避免测试引入 rand 依赖。
    fn pseudo_random_bytes(len: usize) -> Vec<u8> {
        let mut state: u64 = 0x243F_6A88_85A3_08D3;
        (0..len)
            .map(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (state >> 33) as u8
            })
            .collect()
    }

    fn gzip_compress(payload: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, payload).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn decompress_body_with_limit_passes_payload_under_limit() {
        let payload = br#"{"ok":true}"#;
        let compressed = gzip_compress(payload);

        let out = decompress_body_with_limit("gzip", &compressed, 1024)
            .unwrap()
            .unwrap();
        assert_eq!(out, payload);
    }

    #[test]
    fn decompress_body_with_limit_allows_exactly_limit_bytes() {
        let payload = vec![7u8; 64 * 1024];
        let compressed = gzip_compress(&payload);

        let out = decompress_body_with_limit("gzip", &compressed, 64 * 1024)
            .unwrap()
            .unwrap();
        assert_eq!(out.len(), 64 * 1024);
        assert_eq!(out, payload);
    }

    #[test]
    fn decompress_body_with_limit_aborts_gzip_bomb_mid_stream() {
        // 4 MiB 伪随机数据（压缩率约 1:1）gzip 后截断到 2 MiB：流在产出约 2 MiB
        // 解压数据后 abrupt 结束。有界读取应在 1 MiB 预算耗尽处报 TooLarge；
        // 无界读取会一路读到残缺的流尾报 UnexpectedEof（Io）——两者可区分，
        // 因此该测试能识别"先完整展开再比较"的退化。
        let payload = pseudo_random_bytes(4 * 1024 * 1024);
        let compressed = gzip_compress(&payload);
        assert!(compressed.len() > 2 * 1024 * 1024);
        let truncated = &compressed[..2 * 1024 * 1024];

        let result = decompress_body_with_limit("gzip", truncated, 1024 * 1024);
        assert!(
            matches!(result, Err(DecompressError::TooLarge { .. })),
            "应在预算耗尽处截停（TooLarge），而不是读到流尾才报错: {:?}",
            result.as_ref().map(|o| o.as_ref().map(Vec::len))
        );
    }

    #[test]
    fn decompress_body_with_limit_rejects_zstd_bomb() {
        // 高压缩比 payload：8 MiB 全零 → zstd 压缩后仅数 KiB，完整展开必然超限
        let payload = vec![0u8; 8 * 1024 * 1024];
        let compressed = zstd::stream::encode_all(std::io::Cursor::new(&payload[..]), 0).unwrap();
        assert!(compressed.len() < 1024 * 1024);

        let result = decompress_body_with_limit("zstd", &compressed, 1024 * 1024);
        assert!(
            matches!(result, Err(DecompressError::TooLarge { .. })),
            "zstd 压缩炸弹应在预算耗尽处截停: {:?}",
            result.as_ref().map(|o| o.as_ref().map(Vec::len))
        );
    }

    #[test]
    fn decompress_body_with_limit_rejects_brotli_bomb() {
        let payload = vec![0u8; 8 * 1024 * 1024];
        let mut compressed = Vec::new();
        {
            let mut writer = brotli::CompressorWriter::new(&mut compressed, 4096, 5, 22);
            std::io::Write::write_all(&mut writer, &payload).unwrap();
        }
        assert!(compressed.len() < 1024 * 1024);

        let result = decompress_body_with_limit("br", &compressed, 1024 * 1024);
        assert!(
            matches!(result, Err(DecompressError::TooLarge { .. })),
            "brotli 压缩炸弹应在预算耗尽处截停: {:?}",
            result.as_ref().map(|o| o.as_ref().map(Vec::len))
        );
    }

    #[test]
    fn decompress_body_with_limit_bounds_intermediate_stage_of_stacked_encodings() {
        // 堆叠编码 gzip, zstd：zstd 先解出 gzip 流（小），gzip 再展开成 8 MiB。
        // 中间产物同样受预算约束，不能只在最后一级设防。
        let payload = vec![0u8; 8 * 1024 * 1024];
        let gzipped = gzip_compress(&payload);
        let stacked = zstd::stream::encode_all(std::io::Cursor::new(&gzipped[..]), 0).unwrap();

        let result = decompress_body_with_limit("gzip, zstd", &stacked, 1024 * 1024);
        assert!(
            matches!(result, Err(DecompressError::TooLarge { .. })),
            "堆叠编码的中间解压产物也应受预算约束: {:?}",
            result.as_ref().map(|o| o.as_ref().map(Vec::len))
        );
    }

    #[test]
    fn is_supported_content_encoding_matches_decompressable() {
        for enc in [
            "gzip",
            "x-gzip",
            "deflate",
            "br",
            "zstd",
            "zst",
            "gzip, zstd",
        ] {
            assert!(is_supported_content_encoding(enc), "{enc} 应受支持");
        }
        for enc in ["identity", "snappy", "compress", "", "gzip, snappy"] {
            assert!(!is_supported_content_encoding(enc), "{enc} 不应受支持");
        }
    }

    #[test]
    fn get_content_encoding_combines_repeated_headers() {
        // 重复的 content-encoding 头等同逗号拼接，须用 get_all 合并
        let mut headers = HeaderMap::new();
        headers.append("content-encoding", HeaderValue::from_static("gzip"));
        headers.append("content-encoding", HeaderValue::from_static("zstd"));
        assert_eq!(
            get_content_encoding(&headers).as_deref(),
            Some("gzip, zstd")
        );
    }

    #[test]
    fn get_content_encoding_ignores_identity_only() {
        let mut headers = HeaderMap::new();
        headers.append("content-encoding", HeaderValue::from_static("identity"));
        assert_eq!(get_content_encoding(&headers), None);
    }
}
