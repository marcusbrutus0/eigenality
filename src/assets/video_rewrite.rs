//! Video HTML rewriting: collect video elements, optimize, and rewrite.
//!
//! Uses `scraper` for DOM-based collection (to inspect parent-child
//! relationships like `<video>` → `<source>`) and `lol_html` for
//! streaming single-pass rewrite of the final HTML.

use std::collections::HashMap;

use super::videos::{is_excluded, VideoVariants};

/// Mapping from original video URL → generated variants.
pub type VideoVariantMap = HashMap<String, VideoVariants>;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// A single `<video>` element found in the HTML during collection.
#[derive(Debug, Clone)]
pub struct VideoEntry {
    /// The video source URL (from `<video src>` or first `<source src>`).
    pub src: String,
    /// `true` when the URL came from `<video src="...">` directly (Form 1).
    /// `false` when it came from a child `<source>` element (Form 2).
    pub is_form1: bool,
}

// ---------------------------------------------------------------------------
// Collection (scraper-based)
// ---------------------------------------------------------------------------

/// Returns `true` for URLs that point to external or data resources.
pub fn should_skip_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("data:")
}

/// Collect video entries from HTML in document order.
///
/// For each `<video>` element:
/// - Skip if `data-no-optimize` attribute is present.
/// - Form 1: `<video src="...">` — record src with `is_form1 = true`.
/// - Form 2: no `src` attr, first `<source src="...">` child — record with
///   `is_form1 = false`.
/// - Skip external URLs and paths matching exclude glob patterns.
pub fn collect_video_entries(html: &str, exclude_patterns: &[String]) -> Vec<VideoEntry> {
    let document = scraper::Html::parse_document(html);
    let video_sel = scraper::Selector::parse("video").expect("valid selector");
    let source_sel = scraper::Selector::parse("source").expect("valid selector");

    let mut entries = Vec::new();

    for video_el in document.select(&video_sel) {
        // Skip videos marked with data-no-optimize.
        if video_el.value().attr("data-no-optimize").is_some() {
            continue;
        }

        // Form 1: <video src="...">
        if let Some(src) = video_el.value().attr("src") {
            if should_skip_url(src) {
                continue;
            }
            let check_path = src.trim_start_matches('/');
            if is_excluded(check_path, exclude_patterns) {
                continue;
            }
            entries.push(VideoEntry {
                src: src.to_string(),
                is_form1: true,
            });
            continue;
        }

        // Form 2: first <source> child with src.
        let first_source = video_el.select(&source_sel).next();
        if let Some(source_el) = first_source {
            if let Some(src) = source_el.value().attr("src") {
                if should_skip_url(src) {
                    continue;
                }
                let check_path = src.trim_start_matches('/');
                if is_excluded(check_path, exclude_patterns) {
                    continue;
                }
                entries.push(VideoEntry {
                    src: src.to_string(),
                    is_form1: false,
                });
            }
        }
    }

    entries
}

// ---------------------------------------------------------------------------
// Source HTML builder
// ---------------------------------------------------------------------------

/// Build `<source>` element HTML for a set of video variants.
///
/// VP9 sources appear first (highest resolution first), followed by the
/// original as a fallback.
pub fn build_sources_html(variants: &VideoVariants) -> String {
    let mut html = String::new();

    // VP9 sources (highest resolution first — already sorted descending).
    for v in &variants.vp9 {
        html.push_str(&format!(
            "<source src=\"{}\" type=\"video/webm; codecs=&quot;vp9&quot;\">",
            v.url_path,
        ));
    }

    // Original fallback.
    html.push_str(&format!(
        "<source src=\"{}\" type=\"{}\">",
        variants.original.url_path, variants.original.mime_type,
    ));

    html
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_video_entries_form1() {
        let html = r#"<html><body><video src="/assets/clip.mp4" controls></video></body></html>"#;
        let entries = collect_video_entries(html, &[]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].src, "/assets/clip.mp4");
        assert!(entries[0].is_form1);
    }

    #[test]
    fn test_collect_video_entries_form2() {
        let html = r#"<html><body><video controls><source src="/assets/clip.mp4" type="video/mp4"></video></body></html>"#;
        let entries = collect_video_entries(html, &[]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].src, "/assets/clip.mp4");
        assert!(!entries[0].is_form1);
    }

    #[test]
    fn test_collect_video_entries_skips_external() {
        let html =
            r#"<html><body><video src="https://cdn.example.com/video.mp4"></video></body></html>"#;
        let entries = collect_video_entries(html, &[]);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_collect_video_entries_skips_data_no_optimize() {
        let html = r#"<html><body><video src="/assets/clip.mp4" data-no-optimize></video></body></html>"#;
        let entries = collect_video_entries(html, &[]);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_collect_video_entries_multiple_form2() {
        let html = r#"<html><body>
            <video><source src="/assets/a.mp4" type="video/mp4"></video>
            <video><source src="/assets/b.mp4" type="video/mp4"></video>
        </body></html>"#;
        let entries = collect_video_entries(html, &[]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].src, "/assets/a.mp4");
        assert_eq!(entries[1].src, "/assets/b.mp4");
    }

    #[test]
    fn test_collect_video_entries_mixed_forms() {
        let html = r#"<html><body>
            <video src="/assets/inline.mp4" controls></video>
            <video><source src="/assets/sourced.mp4" type="video/mp4"></video>
        </body></html>"#;
        let entries = collect_video_entries(html, &[]);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_form1);
        assert!(!entries[1].is_form1);
    }

    #[test]
    fn test_collect_video_entries_respects_exclude() {
        let html = r#"<html><body><video src="/raw/clip.mp4"></video></body></html>"#;
        let patterns = vec!["raw/*".to_string()];
        let entries = collect_video_entries(html, &patterns);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_build_sources_html() {
        use super::super::videos::VideoVariant;

        let variants = VideoVariants {
            original_width: 1920,
            original_height: 1080,
            vp9: vec![
                VideoVariant {
                    url_path: "/assets/clip-1080p-abc.webm".to_string(),
                    height: 1080,
                    mime_type: "video/webm",
                    codec: "vp9".into(),
                },
                VideoVariant {
                    url_path: "/assets/clip-720p-abc.webm".to_string(),
                    height: 720,
                    mime_type: "video/webm",
                    codec: "vp9".into(),
                },
            ],
            original: VideoVariant {
                url_path: "/assets/clip-abc.mp4".to_string(),
                height: 1080,
                mime_type: "video/mp4",
                codec: "h264".into(),
            },
            poster_url: "/assets/clip-poster-abc.webp".to_string(),
        };

        let html = build_sources_html(&variants);

        // VP9 sources come first (highest res first).
        let vp9_1080_pos = html.find("clip-1080p-abc.webm").unwrap();
        let vp9_720_pos = html.find("clip-720p-abc.webm").unwrap();
        let orig_pos = html.find("clip-abc.mp4").unwrap();

        assert!(vp9_1080_pos < vp9_720_pos);
        assert!(vp9_720_pos < orig_pos);

        // Check type attributes.
        assert!(html.contains("type=\"video/webm; codecs=&quot;vp9&quot;\""));
        assert!(html.contains("type=\"video/mp4\""));
    }

    #[test]
    fn test_should_skip_url() {
        assert!(should_skip_url("http://example.com/video.mp4"));
        assert!(should_skip_url("https://cdn.example.com/video.mp4"));
        assert!(should_skip_url("data:video/mp4;base64,abc"));
        assert!(!should_skip_url("/assets/clip.mp4"));
        assert!(!should_skip_url("assets/clip.mp4"));
    }
}
