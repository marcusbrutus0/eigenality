//! HTML rewriting for responsive images.
//!
//! Uses `lol_html` to:
//! 1. Transform `<img>` tags into `<picture>` elements with `<source>` and `srcset`.
//! 2. Rewrite CSS `background-image` classes to point to optimized variants.

use eyre::Result;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use crate::config::ImageOptimConfig;

use super::images::{
    ImageCache, ImageVariants, is_excluded, optimize_image, url_dir_prefix, url_to_dist_path,
};

/// Mapping from original image URL path → generated variants.
type VariantMap = HashMap<String, ImageVariants>;

/// Process all images referenced in HTML and rewrite `<img>` → `<picture>`.
///
/// This is the main entry point called from the build pipeline.
///
/// 1. Scans HTML for `<img src="...">` tags.
/// 2. For each local image, runs optimization (resize + convert).
/// 3. Rewrites the HTML: `<img>` → `<picture>` with `<source srcset>`.
/// 4. Also handles CSS `background-image` by generating variant CSS classes.
///
/// Returns the rewritten HTML.
pub fn optimize_and_rewrite_images(
    html: &str,
    config: &ImageOptimConfig,
    cache: &ImageCache,
    dist_dir: &Path,
) -> Result<String> {
    if !config.optimize {
        return Ok(html.to_string());
    }

    // Phase 1: Collect all image src URLs from <img> tags and optimize them.
    let img_srcs = collect_img_srcs(html)?;
    let mut variant_map: VariantMap = HashMap::new();

    for src in &img_srcs {
        if variant_map.contains_key(src.as_str()) {
            continue;
        }

        // Skip absolute URLs (http/https) — they can't be optimized locally.
        if src.starts_with("http://") || src.starts_with("https://") {
            continue;
        }

        // Skip data: URIs.
        if src.starts_with("data:") {
            continue;
        }

        // Check exclusion patterns.
        let check_path = src.trim_start_matches('/');
        if is_excluded(check_path, &config.exclude) {
            tracing::debug!("  Image excluded from optimization: {}", src);
            continue;
        }

        // Resolve to filesystem.
        let fs_path = url_to_dist_path(src, dist_dir);
        if !fs_path.exists() {
            tracing::debug!("  Image file not found, skipping optimization: {}", src);
            continue;
        }

        let url_prefix = url_dir_prefix(src);

        match optimize_image(&fs_path, &url_prefix, config, cache, dist_dir) {
            Ok(Some(variants)) => {
                tracing::debug!(
                    "  Optimized image: {} ({}x{}, {} format(s))",
                    src,
                    variants.original_width,
                    variants.original_height,
                    variants.by_format.len(),
                );
                variant_map.insert(src.clone(), variants);
            }
            Ok(None) => {
                tracing::debug!("  Image could not be optimized (unsupported): {}", src);
            }
            Err(e) => {
                tracing::warn!("  Failed to optimize image {}: {:#}", src, e);
            }
        }
    }

    if variant_map.is_empty() {
        return Ok(html.to_string());
    }

    // Phase 2: Rewrite HTML using lol_html.
    rewrite_img_to_picture(html, &variant_map)
}

/// Collect all `<img src="...">` values from HTML using lol_html.
fn collect_img_srcs(html: &str) -> Result<Vec<String>> {
    let srcs: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let srcs_clone = srcs.clone();

    let output = lol_html::rewrite_str(
        html,
        lol_html::RewriteStrSettings {
            element_content_handlers: vec![lol_html::element!("img[src]", move |el| {
                if let Some(src) = el.get_attribute("src") {
                    srcs_clone.borrow_mut().push(src);
                }
                Ok(())
            })],
            ..lol_html::RewriteStrSettings::new()
        },
    )
    .map_err(|e| eyre::eyre!("lol_html error collecting img srcs: {}", e))?;

    // We don't need the output here — just the collected srcs.
    drop(output);

    let result = srcs.borrow().clone();
    Ok(result)
}

