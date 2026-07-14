use std::collections::HashMap;

pub fn has_proxy_environment(env: &HashMap<String, String>) -> bool {
    [
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "ALL_PROXY",
        "https_proxy",
        "http_proxy",
        "all_proxy",
    ]
    .into_iter()
    .any(|name| env.get(name).is_some_and(|value| !value.is_empty()))
}

pub fn detect_system_proxy() -> Option<String> {
    platform_system_proxy()
}

fn normalize_proxy_url(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value.contains("://") {
        return Some(value.to_string());
    }
    Some(format!("http://{value}"))
}

fn parse_windows_proxy_server(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    if value.contains('=') {
        for wanted_scheme in ["https", "http"] {
            for entry in value.split(';').map(str::trim) {
                let Some((scheme, proxy)) = entry.split_once('=') else {
                    continue;
                };
                if scheme.eq_ignore_ascii_case(wanted_scheme) {
                    return normalize_proxy_url(proxy);
                }
            }
        }
        return None;
    }

    normalize_proxy_url(value)
}

#[cfg(any(test, target_os = "macos"))]
fn parse_macos_scutil_proxy(output: &str) -> Option<String> {
    let mut values = HashMap::new();
    for line in output.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        values.insert(key.trim(), value.trim());
    }

    for (enable_key, host_key, port_key) in [
        ("HTTPEnable", "HTTPProxy", "HTTPPort"),
        ("HTTPSEnable", "HTTPSProxy", "HTTPSPort"),
    ] {
        if values.get(enable_key) != Some(&"1") {
            continue;
        }
        let host = values.get(host_key).copied().unwrap_or_default();
        let port = values.get(port_key).copied().unwrap_or_default();
        if !host.is_empty() && !port.is_empty() {
            return normalize_proxy_url(&format!("{host}:{port}"));
        }
    }

    None
}

#[cfg(windows)]
fn platform_system_proxy() -> Option<String> {
    windows_system_proxy()
}

#[cfg(target_os = "macos")]
fn platform_system_proxy() -> Option<String> {
    let output = std::process::Command::new("scutil")
        .arg("--proxy")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_macos_scutil_proxy(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn platform_system_proxy() -> Option<String> {
    None
}

#[cfg(windows)]
fn windows_system_proxy() -> Option<String> {
    use std::ffi::{OsStr, OsString};
    use std::iter::once;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_ROUTINE_FLAGS, RRF_RT_REG_DWORD, RRF_RT_REG_EXPAND_SZ,
        RRF_RT_REG_SZ, RegGetValueW,
    };
    use windows::core::PCWSTR;

    const INTERNET_SETTINGS: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

    fn wide_null(value: impl AsRef<OsStr>) -> Vec<u16> {
        value.as_ref().encode_wide().chain(once(0)).collect()
    }

    fn read_dword(subkey: &str, name: &str) -> Option<u32> {
        let subkey = wide_null(subkey);
        let name = wide_null(name);
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_DWORD,
                None,
                Some((&mut value as *mut u32).cast()),
                Some(&mut size),
            )
        }
        .ok()
        .ok()?;
        Some(value)
    }

    fn read_string(subkey: &str, name: &str) -> Option<String> {
        let subkey = wide_null(subkey);
        let name = wide_null(name);
        let flags = REG_ROUTINE_FLAGS(RRF_RT_REG_SZ.0 | RRF_RT_REG_EXPAND_SZ.0);
        let mut size = 0u32;
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                PCWSTR(name.as_ptr()),
                flags,
                None,
                None,
                Some(&mut size),
            )
        }
        .ok()
        .ok()?;
        if size == 0 {
            return None;
        }

        let mut value = vec![0u16; (size as usize).div_ceil(2)];
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                PCWSTR(name.as_ptr()),
                flags,
                None,
                Some(value.as_mut_ptr().cast()),
                Some(&mut size),
            )
        }
        .ok()
        .ok()?;
        let len = value.iter().position(|ch| *ch == 0).unwrap_or(value.len());
        Some(
            OsString::from_wide(&value[..len])
                .to_string_lossy()
                .to_string(),
        )
    }

    if read_dword(INTERNET_SETTINGS, "ProxyEnable")? == 0 {
        return None;
    }

    parse_windows_proxy_server(&read_string(INTERNET_SETTINGS, "ProxyServer")?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_proxy_server_prefers_https_scheme_entry() {
        assert_eq!(
            parse_windows_proxy_server(
                "http=proxy.example.test:8080;https=secure-proxy.example.test:8443"
            ),
            Some("http://secure-proxy.example.test:8443".to_string())
        );
    }

    #[test]
    fn windows_proxy_server_prefixes_plain_host() {
        assert_eq!(
            parse_windows_proxy_server("proxy.example.test:8080"),
            Some("http://proxy.example.test:8080".to_string())
        );
    }

    #[test]
    fn macos_scutil_proxy_parses_enabled_http_proxy() {
        let output = r#"
<dictionary> {
  HTTPEnable : 1
  HTTPPort : 8080
  HTTPProxy : proxy.example.test
  HTTPSEnable : 0
}
"#;

        assert_eq!(
            parse_macos_scutil_proxy(output),
            Some("http://proxy.example.test:8080".to_string())
        );
    }

    #[test]
    fn macos_scutil_proxy_ignores_disabled_proxy() {
        let output = r#"
<dictionary> {
  HTTPEnable : 0
  HTTPPort : 8080
  HTTPProxy : proxy.example.test
}
"#;

        assert_eq!(parse_macos_scutil_proxy(output), None);
    }
}
