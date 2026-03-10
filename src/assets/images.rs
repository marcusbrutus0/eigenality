//! Image optimization: format conversion, compression, and responsive resizing.
//!
//! Processes image files (from `dist/assets/` or `static/`) into multiple
//! formats and sizes, writes them to `dist/`, and returns a manifest of
//! generated variants that the HTML rewriter uses to build `<picture>` elements.

use eyre::{Result, WrapErr};
use image::DynamicImage;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use crate::config::ImageOptimConfig;

/// Describes a single generated image variant.
#[derive(Debug, Clone)]
pub struct ImageVariant {
    /// URL path relative to site root, e.g. `/assets/photo-480w.webp`.
    pub url_path: String,
    /// The pixel width of this variant.
    pub width: u32,
    /// MIME type, e.g. `image/webp`, `image/avif`, `image/jpeg`.
    pub mime_type: String
}

/// The full set of variants generated for a single source image.
#[derive(Debug, Clone)]
pub struct ImageVariants {
    /// Original image width.
    pub original_width: u32,
    /// Original image height.
    pub original_height: u32,
    /// Variants grouped by output format.
    /// Key = format name (`"webp"`, `"avif"`, `"jpeg"`, `"png"`, ...).
    /// Value = variants sorted by width ascending.
    pub by_format: HashMap<String, Vec<ImageVariant>>,
    /// The original format name (for the fallback `<source>`/`<img>`).
    pub original_format: String,
}

/// On-disk cache for optimized images.
///
/// Lives under `.eigen_cache/images/`.  Each processed variant is stored
/// with a content-hash filename so that unchanged sources are not
/// reprocessed.
pub struct ImageCache {
    cache_dir: PathBuf,
}

impl ImageCache {
    pub fn open(project_root: &Path) -> Result<Self> {
        let cache_dir = project_root.join(".eigen_cache").join("images");
        std::fs::create_dir_all(&cache_dir)
            .wrap_err_with(|| format!("Failed to create image cache dir {}", cache_dir.display()))?;
        Ok(Self { cache_dir })
    }

    /// Return the cache path for a given key (hash + variant descriptor).
    fn variant_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(key)
    }

    /// Check if a cached variant exists and return its bytes.
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let path = self.variant_path(key);
        std::fs::read(&path).ok()
    }

    /// Store variant bytes under the given key.
    pub fn put(&self, key: &str, data: &[u8]) -> Result<()> {
        let path = self.variant_path(key);
        std::fs::write(&path, data)
            .wrap_err_with(|| format!("Failed to write image cache entry {}", path.display()))?;
        Ok(())
    }
}

/// Hash the source image bytes to create a stable cache key prefix.
fn source_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    result[..8]
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}

/// Determine the `image::ImageFormat` and MIME type for a format name.
fn format_info(name: &str) -> Option<(image::ImageFormat, &'static str)> {
    match name.to_lowercase().as_str() {
        "webp" => Some((image::ImageFormat::WebP, "image/webp")),
        "avif" => Some((image::ImageFormat::Avif, "image/avif")),
        "jpeg" | "jpg" => Some((image::ImageFormat::Jpeg, "image/jpeg")),
        "png" => Some((image::ImageFormat::Png, "image/png")),
        _ => None,
    }
}

/// Guess the original format from a file extension.
fn guess_format_from_ext(ext: &str) -> Option<(image::ImageFormat, &'static str, &'static str)> {
    match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" => Some((image::ImageFormat::Jpeg, "image/jpeg", "jpeg")),
        "png" => Some((image::ImageFormat::Png, "image/png", "png")),
        "webp" => Some((image::ImageFormat::WebP, "image/webp", "webp")),
        "avif" => Some((image::ImageFormat::Avif, "image/avif", "avif")),
        "bmp" => Some((image::ImageFormat::Bmp, "image/bmp", "bmp")),
        "tiff" | "tif" => Some((image::ImageFormat::Tiff, "image/tiff", "tiff")),
        _ => None,
    }
}