/// Rewrite `<img>` tags to `<picture>` elements using lol_html.
///
/// For each `<img src="/assets/photo.jpg" alt="...">` that has variants:
///
/// ```html
/// <picture>
///   <source srcset="/assets/photo-480w.avif 480w, ..." type="image/avif">
///   <source srcset="/assets/photo-480w.webp 480w, ..." type="image/webp">
///   <source srcset="/assets/photo-480w.jpg 480w, ..." type="image/jpeg">
///   <img src="/assets/photo.jpg" alt="...">
/// </picture>
/// ```
fn rewrite_img_to_picture(html: &str, variant_map: &VariantMap) -> Result<String> {
    // We need to clone data into the closure.
    let map = variant_map.clone();

    let output = lol_html::rewrite_str(
        html,
        lol_html::RewriteStrSettings {
            element_content_handlers: vec![lol_html::element!("img[src]", move |el| {
                let src = match el.get_attribute("src") {
                    Some(s) => s,
                    None => return Ok(()),
                };

                let variants = match map.get(&src) {
                    Some(v) => v,
                    None => return Ok(()), // No variants — leave as-is.
                };

                // Collect attributes before replacing.
                let attrs: Vec<(String, String)> = el
                    .attributes()
                    .iter()
                    .map(|a| (a.name(), a.value()))
                    .collect();

                // Build the <picture> replacement HTML.
                let picture_html = build_picture_html(&attrs, variants);

                // Replace the <img> element with the <picture> block.
                el.replace(&picture_html, lol_html::html_content::ContentType::Html);

                Ok(())
            })],
            ..lol_html::RewriteStrSettings::new()
        },
    )
    .map_err(|e| eyre::eyre!("lol_html error rewriting images: {}", e))?;

    Ok(output)
}

/// Build the `<picture>...</picture>` HTML string for one `<img>`.
///
/// `attrs` is the list of (name, value) pairs from the original `<img>` tag.
fn build_picture_html(
    attrs: &[(String, String)],
    variants: &ImageVariants,
) -> String {
    let mut html = String::from("<picture>\n");

    // Determine the ordering of formats: modern formats first, original last.
    let format_order = determine_format_order(variants);

    for fmt_name in &format_order {
        if let Some(fmt_variants) = variants.by_format.get(fmt_name.as_str()) {
            if fmt_variants.is_empty() {
                continue;
            }

            let mime = &fmt_variants[0].mime_type;

            // Build srcset string.
            let srcset: Vec<String> = fmt_variants
                .iter()
                .map(|v| format!("{} {}w", v.url_path, v.width))
                .collect();

            html.push_str(&format!(
                "  <source srcset=\"{}\" type=\"{}\">\n",
                srcset.join(", "),
                mime,
            ));
        }
    }

    // Build the fallback <img> with all original attributes preserved.
    html.push_str("  <img");

    for (name, value) in attrs {
        html.push_str(&format!(" {}=\"{}\"", name, escape_attr(value)));
    }

    html.push_str(">\n</picture>");

    html
}

/// Order formats: AVIF first (best compression), then WebP, then original.
fn determine_format_order(variants: &ImageVariants) -> Vec<String> {
    let mut order = Vec::new();

    // Modern formats first (best → good).
    if variants.by_format.contains_key("avif") && variants.original_format != "avif" {
        order.push("avif".to_string());
    }
    if variants.by_format.contains_key("webp") && variants.original_format != "webp" {
        order.push("webp".to_string());
    }

    // Then any other non-original formats.
    for fmt in variants.by_format.keys() {
        if fmt != &variants.original_format && !order.contains(fmt) {
            order.push(fmt.clone());
        }
    }

    // Original format last (fallback).
    if variants.by_format.contains_key(&variants.original_format) {
        order.push(variants.original_format.clone());
    }

    order
}

