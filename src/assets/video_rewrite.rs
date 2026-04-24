//! Video HTML rewriting: collect video elements, optimize, and rewrite.
//!
//! Uses `scraper` for DOM-based collection (to inspect parent-child
//! relationships like `<video>` → `<source>`) and `lol_html` for
//! streaming single-pass rewrite of the final HTML.

use eyre::Result;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use crate::config::VideoOptimConfig;

use super::images::{url_dir_prefix, url_to_dist_path};
use super::videos::{is_excluded, optimize_video, VideoCache, VideoVariants};

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

// ---------------------------------------------------------------------------
// Optimize + rewrite (main entry point)
// ---------------------------------------------------------------------------

/// Process all videos referenced in HTML: optimize and rewrite elements.
///
/// Three phases:
/// 1. Collect `<video>` entries via `collect_video_entries` (scraper).
/// 2. Optimize each unique video via `optimize_video` (async, ffmpeg).
/// 3. Single-pass lol_html rewrite via `rewrite_video_elements`.
pub async fn optimize_and_rewrite_videos(
    html: &str,
    config: &VideoOptimConfig,
    cache: &VideoCache,
    dist_dir: &Path,
) -> Result<String> {
    if !config.optimize {
        return Ok(html.to_string());
    }

    // Phase 1: Collect entries.
    let entries = collect_video_entries(html, &config.exclude);
    if entries.is_empty() {
        return Ok(html.to_string());
    }

    // Phase 2: Optimize each unique source video.
    let mut variant_map: VideoVariantMap = HashMap::new();
    for entry in &entries {
        if variant_map.contains_key(&entry.src) {
            continue;
        }

        let fs_path = url_to_dist_path(&entry.src, dist_dir);
        if !fs_path.exists() {
            tracing::debug!("  Video file not found, skipping: {}", entry.src);
            continue;
        }

        let url_prefix = url_dir_prefix(&entry.src);

        match optimize_video(&fs_path, &url_prefix, config, cache, dist_dir).await {
            Ok(variants) => {
                tracing::debug!(
                    "  Optimized video: {} ({}x{}, {} VP9 variant(s))",
                    entry.src,
                    variants.original_width,
                    variants.original_height,
                    variants.vp9.len(),
                );
                variant_map.insert(entry.src.clone(), variants);
            }
            Err(e) => {
                tracing::warn!("  Failed to optimize video {}: {:#}", entry.src, e);
            }
        }
    }

    if variant_map.is_empty() {
        return Ok(html.to_string());
    }

    // Phase 3: Rewrite HTML.
    rewrite_video_elements(html, &entries, &variant_map)
}

// ---------------------------------------------------------------------------
// lol_html rewrite (single-pass, indexed)
// ---------------------------------------------------------------------------