/// Check whether a path should be excluded from optimization.
pub fn is_excluded(path: &str, exclude_patterns: &[String]) -> bool {
    for pattern in exclude_patterns {
        if let Ok(glob_pattern) = glob::Pattern::new(pattern) {
            if glob_pattern.matches(path) {
                return true;
            }
        }
    }
    false
}

/// Process a single image: generate all configured format × width variants.
///
/// `src_path` is the path on disk (e.g. `dist/assets/photo.jpg` or
/// `dist/images/hero.png`).
///
/// `url_prefix` is the URL directory prefix (e.g. `/assets`).
///
/// Returns `None` if the image cannot be decoded (e.g. unsupported format).
pub fn optimize_image(
    src_path: &Path,
    url_prefix: &str,
    config: &ImageOptimConfig,
    cache: &ImageCache,
    dist_dir: &Path,
) -> Result<Option<ImageVariants>> {
    let src_data = std::fs::read(src_path)
        .wrap_err_with(|| format!("Failed to read image {}", src_path.display()))?;

    // Determine original format from extension.
    let ext = src_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let (_orig_img_format, _orig_mime, orig_format_name) = match guess_format_from_ext(ext) {
        Some(info) => info,
        None => {
            tracing::debug!("  Skipping unsupported image format: {}", src_path.display());
            return Ok(None);
        }
    };

    // Decode the image.
    let img = match image::ImageReader::new(Cursor::new(&src_data))
        .with_guessed_format()
        .wrap_err("Failed to guess image format")?
        .decode()
    {
        Ok(img) => img,
        Err(e) => {
            tracing::warn!("  Cannot decode image {}: {}", src_path.display(), e);
            return Ok(None);
        }
    };

    let orig_width = img.width();
    let orig_height = img.height();
    let hash = source_hash(&src_data);

    // Stem for naming variants.
    let stem = src_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");

    let mut by_format: HashMap<String, Vec<ImageVariant>> = HashMap::new();

    // Collect all widths we'll generate (only widths < original, plus original).
    let mut widths: Vec<u32> = config
        .widths
        .iter()
        .copied()
        .filter(|&w| w < orig_width)
        .collect();
    widths.push(orig_width);
    widths.sort();
    widths.dedup();

    // Collect all output formats: configured formats + original.
    let mut output_formats: Vec<String> = config.formats.clone();
    if !output_formats.contains(&orig_format_name.to_string()) {
        output_formats.push(orig_format_name.to_string());
    }

    for fmt_name in &output_formats {
        let (img_format, mime_type) = match format_info(fmt_name) {
            Some(info) => info,
            None => {
                tracing::warn!("  Unknown image output format '{}', skipping.", fmt_name);
                continue;
            }
        };

        let mut variants = Vec::new();

        for &w in &widths {
            let variant_filename = format!("{}-{}w-{}.{}", stem, w, hash, fmt_name);
            let cache_key = &variant_filename;

            // Determine output path.
            let out_path = dist_dir.join(url_prefix.trim_start_matches('/')).join(&variant_filename);
            let variant_url = format!("{}/{}", url_prefix, variant_filename);

            // Check cache first.
            if let Some(cached_data) = cache.get(cache_key) {
                write_variant_file(&out_path, &cached_data)?;
            } else {
                // Resize if needed.
                let resized = if w < orig_width {
                    img.resize(w, u32::MAX, image::imageops::FilterType::Lanczos3)
                } else {
                    img.clone()
                };

                let encoded = encode_image(&resized, img_format, config.quality)?;
                cache.put(cache_key, &encoded)?;
                write_variant_file(&out_path, &encoded)?;
            }

            variants.push(ImageVariant {
                url_path: variant_url,
                width: w,
                mime_type: mime_type.to_string()
            });
        }

        variants.sort_by_key(|v| v.width);
        by_format.insert(fmt_name.clone(), variants);
    }

    Ok(Some(ImageVariants {
        original_width: orig_width,
        original_height: orig_height,
        by_format,
        original_format: orig_format_name.to_string(),
    }))
}

