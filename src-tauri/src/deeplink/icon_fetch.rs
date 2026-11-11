//! 深链接 `iconUrl` 的下载。
//!
//! 链接可以来自任意网页，下载地址就等于由对方指定，所以按不可信的服务端请求处理：
//! - 只允许 https、默认端口、域名主机，不收 IP 字面量和带账号密码的地址；
//! - 解析出的地址必须全部是公网地址（拒绝私网、回环、链路本地、CGNAT、ULA 等），
//!   再把连接钉在这些已核对的地址上，防止 DNS 重绑定在核对之后换成内网地址
//!   （fake-ip 网段是例外，见 `is_public_ipv4`）；
//! - 不跟随自动重定向，手动最多跟 3 跳，每一跳重新核对；
//! - 不走系统代理（代理替我们解析域名，核对过的地址就不算数了），不带 Cookie / Referer；
//! - 整个过程 5 秒超时，响应体流式读取，超过 256 KB 立即放弃；
//! - Content-Type 和文件头必须一致且是 PNG / JPEG / WebP / SVG 之一，位图再读出宽高核对。
//!
//! 这里只负责把字节安全地拿回来，返回带核对过类型的 data URI；前端在 canvas 上重新编码成
//! 96×96 PNG 之后才会存进供应商，保存时后端再按自定义图标的规则完整校验一次。
//! 下载失败不报错给用户，确认框退回首字母或链接里的内置图标。

use base64::Engine;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;
use url::{Host, Url};

const MAX_ICON_URL_LENGTH: usize = 2048;
const MAX_ICON_BYTES: usize = 256 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REDIRECTS: usize = 3;
/// 解码前的宽高上限，和前端 `customIcon.ts` 的 `MAX_SOURCE_DIMENSION` 一致
const MAX_SOURCE_DIMENSION: u32 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IconFormat {
    Png,
    Jpeg,
    Webp,
    Svg,
}

impl IconFormat {
    fn from_content_type(value: &str) -> Option<Self> {
        let mime = value.split(';').next()?.trim().to_ascii_lowercase();
        match mime.as_str() {
            "image/png" => Some(Self::Png),
            "image/jpeg" | "image/jpg" => Some(Self::Jpeg),
            "image/webp" => Some(Self::Webp),
            "image/svg+xml" => Some(Self::Svg),
            _ => None,
        }
    }

    fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Svg => "image/svg+xml",
        }
    }
}

/// 链接里的 `iconUrl` 形状是否可用：https、域名主机、默认端口、没有账号密码。
/// 解析阶段用它过滤，下载时每一跳重定向也再过一遍。
pub(crate) fn parse_icon_url(raw: &str) -> Option<Url> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > MAX_ICON_URL_LENGTH {
        return None;
    }
    let url = Url::parse(raw).ok()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    // WHATWG 解析会把 0x7f.1、2130706433 之类的写法规整成 IPv4，这里一并挡掉
    let Some(Host::Domain(domain)) = url.host() else {
        return None;
    };
    let domain = domain.trim_end_matches('.');
    if !domain.contains('.') || domain == "localhost" || domain.ends_with(".localhost") {
        return None;
    }
    Some(url)
}

/// 只放行公网单播地址
fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_ipv4(v4),
        IpAddr::V6(v6) => is_public_ipv6(v6),
    }
}

