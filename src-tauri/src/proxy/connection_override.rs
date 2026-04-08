use std::net::SocketAddr;
use std::str::FromStr;

pub fn parse_connection_override(value: Option<&str>) -> Result<Option<SocketAddr>, String> {
    let Some(raw) = value.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(None);
    };

    let addr = SocketAddr::from_str(raw).map_err(|_| {
        "Invalid connection override, expected IPv4:port or [IPv6]:port".to_string()
    })?;

    if addr.port() == 0 {
        return Err("Invalid connection override port, expected 1-65535".to_string());
    }

    Ok(Some(addr))
}

#[cfg(test)]
mod tests {
    use super::parse_connection_override;

    #[test]
    fn accepts_empty() {
        assert!(parse_connection_override(None).unwrap().is_none());
        assert!(parse_connection_override(Some(" ")).unwrap().is_none());
    }

    #[test]
    fn accepts_ipv4_and_ipv6() {
        assert_eq!(
            parse_connection_override(Some("1.2.3.4:12345"))
                .unwrap()
                .unwrap()
                .to_string(),
            "1.2.3.4:12345"
        );
        assert_eq!(
            parse_connection_override(Some("[2001:db8::1]:443"))
                .unwrap()
                .unwrap()
                .to_string(),
            "[2001:db8::1]:443"
        );
    }

    #[test]
    fn rejects_invalid_value() {
        assert!(parse_connection_override(Some("example.com:443")).is_err());
        assert!(parse_connection_override(Some("1.2.3.4")).is_err());
        assert!(parse_connection_override(Some("1.2.3.4:0")).is_err());
    }
}