/// Encode a `DynamicImage` to bytes in the given format.
fn encode_image(img: &DynamicImage, format: image::ImageFormat, quality: u8) -> Result<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::new();

    match format {
        image::ImageFormat::Jpeg => {
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut buf,
                quality,
            );
            img.write_with_encoder(encoder)
                .wrap_err("Failed to encode JPEG")?;
        }
        image::ImageFormat::WebP => {
            // The image crate's WebP encoder only supports lossless encoding.
            // We use it as-is — it achieves better compression than PNG.
            let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut buf);
            img.write_with_encoder(encoder)
                .wrap_err("Failed to encode WebP")?;
        }
        image::ImageFormat::Avif => {
            let encoder = image::codecs::avif::AvifEncoder::new_with_speed_quality(
                &mut buf,
                10, // fastest speed
                quality,
            );
            img.write_with_encoder(encoder)
                .wrap_err("Failed to encode AVIF")?;
        }
        image::ImageFormat::Png => {
            img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
                .wrap_err("Failed to encode PNG")?;
        }
        _ => {
            img.write_to(&mut Cursor::new(&mut buf), format)
                .wrap_err_with(|| format!("Failed to encode image as {:?}", format))?;
        }
    }

    Ok(buf)
}

/// Write variant bytes to the dist output path, creating parent dirs.
fn write_variant_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .wrap_err_with(|| format!("Failed to create dir {}", parent.display()))?;
    }
    std::fs::write(path, data)
        .wrap_err_with(|| format!("Failed to write image variant {}", path.display()))?;
    Ok(())
}

/// Resolve a URL path (e.g. `/assets/photo.jpg`) to a filesystem path under dist.
pub fn url_to_dist_path(url_path: &str, dist_dir: &Path) -> PathBuf {
    let rel = url_path.trim_start_matches('/');
    dist_dir.join(rel)
}