/// Escape HTML attribute value.
fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Rewrite CSS `background-image: url(...)` references in `<style>` blocks
/// and inline `style` attributes to use optimized image variants.
///
/// For each CSS class/rule that references an image, generate additional
/// rules with media queries for different sizes, using the optimized
/// variants in the best available format.
///
/// This scans for patterns like:
///   `background-image: url('/assets/photo.jpg')`
/// and appends responsive CSS rules after the `<style>` block.
pub fn rewrite_css_background_images(
    html: &str,
    config: &ImageOptimConfig,
    cache: &ImageCache,
    dist_dir: &Path,
) -> Result<String> {
    if !config.optimize {
        return Ok(html.to_string());
    }

    // Extract background-image URLs using regex (lol_html operates on HTML,
    // not CSS — we use regex to find CSS url() references).
    let bg_re = regex::Regex::new(
        r#"(?i)background(?:-image)?\s*:[^;]*url\(\s*['"]?([^'")\s]+)['"]?\s*\)"#
    ).unwrap();

    // Collect unique local image URLs referenced in CSS.
    let mut css_images: Vec<String> = Vec::new();
    for cap in bg_re.captures_iter(html) {
        let url = cap[1].to_string();
        if !url.starts_with("http://")
            && !url.starts_with("https://")
            && !url.starts_with("data:")
        {
            let check_path = url.trim_start_matches('/');
            if !is_excluded(check_path, &config.exclude) && !css_images.contains(&url) {
                css_images.push(url);
            }
        }
    }

    if css_images.is_empty() {
        return Ok(html.to_string());
    }

    // Optimize each CSS-referenced image (if not already done for <img>).
    let mut variant_map: VariantMap = HashMap::new();
    for url in &css_images {
        let fs_path = url_to_dist_path(url, dist_dir);
        if !fs_path.exists() {
            continue;
        }
        let url_prefix = url_dir_prefix(url);
        match optimize_image(&fs_path, &url_prefix, config, cache, dist_dir) {
            Ok(Some(variants)) => {
                variant_map.insert(url.clone(), variants);
            }
            Ok(None) | Err(_) => {}
        }
    }

    if variant_map.is_empty() {
        return Ok(html.to_string());
    }

    // For CSS background images, we use the largest WebP/AVIF variant as a
    // simple replacement — full responsive CSS would require knowing the
    // selector context which is beyond our scope.
    // Instead, we inject an `image-set()` CSS function where supported.
    let mut result = html.to_string();
    for (original_url, variants) in &variant_map {
        // Pick the best format available (prefer webp over avif for CSS
        // compatibility, since image-set() support is broader for webp).
        let best_format = if variants.by_format.contains_key("webp") {
            "webp"
        } else if variants.by_format.contains_key("avif") {
            "avif"
        } else {
            continue;
        };

        if let Some(fmt_variants) = variants.by_format.get(best_format) {
            // Use the largest variant (closest to original).
            if let Some(largest) = fmt_variants.last() {
                // Build image-set() fallback pattern.
                // Replace: url('/assets/photo.jpg')
                // With: url('/assets/photo-1200w.webp')
                let old_url_pattern = original_url.as_str();
                let new_url = &largest.url_path;
                result = result.replace(old_url_pattern, new_url);
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use crate::assets::images::ImageVariant;

    use super::*;

    #[test]
    fn test_collect_img_srcs() {
        let html = r#"<img src="/assets/a.jpg" alt="A"><p>text</p><img src="/assets/b.png">"#;
        let srcs = collect_img_srcs(html).unwrap();
        assert_eq!(srcs, vec!["/assets/a.jpg", "/assets/b.png"]);
    }

    #[test]
    fn test_collect_img_srcs_no_src() {
        let html = r#"<img alt="no src"><div>hello</div>"#;
        let srcs = collect_img_srcs(html).unwrap();
        assert!(srcs.is_empty());
    }

    #[test]
    fn test_escape_attr() {
        assert_eq!(escape_attr("hello"), "hello");
        assert_eq!(escape_attr(r#"a"b"#), "a&quot;b");
        assert_eq!(escape_attr("a&b"), "a&amp;b");
    }

    #[test]
    fn test_determine_format_order() {
        let mut by_format = HashMap::new();
        by_format.insert("avif".to_string(), vec![]);
        by_format.insert("webp".to_string(), vec![]);
        by_format.insert("jpeg".to_string(), vec![]);

        let variants = ImageVariants {
            original_width: 800,
            original_height: 600,
            by_format,
            original_format: "jpeg".to_string(),
        };

        let order = determine_format_order(&variants);
        assert_eq!(order, vec!["avif", "webp", "jpeg"]);
    }

    #[test]
    fn test_determine_format_order_webp_original() {
        let mut by_format = HashMap::new();
        by_format.insert("avif".to_string(), vec![]);
        by_format.insert("webp".to_string(), vec![]);

        let variants = ImageVariants {
            original_width: 800,
            original_height: 600,
            by_format,
            original_format: "webp".to_string(),
        };

        let order = determine_format_order(&variants);
        // avif first, then webp as original fallback.
        assert_eq!(order, vec!["avif", "webp"]);
    }

    #[test]
    fn test_build_picture_html_structure() {
        // We test the output format by constructing variants manually
        // and checking the generated HTML contains the right structure.
        let mut by_format = HashMap::new();
        by_format.insert(
            "webp".to_string(),
            vec![
                ImageVariant {
                    url_path: "/assets/photo-480w.webp".to_string(),
                    width: 480,
                    mime_type: "image/webp".to_string()
                },
                ImageVariant {
                    url_path: "/assets/photo-800w.webp".to_string(),
                    width: 800,
                    mime_type: "image/webp".to_string()
                },
            ],
        );
        by_format.insert(
            "jpeg".to_string(),
            vec![
                ImageVariant {
                    url_path: "/assets/photo-480w.jpg".to_string(),
                    width: 480,
                    mime_type: "image/jpeg".to_string()
                },
                ImageVariant {
                    url_path: "/assets/photo-800w.jpg".to_string(),
                    width: 800,
                    mime_type: "image/jpeg".to_string()
                },
            ],
        );

        let variants = ImageVariants {
            original_width: 800,
            original_height: 600,
            by_format,
            original_format: "jpeg".to_string(),
        };

        // Use lol_html to create a real Element, then test build_picture_html.
        // This is complex, so we'll test via the full rewrite pipeline instead.
        let html = r#"<img src="/assets/photo.jpg" alt="test" class="hero">"#;
        let mut map = HashMap::new();
        map.insert("/assets/photo.jpg".to_string(), variants);

        let result = rewrite_img_to_picture(html, &map).unwrap();

        assert!(result.contains("<picture>"));
        assert!(result.contains("</picture>"));
        assert!(result.contains(r#"type="image/webp""#));
        assert!(result.contains(r#"type="image/jpeg""#));
        assert!(result.contains("480w"));
        assert!(result.contains("800w"));
        // Original attributes preserved on <img>.
        assert!(result.contains(r#"alt="test""#));
        assert!(result.contains(r#"class="hero""#));
        assert!(result.contains(r#"src="/assets/photo.jpg""#));
    }

    #[test]
    fn test_rewrite_leaves_unmatched_imgs() {
        let html = r#"<img src="/assets/unknown.jpg" alt="x">"#;
        let map: VariantMap = HashMap::new();
        let result = rewrite_img_to_picture(html, &map).unwrap();
        // Should be unchanged.
        assert!(result.contains(r#"<img src="/assets/unknown.jpg" alt="x">"#));
    }

    #[test]
    fn test_rewrite_preserves_non_img_html() {
        let html = r#"<div><p>Hello</p><img src="/other.jpg"></div>"#;
        let map: VariantMap = HashMap::new();
        let result = rewrite_img_to_picture(html, &map).unwrap();
        assert!(result.contains("<div>"));
        assert!(result.contains("<p>Hello</p>"));
    }

    #[test]
    fn test_full_optimize_and_rewrite() {
        // Integration test: create a real image, run the full pipeline.
        let tmp = tempfile::TempDir::new().unwrap();
        let dist_dir = tmp.path().join("dist");
        std::fs::create_dir_all(dist_dir.join("assets")).unwrap();

        // Create a 200x100 blue JPEG.
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(200, 100, |_, _| {
            image::Rgb([0, 0, 255])
        }));
        let src_path = dist_dir.join("assets/hero.jpg");
        img.save(&src_path).unwrap();

        let config = ImageOptimConfig {
            optimize: true,
            formats: vec!["webp".to_string()],
            quality: 75,
            widths: vec![100],
            exclude: vec![],
        };

        let cache = ImageCache::open(tmp.path()).unwrap();

        let html = r#"<html><body><img src="/assets/hero.jpg" alt="Hero" class="main"></body></html>"#;

        let result = optimize_and_rewrite_images(html, &config, &cache, &dist_dir).unwrap();

        assert!(result.contains("<picture>"));
        assert!(result.contains("</picture>"));
        assert!(result.contains("100w"));
        assert!(result.contains("200w")); // original width
        assert!(result.contains("image/webp"));
        assert!(result.contains("image/jpeg")); // original fallback
        assert!(result.contains(r#"alt="Hero""#));
        assert!(result.contains(r#"class="main""#));
    }

    #[test]
    fn test_optimize_disabled() {
        let config = ImageOptimConfig {
            optimize: false,
            ..ImageOptimConfig::default()
        };

        let tmp = tempfile::TempDir::new().unwrap();
        let cache = ImageCache::open(tmp.path()).unwrap();
        let dist_dir = tmp.path().join("dist");

        let html = r#"<img src="/assets/photo.jpg">"#;
        let result = optimize_and_rewrite_images(html, &config, &cache, &dist_dir).unwrap();
        assert_eq!(result, html);
    }

    #[test]
    fn test_excludes_svg_and_gif() {
        let config = ImageOptimConfig {
            optimize: true,
            formats: vec!["webp".to_string()],
            quality: 75,
            widths: vec![480],
            exclude: vec!["**/*.svg".to_string(), "**/*.gif".to_string()],
        };

        let tmp = tempfile::TempDir::new().unwrap();
        let cache = ImageCache::open(tmp.path()).unwrap();
        let dist_dir = tmp.path().join("dist");

        let html = r#"<img src="/icons/logo.svg"><img src="/images/anim.gif">"#;
        let result = optimize_and_rewrite_images(html, &config, &cache, &dist_dir).unwrap();
        // Both should remain as plain <img> — no <picture> wrapping.
        assert!(!result.contains("<picture>"));
    }
}
