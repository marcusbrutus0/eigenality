/// Extract the origin (scheme + host + optional port) from a URL.
///
/// Example: `https://cms.example.com/api/v1/apps/x` → `https://cms.example.com`
pub fn extract_origin(url: &str) -> Option<String> {
    let scheme_end = url.find("://")?;
    let after_scheme = &url[scheme_end + 3..];
    let authority_end = after_scheme.find('/').unwrap_or(after_scheme.len());
    Some(format!(
        "{}{}",
        &url[..scheme_end + 3],
        &after_scheme[..authority_end]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_origin_https() {
        assert_eq!(
            extract_origin("https://cms.example.com/api/v1/apps/x"),
            Some("https://cms.example.com".to_string()),
        );
    }

    #[test]
    fn extract_origin_with_port() {
        assert_eq!(
            extract_origin("https://cms.example.com:8443/api"),
            Some("https://cms.example.com:8443".to_string()),
        );
    }

    #[test]
    fn extract_origin_http() {
        assert_eq!(
            extract_origin("http://localhost:3000/api"),
            Some("http://localhost:3000".to_string()),
        );
    }

    #[test]
    fn extract_origin_no_path() {
        assert_eq!(
            extract_origin("https://cms.example.com"),
            Some("https://cms.example.com".to_string()),
        );
    }

    #[test]
    fn extract_origin_not_http() {
        assert_eq!(extract_origin("/some/path"), None);
    }

    #[test]
    fn extract_origin_empty() {
        assert_eq!(extract_origin(""), None);
    }
}