/// 198.18.0.0/15（基准测试网段）故意放行：Clash、Surge 等工具的 TUN 模式（fake-ip）把所有
/// 域名都解析到这个网段，拒绝它就等于让这些用户永远拿不到图标。
///
/// 代价是 fake-ip 下核对不到代理最终连接的真实地址：链接指向一个用公开证书的内网域名时，
/// 代理会解析到内网地址并完成 TLS 握手，我们会对它发出一次 GET。接受这个代价的理由：
/// 这次请求不带 Cookie 和认证，响应也不会回到链接发起方手里；而用户点链接时正开着
/// 发起方的网页，那个网页用 `<img>` 就能从同一台机器、经同一个 TUN 发出同样的请求，
/// 还带着浏览器的 Cookie（浏览器在 fake-ip 下看到的也是这个网段）。这里没有给对方新的能力。
fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0 // 本网络 0.0.0.0/8
        || a == 10 // 私网
        || a == 127 // 回环
        || (a == 100 && (64..128).contains(&b)) // CGNAT 100.64.0.0/10
        || (a == 169 && b == 254) // 链路本地
        || (a == 172 && (16..32).contains(&b)) // 私网
        || (a == 192 && b == 0 && c == 0) // IETF 协议分配 192.0.0.0/24
        || (a == 192 && b == 0 && c == 2) // 文档
        || (a == 192 && b == 168) // 私网
        || (a == 198 && b == 51 && c == 100) // 文档
        || (a == 203 && b == 0 && c == 113) // 文档
        || a >= 224) // 组播、保留、广播
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    // NAT64 众所周知前缀 64:ff9b::/96：纯 IPv6 网络下访问 IPv4 站点会解析成它，按内嵌地址判断
    if segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return is_public_ipv4(embedded_ipv4(segments[6], segments[7]));
    }
    // 只认全球单播 2000::/3，回环、ULA、链路本地、组播、IPv4 映射地址都不在里面
    if segments[0] & 0xe000 != 0x2000 {
        return false;
    }
    // 2001::/23 是 IETF 协议分配（含 Teredo），2001:db8::/32 是文档地址
    if segments[0] == 0x2001 && (segments[1] < 0x0200 || segments[1] == 0x0db8) {
        return false;
    }
    // 6to4 2002::/16 内嵌 IPv4
    if segments[0] == 0x2002 {
        return is_public_ipv4(embedded_ipv4(segments[1], segments[2]));
    }
    true
}

fn embedded_ipv4(high: u16, low: u16) -> Ipv4Addr {
    let [a, b] = high.to_be_bytes();
    let [c, d] = low.to_be_bytes();
    Ipv4Addr::new(a, b, c, d)
}

/// 按文件头识别格式
fn sniff_format(bytes: &[u8]) -> Option<IconFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(IconFormat::Png);
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(IconFormat::Jpeg);
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(IconFormat::Webp);
    }
    // SVG 是文本：去掉 BOM 和空白后以 `<` 开头（`<?xml`、`<!--`、`<svg`），开头 1 KB 内出现 `<svg`
    let text = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let text = &text[text.iter().take_while(|b| b.is_ascii_whitespace()).count()..];
    if text.first() == Some(&b'<') {
        let head = &text[..text.len().min(1024)];
        if head.windows(4).any(|w| w.eq_ignore_ascii_case(b"<svg")) {
            return Some(IconFormat::Svg);
        }
    }
    None
}

/// 位图从文件头读出宽高；SVG 没有可靠的像素尺寸，交给前端加载后核对
fn read_dimensions(format: IconFormat, bytes: &[u8]) -> Option<(u32, u32)> {
    match format {
        IconFormat::Png => {
            if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
                return None;
            }
            Some((be_u32(&bytes[16..20]), be_u32(&bytes[20..24])))
        }
        IconFormat::Jpeg => jpeg_dimensions(bytes),
        IconFormat::Webp => webp_dimensions(bytes),
        IconFormat::Svg => None,
    }
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut offset = 2;
    while offset + 4 <= bytes.len() {
        if bytes[offset] != 0xff {
            return None;
        }
        let marker = bytes[offset + 1];
        // 填充字节
        if marker == 0xff {
            offset += 1;
            continue;
        }
        // 没有长度字段的标记
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            offset += 2;
            continue;
        }
        let length = u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]]) as usize;
        if length < 2 {
            return None;
        }
        // SOF0–SOF15，排除 DHT(C4)、JPG(C8)、DAC(CC)
        if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            let segment = bytes.get(offset + 4..offset + 9)?;
            let height = u16::from_be_bytes([segment[1], segment[2]]) as u32;
            let width = u16::from_be_bytes([segment[3], segment[4]]) as u32;
            return Some((width, height));
        }
        offset += 2 + length;
    }
    None
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let chunk = bytes.get(12..16)?;
    match chunk {
        // 有损：帧头起始码 9d 01 2a 之后是 14 位宽高
        b"VP8 " => {
            let frame = bytes.get(20..30)?;
            if frame[3..6] != [0x9d, 0x01, 0x2a] {
                return None;
            }
            let width = u16::from_le_bytes([frame[6], frame[7]]) & 0x3fff;
            let height = u16::from_le_bytes([frame[8], frame[9]]) & 0x3fff;
            Some((width as u32, height as u32))
        }
        // 无损：签名 0x2f 之后按位打包的 14 位 (宽-1)、(高-1)
        b"VP8L" => {
            let header = bytes.get(20..25)?;
            if header[0] != 0x2f {
                return None;
            }
            let bits = u32::from_le_bytes([header[1], header[2], header[3], header[4]]);
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        // 扩展：24 位 (宽-1)、(高-1)
        b"VP8X" => {
            let header = bytes.get(24..30)?;
            let width = u32::from_le_bytes([header[0], header[1], header[2], 0]) + 1;
            let height = u32::from_le_bytes([header[3], header[4], header[5], 0]) + 1;
            Some((width, height))
        }
        _ => None,
    }
}