/// Single-pass lol_html rewrite of `<video>` and child `<source>` elements.
///
/// Maintains a `video_index` counter incremented for each `<video>` element
/// that was collected (i.e., those that passed the same skip predicates used
/// during collection). Videos that were skipped during collection (e.g.,
/// `data-no-optimize`, external, excluded) are also skipped here so indices
/// stay aligned.
pub fn rewrite_video_elements(
    html: &str,
    entries: &[VideoEntry],
    variant_map: &VideoVariantMap,
) -> Result<String> {
    let entries = entries.to_vec();
    let map = variant_map.clone();

    // Index into `entries`, incremented for each qualifying <video>.
    let video_index: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));

    // Tracks state for the current <video> being processed.
    // Some((is_form1, sources_html)) when the current video has variants.
    // None when the current video was skipped or has no variants.
    let current_state: Rc<RefCell<Option<(bool, String)>>> = Rc::new(RefCell::new(None));

    // Whether the first matching <source> within a Form 2 video has been
    // replaced already (we only replace once).
    let source_replaced: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    let idx_for_video = video_index.clone();
    let state_for_video = current_state.clone();
    let replaced_for_video = source_replaced.clone();
    let entries_for_video = entries.clone();
    let map_for_video = map.clone();

    let state_for_source = current_state.clone();
    let replaced_for_source = source_replaced.clone();

    let output = lol_html::rewrite_str(
        html,
        lol_html::RewriteStrSettings {
            element_content_handlers: vec![
                // --- <video> handler ---
                lol_html::element!("video", move |el| {
                    // Reset per-video state.
                    *state_for_video.borrow_mut() = None;
                    *replaced_for_video.borrow_mut() = false;

                    // Check if this video was skipped during collection.
                    // Apply the same predicates: data-no-optimize → skip entirely.
                    if el.get_attribute("data-no-optimize").is_some() {
                        // Not in entries — don't increment index.
                        // Still strip the attribute for clean output.
                        el.remove_attribute("data-no-optimize");
                        return Ok(());
                    }

                    // Check skip predicates on the src (same as collection).
                    // For Form 2 videos (no src), we rely on the entries list
                    // already having filtered during collection.
                    if let Some(s) = el.get_attribute("src") {
                        if should_skip_url(&s) {
                            return Ok(());
                        }
                    }

                    // This video corresponds to entries[video_index].
                    let mut idx = idx_for_video.borrow_mut();
                    let current_idx = *idx;

                    if current_idx >= entries_for_video.len() {
                        return Ok(());
                    }

                    let entry = &entries_for_video[current_idx];
                    *idx += 1;

                    // Look up variants.
                    let variants = match map_for_video.get(&entry.src) {
                        Some(v) => v,
                        None => return Ok(()),
                    };

                    let sources_html = build_sources_html(variants);

                    // Set poster.
                    el.set_attribute("poster", &variants.poster_url)?;

                    // Set preload="none" unless explicitly set.
                    if el.get_attribute("preload").is_none() {
                        el.set_attribute("preload", "none")?;
                    }

                    // Remove data-no-optimize if present (belt-and-suspenders).
                    el.remove_attribute("data-no-optimize");

                    if entry.is_form1 {
                        // Form 1: remove src attr, prepend sources.
                        el.remove_attribute("src");
                        el.prepend(&sources_html, lol_html::html_content::ContentType::Html);
                        *state_for_video.borrow_mut() = Some((true, String::new()));
                    } else {
                        // Form 2: keep element, source handler will replace children.
                        *state_for_video.borrow_mut() = Some((false, sources_html));
                    }

                    Ok(())
                }),
                // --- <source> handler (inside <video>) ---
                lol_html::element!("video source", move |el| {
                    let state = state_for_source.borrow();
                    let Some((is_form1, ref sources_html)) = *state else {
                        return Ok(());
                    };

                    if is_form1 {
                        // Form 1: the original <source> children should be removed
                        // since we prepended our generated sources.
                        el.remove();
                        return Ok(());
                    }

                    // Form 2: replace the first matching <source> with generated
                    // sources; remove subsequent siblings.
                    let mut replaced = replaced_for_source.borrow_mut();
                    if !*replaced {
                        el.replace(sources_html, lol_html::html_content::ContentType::Html);
                        *replaced = true;
                    } else {
                        el.remove();
                    }

                    Ok(())
                }),
            ],
            ..lol_html::RewriteStrSettings::new()
        },
    )
    .map_err(|e| eyre::eyre!("lol_html error rewriting videos: {}", e))?;

    Ok(output)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Task 6: Unit tests ---

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

    // --- Task 7: Integration tests (require ffmpeg) ---

    #[tokio::test]
    async fn test_optimize_and_rewrite_form1_with_ffmpeg() {
        use super::super::videos::{check_ffmpeg, VideoCache};

        if check_ffmpeg().await.is_none() {
            eprintln!("skipping test_optimize_and_rewrite_form1_with_ffmpeg: ffmpeg not found");
            return;
        }

        let tmp = tempfile::TempDir::new().unwrap();
        let dist_dir = tmp.path().join("dist");
        std::fs::create_dir_all(dist_dir.join("assets")).unwrap();

        // Generate a tiny test video.
        let test_video = tmp.path().join("test_src.mp4");
        let ffout = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=160x120:rate=10",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=stereo",
                "-t",
                "1",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
            ])
            .arg(&test_video)
            .output()
            .await
            .unwrap();
        assert!(ffout.status.success(), "ffmpeg test video generation failed");

        let src_path = dist_dir.join("assets/clip.mp4");
        std::fs::copy(&test_video, &src_path).unwrap();

        let config = VideoOptimConfig {
            optimize: true,
            format: "vp9".into(),
            quality: 50,
            heights: vec![60],
            exclude: vec![],
            poster_quality: 50,
        };

        let cache = VideoCache::open(tmp.path()).unwrap();

        let html = r#"<html><body><video src="/assets/clip.mp4" controls></video></body></html>"#;

        let result = optimize_and_rewrite_videos(html, &config, &cache, &dist_dir)
            .await
            .unwrap();

        // Poster added.
        assert!(result.contains("poster="));
        assert!(result.contains(".webp"));

        // preload="none" added.
        assert!(result.contains("preload=\"none\""));

        // VP9 sources present.
        assert!(result.contains("video/webm"));
        assert!(result.contains("vp9"));

        // controls preserved.
        assert!(result.contains("controls"));

        // Original src removed from <video> tag.
        assert!(!result.contains("src=\"/assets/clip.mp4\""));

        // Original available as <source> fallback.
        assert!(result.contains("video/mp4"));
    }

    #[tokio::test]
    async fn test_rewrite_skips_when_disabled() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dist_dir = tmp.path().join("dist");

        let config = VideoOptimConfig {
            optimize: false,
            format: "vp9".into(),
            quality: 30,
            heights: vec![480, 720],
            exclude: vec![],
            poster_quality: 80,
        };

        let cache = VideoCache::open(tmp.path()).unwrap();

        let html = r#"<html><body><video src="/assets/clip.mp4" controls></video></body></html>"#;

        let result = optimize_and_rewrite_videos(html, &config, &cache, &dist_dir)
            .await
            .unwrap();

        // HTML unchanged.
        assert_eq!(result, html);
    }

    #[tokio::test]
    async fn test_rewrite_preserves_explicit_preload() {
        use super::super::videos::{check_ffmpeg, VideoCache};

        if check_ffmpeg().await.is_none() {
            eprintln!(
                "skipping test_rewrite_preserves_explicit_preload: ffmpeg not found"
            );
            return;
        }

        let tmp = tempfile::TempDir::new().unwrap();
        let dist_dir = tmp.path().join("dist");
        std::fs::create_dir_all(dist_dir.join("assets")).unwrap();

        let test_video = tmp.path().join("test_src.mp4");
        let ffout = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=160x120:rate=10",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=stereo",
                "-t",
                "1",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
            ])
            .arg(&test_video)
            .output()
            .await
            .unwrap();
        assert!(ffout.status.success());

        let src_path = dist_dir.join("assets/clip.mp4");
        std::fs::copy(&test_video, &src_path).unwrap();

        let config = VideoOptimConfig {
            optimize: true,
            format: "vp9".into(),
            quality: 50,
            heights: vec![60],
            exclude: vec![],
            poster_quality: 50,
        };

        let cache = VideoCache::open(tmp.path()).unwrap();

        let html = r#"<html><body><video src="/assets/clip.mp4" preload="auto" autoplay muted loop></video></body></html>"#;

        let result = optimize_and_rewrite_videos(html, &config, &cache, &dist_dir)
            .await
            .unwrap();

        // Explicit preload="auto" preserved (not overwritten to "none").
        assert!(result.contains("preload=\"auto\""));
        assert!(!result.contains("preload=\"none\""));

        // autoplay, muted, loop preserved.
        assert!(result.contains("autoplay"));
        assert!(result.contains("muted"));
        assert!(result.contains("loop"));
    }

    #[tokio::test]
    async fn test_rewrite_multiple_form2_correct_posters() {
        use super::super::videos::{check_ffmpeg, VideoCache};

        if check_ffmpeg().await.is_none() {
            eprintln!(
                "skipping test_rewrite_multiple_form2_correct_posters: ffmpeg not found"
            );
            return;
        }

        let tmp = tempfile::TempDir::new().unwrap();
        let dist_dir = tmp.path().join("dist");
        std::fs::create_dir_all(dist_dir.join("assets")).unwrap();

        // Generate TWO distinct test videos (different sizes for different hashes).
        let video_a = tmp.path().join("src_a.mp4");
        let ffout_a = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=160x120:rate=10",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=stereo",
                "-t",
                "1",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
            ])
            .arg(&video_a)
            .output()
            .await
            .unwrap();
        assert!(ffout_a.status.success());

        let video_b = tmp.path().join("src_b.mp4");
        let ffout_b = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=320x240:rate=10",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=stereo",
                "-t",
                "1",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
            ])
            .arg(&video_b)
            .output()
            .await
            .unwrap();
        assert!(ffout_b.status.success());

        let src_a = dist_dir.join("assets/alpha.mp4");
        let src_b = dist_dir.join("assets/beta.mp4");
        std::fs::copy(&video_a, &src_a).unwrap();
        std::fs::copy(&video_b, &src_b).unwrap();

        let config = VideoOptimConfig {
            optimize: true,
            format: "vp9".into(),
            quality: 50,
            heights: vec![60],
            exclude: vec![],
            poster_quality: 50,
        };

        let cache = VideoCache::open(tmp.path()).unwrap();

        let html = r#"<html><body>
            <video><source src="/assets/alpha.mp4" type="video/mp4"></video>
            <video><source src="/assets/beta.mp4" type="video/mp4"></video>
        </body></html>"#;

        let result = optimize_and_rewrite_videos(html, &config, &cache, &dist_dir)
            .await
            .unwrap();

        // CRITICAL: both videos get DIFFERENT poster URLs.
        // Extract poster values.
        let poster_re = regex::Regex::new(r#"poster="([^"]+)""#).unwrap();
        let posters: Vec<String> = poster_re
            .captures_iter(&result)
            .map(|c| c[1].to_string())
            .collect();

        assert_eq!(posters.len(), 2, "Expected 2 poster attributes, got: {posters:?}");
        assert_ne!(
            posters[0], posters[1],
            "Two different videos must get different posters: {posters:?}"
        );

        // Both have poster attributes.
        assert!(posters[0].contains("alpha"));
        assert!(posters[1].contains("beta"));
    }
}
