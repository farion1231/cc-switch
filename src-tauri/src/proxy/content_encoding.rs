//! HTTP content-encoding helpers.

use axum::http::HeaderMap;
use std::io::Read;

/// Decompress body bytes according to a single content-encoding value.
pub(crate) fn decompress_body(
    content_encoding: &str,
    body: &[u8],
) -> Result<Vec<u8>, std::io::Error> {
    match content_encoding {
        "gzip" | "x-gzip" => {
            let mut decoder = flate2::read::GzDecoder::new(body);
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed)?;
            Ok(decompressed)
        }
        "deflate" => {
            let mut decoder = flate2::read::DeflateDecoder::new(body);
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed)?;
            Ok(decompressed)
        }
        "br" => {
            let mut decompressed = Vec::new();
            brotli::BrotliDecompress(&mut std::io::Cursor::new(body), &mut decompressed)?;
            Ok(decompressed)
        }
        "zstd" | "zst" => zstd::stream::decode_all(std::io::Cursor::new(body)),
        _ => {
            log::warn!("未知的 content-encoding: {content_encoding}，跳过解压");
            Ok(body.to_vec())
        }
    }
}

pub(crate) fn is_supported_content_encoding(content_encoding: &str) -> bool {
    matches!(
        content_encoding,
        "gzip" | "x-gzip" | "deflate" | "br" | "zstd" | "zst"
    )
}

/// Extract content-encoding, ignoring identity and empty values.
pub(crate) fn get_content_encoding(headers: &HeaderMap) -> Option<String> {
    headers
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty() && s != "identity")
}