/// Get the URL directory prefix from a URL path.
/// e.g. `/assets/photo.jpg` → `/assets`.
pub fn url_dir_prefix(url_path: &str) -> String {
    if let Some(pos) = url_path.rfind('/') {
        url_path[..pos].to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_excluded_svg() {
        let patterns = vec!["**/*.svg".to_string(), "**/*.gif".to_string()];
        assert!(is_excluded("images/logo.svg", &patterns));
        assert!(is_excluded("deep/path/anim.gif", &patterns));
        assert!(!is_excluded("photos/hero.jpg", &patterns));
    }

    #[test]
    fn test_is_excluded_specific_path() {
        let patterns = vec!["static/favicons/*".to_string()];
        assert!(is_excluded("static/favicons/icon.png", &patterns));
        assert!(!is_excluded("static/images/icon.png", &patterns));
    }

    #[test]
    fn test_source_hash_deterministic() {
        let data = b"hello world";
        assert_eq!(source_hash(data), source_hash(data));
        assert_eq!(source_hash(data).len(), 16);
    }

    #[test]
    fn test_source_hash_different_data() {
        assert_ne!(source_hash(b"aaa"), source_hash(b"bbb"));
    }

    #[test]
    fn test_format_info() {
        assert!(format_info("webp").is_some());
        assert!(format_info("avif").is_some());
        assert!(format_info("jpeg").is_some());
        assert!(format_info("jpg").is_some());
        assert!(format_info("png").is_some());
        assert!(format_info("bmp").is_none()); // not in our target formats
    }

    #[test]
    fn test_guess_format_from_ext() {
        let (_, _, name) = guess_format_from_ext("jpg").unwrap();
        assert_eq!(name, "jpeg");
        let (_, _, name) = guess_format_from_ext("PNG").unwrap();
        assert_eq!(name, "png");
        assert!(guess_format_from_ext("svg").is_none());
    }

    #[test]
    fn test_url_dir_prefix() {
        assert_eq!(url_dir_prefix("/assets/photo.jpg"), "/assets");
        assert_eq!(url_dir_prefix("/a/b/c.png"), "/a/b");
        assert_eq!(url_dir_prefix("photo.jpg"), "");
    }

    #[test]
    fn test_url_to_dist_path() {
        let dist = Path::new("/project/dist");
        assert_eq!(
            url_to_dist_path("/assets/photo.jpg", dist),
            PathBuf::from("/project/dist/assets/photo.jpg")
        );
    }

    #[test]
    fn test_optimize_real_image() {
        // Create a small test image in memory and write to a temp file.
        let tmp = tempfile::TempDir::new().unwrap();
        let dist_dir = tmp.path().join("dist");
        std::fs::create_dir_all(dist_dir.join("assets")).unwrap();

        // Create a 100x50 red JPEG.
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_fn(100, 50, |_, _| {
            image::Rgb([255, 0, 0])
        }));
        let src_path = dist_dir.join("assets/test.jpg");
        img.save(&src_path).unwrap();

        let config = ImageOptimConfig {
            optimize: true,
            formats: vec!["webp".to_string()],
            quality: 75,
            widths: vec![48, 80, 200], // 200 > 100, should be clamped
            exclude: vec![],
        };

        let cache = ImageCache::open(tmp.path()).unwrap();

        let result = optimize_image(
            &src_path,
            "/assets",
            &config,
            &cache,
            &dist_dir,
        )
        .unwrap()
        .unwrap();

        assert_eq!(result.original_width, 100);
        assert_eq!(result.original_height, 50);
        assert_eq!(result.original_format, "jpeg");

        // Should have webp + jpeg (original) formats.
        assert!(result.by_format.contains_key("webp"));
        assert!(result.by_format.contains_key("jpeg"));

        // WebP variants: 48w, 80w, 100w (original) — 200 was > original so dropped.
        let webp_variants = &result.by_format["webp"];
        let widths: Vec<u32> = webp_variants.iter().map(|v| v.width).collect();
        assert_eq!(widths, vec![48, 80, 100]);

        // Verify files exist on disk.
        for variant in webp_variants {
            let path = url_to_dist_path(&variant.url_path, &dist_dir);
            assert!(path.exists(), "Missing variant file: {}", path.display());
        }
    }

    #[test]
    fn test_optimize_image_cache_hit() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dist_dir = tmp.path().join("dist");
        std::fs::create_dir_all(dist_dir.join("assets")).unwrap();

        let img = DynamicImage::ImageRgb8(image::RgbImage::from_fn(60, 40, |_, _| {
            image::Rgb([0, 255, 0])
        }));
        let src_path = dist_dir.join("assets/green.jpg");
        img.save(&src_path).unwrap();

        let config = ImageOptimConfig {
            optimize: true,
            formats: vec!["webp".to_string()],
            quality: 75,
            widths: vec![30],
            exclude: vec![],
        };

        let cache = ImageCache::open(tmp.path()).unwrap();

        // First run — populates cache.
        let r1 = optimize_image(&src_path, "/assets", &config, &cache, &dist_dir)
            .unwrap().unwrap();

        // Delete dist files to prove cache is used on second run.
        for variants in r1.by_format.values() {
            for v in variants {
                let p = url_to_dist_path(&v.url_path, &dist_dir);
                if p.exists() {
                    std::fs::remove_file(&p).ok();
                }
            }
        }

        // Second run — should restore from cache.
        let r2 = optimize_image(&src_path, "/assets", &config, &cache, &dist_dir)
            .unwrap().unwrap();

        for variants in r2.by_format.values() {
            for v in variants {
                let p = url_to_dist_path(&v.url_path, &dist_dir);
                assert!(p.exists(), "Cached variant not restored: {}", p.display());
            }
        }
    }
}
