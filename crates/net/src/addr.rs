pub fn normalize_host_port(raw: &str) -> Result<String, String> {
    let mut s = raw.trim();
    if let Some((_, rest)) = s.split_once("://") {
        s = rest;
    }
    let s = s.trim_end_matches('/').trim().to_string();
    match s.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && port.parse::<u16>().is_ok() => Ok(s),
        Some((_, port)) => Err(format!(
            "\"{raw}\" is not a valid address — expected host:port (\"{port}\" is not a port number)"
        )),
        None => Err(format!(
            "\"{raw}\" is missing a port — use the full host:port shown on the other device, e.g. 192.168.1.20:53124"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_host_port as n;

    #[test]
    fn accepts_common_pasted_forms() {
        assert_eq!(n("192.168.1.20:53124").unwrap(), "192.168.1.20:53124");
        assert_eq!(n("  10.0.0.5:9000 ").unwrap(), "10.0.0.5:9000");
        assert_eq!(n("http://192.168.1.20:53124/").unwrap(), "192.168.1.20:53124");
        assert_eq!(n("ferry://box.local:9000").unwrap(), "box.local:9000");
        assert_eq!(n("[::1]:9000").unwrap(), "[::1]:9000");
    }

    #[test]
    fn rejects_missing_or_bad_port() {
        assert!(n("192.168.1.20").unwrap_err().contains("missing a port"));
        assert!(n("host:abc").unwrap_err().contains("not a port number"));
        assert!(n("host:99999").unwrap_err().contains("not a port number"));
    }
}
