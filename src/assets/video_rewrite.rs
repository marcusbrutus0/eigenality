//! Video HTML rewriting: collect video elements, optimize, and rewrite.
//!
//! Uses `scraper` for DOM-based collection (to inspect parent-child
//! relationships like `<video>` → `<source>`) and `lol_html` for
//! streaming single-pass rewrite of the final HTML.

use eyre::Result;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write;
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

/// Returns `true` when `src` is a local, non-excluded URL suitable for optimization.
fn is_optimizable(src: &str, exclude_patterns: &[String]) -> bool {
    !should_skip_url(src) && !is_excluded(src.trim_start_matches('/'), exclude_patterns)
}

/// Collect video entries from HTML in document order.
///
/// Returns one slot per `<video>` element in the DOM, in document order.
/// Slots are `None` for videos that should be skipped (data-no-optimize,
/// external URLs, excluded paths, no usable source).  This 1:1 mapping
/// ensures the rewrite phase (which increments unconditionally per
/// `<video>`) stays index-aligned with the collection phase.
///
/// For each `<video>` element:
/// - `None` if `data-no-optimize` attribute is present.
/// - Form 1: `<video src="...">` — `Some(VideoEntry { is_form1: true })`.
/// - Form 2: no `src` attr, first `<source src="...">` child — `Some(VideoEntry { is_form1: false })`.
/// - `None` for external URLs, excluded paths, or no source found.
pub fn collect_video_entries(html: &str, exclude_patterns: &[String]) -> Vec<Option<VideoEntry>> {
    let document = scraper::Html::parse_document(html);
    let video_sel = scraper::Selector::parse("video").expect("valid selector");
    let source_sel = scraper::Selector::parse("source").expect("valid selector");

    let mut entries = Vec::new();

    for video_el in document.select(&video_sel) {
        // Skip videos marked with data-no-optimize.
        if video_el.value().attr("data-no-optimize").is_some() {
            entries.push(None);
            continue;
        }

        // Form 1: <video src="...">
        if let Some(src) = video_el.value().attr("src") {
            entries.push(if is_optimizable(src, exclude_patterns) {
                Some(VideoEntry {
                    src: src.to_string(),
                    is_form1: true,
                })
            } else {
                None
            });
            continue;
        }

        // Form 2: first <source> child with src.
        let form2 = video_el
            .select(&source_sel)
            .next()
            .and_then(|el| el.value().attr("src"))
            .filter(|src| is_optimizable(src, exclude_patterns))
            .map(|src| VideoEntry {
                src: src.to_string(),
                is_form1: false,
            });

        entries.push(form2);
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
        let _ = write!(
            html,
            r#"<source src="{}" type="video/webm; codecs=&quot;vp9&quot;">"#,
            v.url_path,
        );
    }

    // Original fallback.
    let _ = write!(
        html,
        r#"<source src="{}" type="{}">"#,
        variants.original.url_path, variants.original.mime_type,
    );

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

    // Phase 1: Collect entries (one slot per <video>, None for skipped).
    let entries = collect_video_entries(html, &config.exclude);

    // Check if any entries are actionable.
    if !entries.iter().any(|e| e.is_some()) {
        return Ok(html.to_string());
    }

    // Phase 2: Optimize each unique source video.
    let mut variant_map: VideoVariantMap = HashMap::new();
    for entry in entries.iter().flatten() {
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

/// Per-`<video>` state used during the lol_html rewrite pass.
#[derive(Clone)]
enum VideoRewriteState {
    /// Form 1: sources already prepended; strip any original `<source>` children.
    Form1,
    /// Form 2: replace the first `<source>` child with these generated elements,
    /// then remove subsequent `<source>` siblings.
    Form2 { sources_html: String },
}

/// Single-pass lol_html rewrite of `<video>` and child `<source>` elements.
///
/// Maintains a `video_index` counter incremented **unconditionally** for
/// every `<video>` element encountered.  The `entries` slice has one slot
/// per `<video>` (with `None` for skipped ones), so unconditional
/// incrementing keeps indices aligned without re-deriving skip predicates.
pub fn rewrite_video_elements(
    html: &str,
    entries: &[Option<VideoEntry>],
    variant_map: &VideoVariantMap,
) -> Result<String> {
    // Index into `entries`, incremented unconditionally per <video>.
    let video_index: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));

    // State for the current <video> being processed.
    let current_state: Rc<RefCell<Option<VideoRewriteState>>> = Rc::new(RefCell::new(None));

    // Whether the first matching <source> within a Form 2 video has been
    // replaced already (we only replace once).
    let source_replaced: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    let entries_for_video = entries.to_vec();
    let map_for_video = variant_map.clone();
    let idx_for_video = video_index.clone();
    let state_for_video = current_state.clone();
    let replaced_for_video = source_replaced.clone();

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

                    // Unconditionally consume the next slot.
                    let mut idx = idx_for_video.borrow_mut();
                    let current_idx = *idx;
                    *idx += 1;

                    // Look up the entry for this video.
                    let entry = match entries_for_video.get(current_idx) {
                        Some(Some(e)) => e,
                        _ => {
                            // Skipped or out of bounds — strip data-no-optimize
                            // for clean output but don't process further.
                            el.remove_attribute("data-no-optimize");
                            return Ok(());
                        }
                    };

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
                        *state_for_video.borrow_mut() = Some(VideoRewriteState::Form1);
                    } else {
                        // Form 2: source handler will replace children.
                        *state_for_video.borrow_mut() =
                            Some(VideoRewriteState::Form2 { sources_html });
                    }

                    Ok(())
                }),
                // --- <source> handler (inside <video>) ---
                lol_html::element!("video source", move |el| {
                    let state = state_for_source.borrow();
                    let Some(ref rewrite_state) = *state else {
                        return Ok(());
                    };

                    match rewrite_state {
                        VideoRewriteState::Form1 => {
                            // Original <source> children removed; we already
                            // prepended generated sources.
                            el.remove();
                        }
                        VideoRewriteState::Form2 { sources_html } => {
                            // Replace the first <source> with generated elements;
                            // remove subsequent siblings.
                            let mut replaced = replaced_for_source.borrow_mut();
                            if !*replaced {
                                el.replace(sources_html, lol_html::html_content::ContentType::Html);
                                *replaced = true;
                            } else {
                                el.remove();
                            }
                        }
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
        let e = entries[0].as_ref().unwrap();
        assert_eq!(e.src, "/assets/clip.mp4");
        assert!(e.is_form1);
    }

    #[test]
    fn test_collect_video_entries_form2() {
        let html = r#"<html><body><video controls><source src="/assets/clip.mp4" type="video/mp4"></video></body></html>"#;
        let entries = collect_video_entries(html, &[]);
        assert_eq!(entries.len(), 1);
        let e = entries[0].as_ref().unwrap();
        assert_eq!(e.src, "/assets/clip.mp4");
        assert!(!e.is_form1);
    }

    #[test]
    fn test_collect_video_entries_skips_external() {
        let html =
            r#"<html><body><video src="https://cdn.example.com/video.mp4"></video></body></html>"#;
        let entries = collect_video_entries(html, &[]);
        assert_eq!(entries.len(), 1); // one slot
        assert!(entries[0].is_none()); // but skipped
    }

    #[test]
    fn test_collect_video_entries_skips_data_no_optimize() {
        let html = r#"<html><body><video src="/assets/clip.mp4" data-no-optimize></video></body></html>"#;
        let entries = collect_video_entries(html, &[]);
        assert_eq!(entries.len(), 1); // one slot
        assert!(entries[0].is_none()); // but skipped
    }

    #[test]
    fn test_collect_video_entries_multiple_form2() {
        let html = r#"<html><body>
            <video><source src="/assets/a.mp4" type="video/mp4"></video>
            <video><source src="/assets/b.mp4" type="video/mp4"></video>
        </body></html>"#;
        let entries = collect_video_entries(html, &[]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].as_ref().unwrap().src, "/assets/a.mp4");
        assert_eq!(entries[1].as_ref().unwrap().src, "/assets/b.mp4");
    }

    #[test]
    fn test_collect_video_entries_mixed_forms() {
        let html = r#"<html><body>
            <video src="/assets/inline.mp4" controls></video>
            <video><source src="/assets/sourced.mp4" type="video/mp4"></video>
        </body></html>"#;
        let entries = collect_video_entries(html, &[]);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].as_ref().unwrap().is_form1);
        assert!(!entries[1].as_ref().unwrap().is_form1);
    }

    #[test]
    fn test_collect_video_entries_respects_exclude() {
        let html = r#"<html><body><video src="/raw/clip.mp4"></video></body></html>"#;
        let patterns = vec!["raw/*".to_string()];
        let entries = collect_video_entries(html, &patterns);
        assert_eq!(entries.len(), 1); // one slot
        assert!(entries[0].is_none()); // but skipped
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
                },
                VideoVariant {
                    url_path: "/assets/clip-720p-abc.webm".to_string(),
                    height: 720,
                    mime_type: "video/webm",
                },
            ],
            original: VideoVariant {
                url_path: "/assets/clip-abc.mp4".to_string(),
                height: 1080,
                mime_type: "video/mp4",
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

    // --- Rewrite unit tests (no ffmpeg needed) ---

    /// Helper: build a simple VideoVariants for testing rewrite logic.
    fn test_variants(stem: &str) -> VideoVariants {
        use super::super::videos::VideoVariant;
        VideoVariants {
            original_width: 1920,
            original_height: 1080,
            vp9: vec![VideoVariant {
                url_path: format!("/assets/{stem}-1080p.webm"),
                height: 1080,
                mime_type: "video/webm",
            }],
            original: VideoVariant {
                url_path: format!("/assets/{stem}.mp4"),
                height: 1080,
                mime_type: "video/mp4",
            },
            poster_url: format!("/assets/{stem}-poster.webp"),
        }
    }

    #[test]
    fn test_rewrite_form1_basic() {
        let html = r#"<html><body><video src="/assets/clip.mp4" controls></video></body></html>"#;
        let entries = vec![Some(VideoEntry {
            src: "/assets/clip.mp4".into(),
            is_form1: true,
        })];
        let mut map = VideoVariantMap::new();
        map.insert("/assets/clip.mp4".into(), test_variants("clip"));

        let result = rewrite_video_elements(html, &entries, &map).unwrap();

        // src attribute removed from <video> element (no longer an attribute).
        // The video tag should not have src= anymore, but <source> children will.
        assert!(
            !result.contains(r#"<video src="#),
            "Original src should be removed from <video> tag",
        );
        // Poster added.
        assert!(result.contains(r#"poster="/assets/clip-poster.webp""#));
        // preload="none" added.
        assert!(result.contains(r#"preload="none""#));
        // controls preserved.
        assert!(result.contains("controls"));
        // VP9 source injected.
        assert!(result.contains("clip-1080p.webm"));
        // Original fallback source injected.
        assert!(result.contains(r#"type="video/mp4""#));
    }

    #[test]
    fn test_rewrite_form2_basic() {
        let html = r#"<html><body><video><source src="/assets/clip.mp4" type="video/mp4"></video></body></html>"#;
        let entries = vec![Some(VideoEntry {
            src: "/assets/clip.mp4".into(),
            is_form1: false,
        })];
        let mut map = VideoVariantMap::new();
        map.insert("/assets/clip.mp4".into(), test_variants("clip"));

        let result = rewrite_video_elements(html, &entries, &map).unwrap();

        // Poster added.
        assert!(result.contains(r#"poster="/assets/clip-poster.webp""#));
        // preload="none" added.
        assert!(result.contains(r#"preload="none""#));
        // VP9 source present.
        assert!(result.contains("clip-1080p.webm"));
        // Original <source> replaced (original was type="video/mp4").
        // The new sources include a video/mp4 fallback too.
        assert!(result.contains(r#"type="video/mp4""#));
    }

    #[test]
    fn test_rewrite_preserves_preload_attr() {
        let html = r#"<html><body><video src="/assets/clip.mp4" preload="auto"></video></body></html>"#;
        let entries = vec![Some(VideoEntry {
            src: "/assets/clip.mp4".into(),
            is_form1: true,
        })];
        let mut map = VideoVariantMap::new();
        map.insert("/assets/clip.mp4".into(), test_variants("clip"));

        let result = rewrite_video_elements(html, &entries, &map).unwrap();

        // Explicit preload="auto" preserved, not overwritten.
        assert!(result.contains(r#"preload="auto""#));
        assert!(!result.contains(r#"preload="none""#));
    }

    #[test]
    fn test_rewrite_skipped_video_in_middle_alignment() {
        // Three <video> elements: first and third are valid, second is
        // skipped (e.g., excluded or external). Verifies the third video
        // gets its own poster, not the second entry's.
        let html = r#"<html><body>
            <video><source src="/assets/alpha.mp4" type="video/mp4"></video>
            <video><source src="https://external.com/skip.mp4" type="video/mp4"></video>
            <video><source src="/assets/beta.mp4" type="video/mp4"></video>
        </body></html>"#;

        let entries = vec![
            Some(VideoEntry {
                src: "/assets/alpha.mp4".into(),
                is_form1: false,
            }),
            None, // skipped (external)
            Some(VideoEntry {
                src: "/assets/beta.mp4".into(),
                is_form1: false,
            }),
        ];

        let mut map = VideoVariantMap::new();
        map.insert("/assets/alpha.mp4".into(), test_variants("alpha"));
        map.insert("/assets/beta.mp4".into(), test_variants("beta"));

        let result = rewrite_video_elements(html, &entries, &map).unwrap();

        // Extract poster values.
        let poster_re = regex::Regex::new(r#"poster="([^"]+)""#).unwrap();
        let posters: Vec<String> = poster_re
            .captures_iter(&result)
            .map(|c| c[1].to_string())
            .collect();

        assert_eq!(posters.len(), 2, "Expected 2 poster attributes: {posters:?}");
        assert!(
            posters[0].contains("alpha"),
            "First poster should be alpha: {}",
            posters[0],
        );
        assert!(
            posters[1].contains("beta"),
            "Third video (second valid) should get beta poster: {}",
            posters[1],
        );
    }

    #[test]
    fn test_rewrite_data_no_optimize_stripped_from_skipped() {
        // A video with data-no-optimize: the attribute should be stripped
        // from the output even though the video is not optimized.
        let html = r#"<html><body><video src="/assets/clip.mp4" data-no-optimize controls></video></body></html>"#;
        let entries = vec![None]; // skipped
        let map = VideoVariantMap::new();

        let result = rewrite_video_elements(html, &entries, &map).unwrap();

        // data-no-optimize removed.
        assert!(!result.contains("data-no-optimize"));
        // Original src and controls preserved.
        assert!(result.contains(r#"src="/assets/clip.mp4""#));
        assert!(result.contains("controls"));
    }

    #[test]
    fn test_rewrite_two_form2_different_posters() {
        // Two Form 2 videos — verifies they each get their own poster.
        let html = r#"<html><body>
            <video><source src="/assets/alpha.mp4" type="video/mp4"></video>
            <video><source src="/assets/beta.mp4" type="video/mp4"></video>
        </body></html>"#;

        let entries = vec![
            Some(VideoEntry {
                src: "/assets/alpha.mp4".into(),
                is_form1: false,
            }),
            Some(VideoEntry {
                src: "/assets/beta.mp4".into(),
                is_form1: false,
            }),
        ];

        let mut map = VideoVariantMap::new();
        map.insert("/assets/alpha.mp4".into(), test_variants("alpha"));
        map.insert("/assets/beta.mp4".into(), test_variants("beta"));

        let result = rewrite_video_elements(html, &entries, &map).unwrap();

        let poster_re = regex::Regex::new(r#"poster="([^"]+)""#).unwrap();
        let posters: Vec<String> = poster_re
            .captures_iter(&result)
            .map(|c| c[1].to_string())
            .collect();

        assert_eq!(posters.len(), 2);
        assert_ne!(posters[0], posters[1]);
        assert!(posters[0].contains("alpha"));
        assert!(posters[1].contains("beta"));
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

    /// End-to-end test with a real video: generates a video, runs the full
    /// optimize_and_rewrite pipeline, parses the output HTML structurally,
    /// and verifies every generated file exists on disk and is non-empty.
    #[tokio::test]
    async fn test_e2e_real_video_output_verified() {
        use super::super::videos::{check_ffmpeg, VideoCache};

        if check_ffmpeg().await.is_none() {
            eprintln!("skipping test_e2e_real_video_output_verified: ffmpeg not found");
            return;
        }

        let tmp = tempfile::TempDir::new().unwrap();
        let dist_dir = tmp.path().join("dist");
        std::fs::create_dir_all(dist_dir.join("videos")).unwrap();

        // Generate a 160x120 test video with audio.
        let src_video = dist_dir.join("videos/demo.mp4");
        let ffout = tokio::process::Command::new("ffmpeg")
            .args([
                "-y", "-f", "lavfi", "-i",
                "testsrc=duration=1:size=160x120:rate=10",
                "-f", "lavfi", "-i", "anullsrc=r=44100:cl=stereo",
                "-t", "1",
                "-c:v", "libx264", "-pix_fmt", "yuv420p",
                "-c:a", "aac",
            ])
            .arg(&src_video)
            .output()
            .await
            .unwrap();
        assert!(ffout.status.success(), "failed to generate test video");
        assert!(src_video.metadata().unwrap().len() > 0);

        let config = VideoOptimConfig {
            optimize: true,
            quality: 50,
            heights: vec![60, 120, 480],
            exclude: vec![],
            poster_quality: 80,
        };
        let cache = VideoCache::open(tmp.path()).unwrap();

        let input_html = r#"<html><body><video src="/videos/demo.mp4" controls class="hero"></video></body></html>"#;

        let output_html = optimize_and_rewrite_videos(input_html, &config, &cache, &dist_dir)
            .await
            .unwrap();

        // --- Parse output HTML structurally ---
        let doc = scraper::Html::parse_document(&output_html);
        let video_sel = scraper::Selector::parse("video").unwrap();
        let source_sel = scraper::Selector::parse("source").unwrap();

        let videos: Vec<_> = doc.select(&video_sel).collect();
        assert_eq!(videos.len(), 1, "expected exactly 1 <video> element");

        let video = videos[0];

        // <video> must NOT have src attribute (moved to <source> children).
        assert!(
            video.value().attr("src").is_none(),
            "src attribute should be removed from <video>"
        );

        // poster attribute present and points to a .webp file.
        let poster = video.value().attr("poster")
            .expect("<video> must have poster attribute");
        assert!(poster.ends_with(".webp"), "poster should be webp: {poster}");

        // preload="none" set.
        assert_eq!(
            video.value().attr("preload"),
            Some("none"),
            "preload should be 'none'"
        );

        // Original attributes preserved.
        assert_eq!(video.value().attr("controls"), Some(""));
        assert_eq!(video.value().attr("class"), Some("hero"));

        // --- Verify <source> children ---
        let sources: Vec<_> = video.select(&source_sel).collect();
        // 60p + 120p (source height, 480 skipped) VP9 + 1 original = 3 sources.
        assert_eq!(
            sources.len(), 3,
            "expected 3 <source> elements (2 VP9 + 1 fallback), got {}",
            sources.len()
        );

        // First two should be VP9 webm, highest resolution first.
        let src0 = sources[0].value().attr("src").unwrap();
        let type0 = sources[0].value().attr("type").unwrap();
        assert!(src0.ends_with(".webm"), "first source should be webm: {src0}");
        assert!(src0.contains("120p"), "first VP9 source should be 120p (highest): {src0}");
        assert!(type0.contains("vp9"), "first source type should mention vp9: {type0}");

        let src1 = sources[1].value().attr("src").unwrap();
        let type1 = sources[1].value().attr("type").unwrap();
        assert!(src1.ends_with(".webm"), "second source should be webm: {src1}");
        assert!(src1.contains("60p"), "second VP9 source should be 60p: {src1}");
        assert!(type1.contains("vp9"), "second source type should mention vp9: {type1}");

        // Last source is original mp4 fallback.
        let src2 = sources[2].value().attr("src").unwrap();
        let type2 = sources[2].value().attr("type").unwrap();
        assert!(src2.ends_with(".mp4"), "fallback source should be mp4: {src2}");
        assert_eq!(type2, "video/mp4", "fallback type should be video/mp4: {type2}");

        // --- Verify all referenced files exist on disk and are non-empty ---
        for (i, source) in sources.iter().enumerate() {
            let src_url = source.value().attr("src").unwrap();
            let file_path = dist_dir.join(src_url.trim_start_matches('/'));
            assert!(
                file_path.exists(),
                "source[{i}] file does not exist on disk: {}",
                file_path.display()
            );
            let size = file_path.metadata().unwrap().len();
            assert!(
                size > 0,
                "source[{i}] file is empty: {} ({size} bytes)",
                file_path.display()
            );
        }

        // Poster file exists and is non-empty.
        let poster_path = dist_dir.join(poster.trim_start_matches('/'));
        assert!(
            poster_path.exists(),
            "poster file does not exist: {}",
            poster_path.display()
        );
        let poster_size = poster_path.metadata().unwrap().len();
        assert!(
            poster_size > 0,
            "poster file is empty: {} ({poster_size} bytes)",
            poster_path.display()
        );

        // --- Verify VP9 files are valid by probing with ffprobe ---
        for source in &sources[..2] {
            let src_url = source.value().attr("src").unwrap();
            let file_path = dist_dir.join(src_url.trim_start_matches('/'));
            let probe = tokio::process::Command::new("ffprobe")
                .args(["-v", "quiet", "-print_format", "json", "-show_streams", "-select_streams", "v:0"])
                .arg(&file_path)
                .output()
                .await
                .unwrap();
            assert!(
                probe.status.success(),
                "ffprobe failed on {}: {}",
                file_path.display(),
                String::from_utf8_lossy(&probe.stderr)
            );
            let json: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
            let codec = json["streams"][0]["codec_name"].as_str().unwrap_or("");
            assert_eq!(
                codec, "vp9",
                "expected vp9 codec in {}, got: {codec}",
                file_path.display()
            );
        }
    }
}
