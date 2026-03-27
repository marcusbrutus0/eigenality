//! Clean link URL transformation.
//!
//! Strips `.html` extensions from URL paths for clean link generation.

/// Strip `.html` extension from a URL path, producing a clean link.
///
/// - `/about.html` → `/about`
/// - `/posts/my-post.html` → `/posts/my-post`
/// - `/about/index.html` → `/about`
/// - `/index.html` → `/`
/// - `/` → `/`
/// - Non-`.html` paths pass through unchanged.
pub fn to_clean_link(path: &str) -> String {
    // Strip /index.html suffix → parent directory path.
    if let Some(prefix) = path.strip_suffix("/index.html") {
        return if prefix.is_empty() {
            "/".to_string()
        } else {
            prefix.to_string()
        };
    }

    // Strip .html extension.
    if let Some(without_ext) = path.strip_suffix(".html") {
        return without_ext.to_string();
    }

    // Non-.html paths pass through unchanged.
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_html() {
        assert_eq!(to_clean_link("/about.html"), "/about");
    }

    #[test]
    fn test_nested_html() {
        assert_eq!(to_clean_link("/posts/my-post.html"), "/posts/my-post");
    }

    #[test]
    fn test_index_in_directory() {
        assert_eq!(to_clean_link("/about/index.html"), "/about");
    }

    #[test]
    fn test_root_index() {
        assert_eq!(to_clean_link("/index.html"), "/");
    }

    #[test]
    fn test_root_slash() {
        assert_eq!(to_clean_link("/"), "/");
    }

    #[test]
    fn test_non_html_passthrough() {
        assert_eq!(to_clean_link("/style.css"), "/style.css");
    }

    #[test]
    fn test_no_extension_passthrough() {
        assert_eq!(to_clean_link("/about"), "/about");
    }

    #[test]
    fn test_deeply_nested() {
        assert_eq!(to_clean_link("/blog/2024/post.html"), "/blog/2024/post");
    }

    #[test]
    fn test_nested_index() {
        assert_eq!(to_clean_link("/blog/posts/index.html"), "/blog/posts");
    }

    #[test]
    fn test_404_html() {
        assert_eq!(to_clean_link("/404.html"), "/404");
    }
}