/// 核对下载到的内容，合格就转成 data URI
fn body_to_data_uri(content_type: Option<&str>, bytes: &[u8]) -> Result<String, String> {
    let declared = content_type
        .and_then(IconFormat::from_content_type)
        .ok_or_else(|| format!("unsupported content type {content_type:?}"))?;
    let actual = sniff_format(bytes).ok_or("content is not a supported image")?;
    if declared != actual {
        return Err(format!(
            "content type {} does not match the file ({})",
            declared.mime(),
            actual.mime()
        ));
    }
    match actual {
        IconFormat::Svg => {
            std::str::from_utf8(bytes).map_err(|_| "SVG is not valid UTF-8")?;
        }
        format => {
            let (width, height) =
                read_dimensions(format, bytes).ok_or("cannot read image dimensions")?;
            if width == 0
                || height == 0
                || width > MAX_SOURCE_DIMENSION
                || height > MAX_SOURCE_DIMENSION
            {
                return Err(format!("image is {width}×{height}"));
            }
        }
    }
    Ok(format!(
        "data:{};base64,{}",
        actual.mime(),
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// reqwest 的错误文本自带完整 URL（含查询参数，可能是 CDN 签名或令牌），而重定向之后的
/// 地址也没经过 `url_for_log`。去掉 URL，只留错误类型和底层原因写进日志。
fn describe_request_error(error: reqwest::Error) -> String {
    let error = error.without_url();
    let mut text = error.to_string();
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// 解析域名，要求每个地址都是公网地址。有一个不是就整体拒绝，不挑着用。
async fn resolve_public(url: &Url) -> Result<(String, Vec<SocketAddr>), String> {
    let Some(Host::Domain(domain)) = url.host() else {
        return Err("host is not a domain".to_string());
    };
    let domain = domain.to_string();
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((domain.as_str(), 443))
        .await
        .map_err(|e| format!("DNS lookup failed: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err("DNS returned no addresses".to_string());
    }
    if let Some(addr) = addrs.iter().find(|addr| !is_public_ip(addr.ip())) {
        return Err(format!(
            "{domain} resolves to non-public address {}",
            addr.ip()
        ));
    }
    Ok((domain, addrs))
}

async fn fetch_unbounded(raw_url: &str) -> Result<String, String> {
    let mut url = parse_icon_url(raw_url).ok_or("URL is not an allowed https address")?;
    for _ in 0..=MAX_REDIRECTS {
        let (domain, addrs) = resolve_public(&url).await?;
        // 每一跳单独建客户端：把这个域名钉在刚核对过的地址上
        let client = reqwest::Client::builder()
            .no_proxy()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .resolve_to_addrs(&domain, &addrs)
            .user_agent(concat!("CC-Switch/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| format!("failed to build client: {}", describe_request_error(e)))?;
        let mut response = client
            .get(url.clone())
            .header(
                reqwest::header::ACCEPT,
                "image/png,image/jpeg,image/webp,image/svg+xml",
            )
            .send()
            .await
            .map_err(|e| format!("request failed: {}", describe_request_error(e)))?;

        let status = response.status();
        if status.is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or("redirect without a Location")?;
            let next = url
                .join(location)
                .map_err(|_| "redirect Location is not a URL")?;
            url = parse_icon_url(next.as_str()).ok_or("redirect target is not allowed")?;
            continue;
        }
        if !status.is_success() {
            return Err(format!("HTTP {status}"));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_ICON_BYTES as u64)
        {
            return Err("response is larger than 256 KB".to_string());
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("failed to read body: {}", describe_request_error(e)))?
        {
            if body.len() + chunk.len() > MAX_ICON_BYTES {
                return Err("response is larger than 256 KB".to_string());
            }
            body.extend_from_slice(&chunk);
        }
        return body_to_data_uri(content_type.as_deref(), &body);
    }
    Err("too many redirects".to_string())
}

/// 下载链接里的图标，返回带核对过类型的 data URI。失败原因只进日志。
pub async fn fetch_deeplink_icon(raw_url: &str) -> Option<String> {
    let result = match tokio::time::timeout(FETCH_TIMEOUT, fetch_unbounded(raw_url)).await {
        Ok(result) => result,
        Err(_) => Err("timed out".to_string()),
    };
    match result {
        Ok(data_uri) => Some(data_uri),
        Err(reason) => {
            log::info!(
                "Skipped deep link icon {}: {reason}",
                crate::url_for_log(raw_url)
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_url_shape_rules() {
        for ok in [
            "https://example.com/logo.png",
            "https://cdn.example.com/a/b.svg?v=2",
            " https://example.com/logo.png ",
        ] {
            assert!(parse_icon_url(ok).is_some(), "{ok} should be accepted");
        }
        for bad in [
            "http://example.com/logo.png",
            "https://127.0.0.1/logo.png",
            "https://2130706433/logo.png",
            "https://0x7f.1/logo.png",
            "https://[::1]/logo.png",
            "https://localhost/logo.png",
            "https://foo.localhost/logo.png",
            "https://intranet/logo.png",
            "https://user:pass@example.com/logo.png",
            "https://example.com:8443/logo.png",
            "data:image/png;base64,AAAA",
            "file:///etc/passwd",
            "",
        ] {
            assert!(parse_icon_url(bad).is_none(), "{bad} should be rejected");
        }
        let long = format!("https://example.com/{}", "a".repeat(MAX_ICON_URL_LENGTH));
        assert!(parse_icon_url(&long).is_none());
    }

    #[test]
    fn only_public_addresses_pass() {
        for ip in [
            "0.0.0.0",
            "10.1.2.3",
            "127.0.0.1",
            "100.64.0.1",
            "100.127.255.255",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.255",
            "192.0.0.8",
            "192.168.1.1",
            "224.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:127.0.0.1",
            "::ffff:8.8.8.8",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "2001::1",
            "2002:7f00:1::",
            "2002:c0a8:101::",
            "64:ff9b::a00:1",
        ] {
            let ip: IpAddr = ip.parse().unwrap();
            assert!(!is_public_ip(ip), "{ip} should be rejected");
        }
        for ip in [
            "8.8.8.8",
            "1.1.1.1",
            "100.63.255.255",
            "100.128.0.1",
            "172.15.0.1",
            "172.32.0.1",
            "198.18.0.15",
            "198.19.255.255",
            "2606:4700::1111",
            "2400:cb00::1",
            "2002:808:808::",
            "64:ff9b::808:808",
        ] {
            let ip: IpAddr = ip.parse().unwrap();
            assert!(is_public_ip(ip), "{ip} should be accepted");
        }
    }

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    fn jpeg_bytes(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xd8];
        // APP0 段，长度 16
        bytes.extend_from_slice(&[0xff, 0xe0, 0x00, 0x10]);
        bytes.extend_from_slice(&[0; 14]);
        // SOF0
        bytes.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 0x08]);
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&[3; 10]);
        bytes
    }

    fn webp_vp8x(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\0\0\0\0".to_vec();
        bytes.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
        bytes.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
        bytes
    }

    fn webp_vp8l(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WEBPVP8L\0\0\0\0\x2f".to_vec();
        let bits = (width - 1) | ((height - 1) << 14);
        bytes.extend_from_slice(&bits.to_le_bytes());
        bytes
    }

    fn webp_vp8(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WEBPVP8 \0\0\0\0\0\0\0\x9d\x01\x2a".to_vec();
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes
    }

    #[test]
    fn reads_dimensions_of_each_bitmap_format() {
        assert_eq!(
            read_dimensions(IconFormat::Png, &png_bytes(64, 32)),
            Some((64, 32))
        );
        assert_eq!(
            read_dimensions(IconFormat::Jpeg, &jpeg_bytes(300, 200)),
            Some((300, 200))
        );
        assert_eq!(
            read_dimensions(IconFormat::Webp, &webp_vp8x(5000, 10)),
            Some((5000, 10))
        );
        assert_eq!(
            read_dimensions(IconFormat::Webp, &webp_vp8l(128, 96)),
            Some((128, 96))
        );
        assert_eq!(
            read_dimensions(IconFormat::Webp, &webp_vp8(48, 40)),
            Some((48, 40))
        );
    }

    #[test]
    fn body_must_match_its_content_type_and_size_limits() {
        let png = png_bytes(64, 64);
        let uri = body_to_data_uri(Some("image/png; charset=binary"), &png).unwrap();
        assert!(uri.starts_with("data:image/png;base64,"));

        // 类型和内容对不上
        assert!(body_to_data_uri(Some("image/jpeg"), &png).is_err());
        assert!(body_to_data_uri(Some("application/octet-stream"), &png).is_err());
        assert!(body_to_data_uri(None, &png).is_err());
        // 声明 PNG、实际是 GIF
        assert!(body_to_data_uri(Some("image/png"), b"GIF89a\x00\x02\x00\x02").is_err());
        // 体积小、宽高巨大
        assert!(body_to_data_uri(Some("image/png"), &png_bytes(50_000, 50_000)).is_err());
        assert!(body_to_data_uri(Some("image/png"), &png_bytes(0, 10)).is_err());
        assert!(body_to_data_uri(Some("image/webp"), &webp_vp8x(5000, 10)).is_err());
        assert!(body_to_data_uri(Some("image/jpeg"), &jpeg_bytes(300, 200)).is_ok());
        // 文件头被截断
        assert!(body_to_data_uri(Some("image/png"), &png[..20]).is_err());
    }

    #[test]
    fn svg_is_recognized_only_as_svg_text() {
        let svg =
            b"\xef\xbb\xbf\n<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>";
        assert!(body_to_data_uri(Some("image/svg+xml"), svg)
            .unwrap()
            .starts_with("data:image/svg+xml;base64,"));
        assert!(body_to_data_uri(Some("image/svg+xml"), b"<html><body>hi</body></html>").is_err());
        assert!(body_to_data_uri(Some("text/html"), b"<svg/>").is_err());
        assert!(body_to_data_uri(Some("image/svg+xml"), b"<svg \xff\xfe/>").is_err());
    }

    #[tokio::test]
    async fn request_errors_leave_the_url_out_of_the_log() {
        // 连本机一个没人监听的端口，拿到一个带 URL 的真实 reqwest 错误
        let error = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get("http://127.0.0.1:9/logo.png?token=SECRET_TOKEN")
            .send()
            .await
            .expect_err("nothing listens on port 9");
        assert!(error.to_string().contains("SECRET_TOKEN"));

        let described = describe_request_error(error);
        assert!(!described.contains("SECRET_TOKEN"), "{described}");
        assert!(!described.contains("127.0.0.1"), "{described}");
    }

    #[tokio::test]
    async fn refuses_disallowed_urls_without_network() {
        assert_eq!(fetch_deeplink_icon("http://example.com/a.png").await, None);
        assert_eq!(fetch_deeplink_icon("https://127.0.0.1/a.png").await, None);
        assert_eq!(fetch_deeplink_icon("https://localhost/a.png").await, None);
    }
}
