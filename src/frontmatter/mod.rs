use eyre::{Result, WrapErr};
use serde::Deserialize;
use std::collections::HashMap;

/// A parsed frontmatter block from a template file.
#[derive(Debug, Clone)]
pub struct Frontmatter {
    /// If present, this page is dynamic — the collection query provides the
    /// list of items to iterate over.
    pub collection: Option<DataQuery>,
    /// Which field on each collection item to use as the URL slug.
    /// Defaults to `"slug"`.
    pub slug_field: String,
    /// The variable name each collection item is exposed as in the template.
    /// Defaults to `"item"`.
    pub item_as: String,
    /// Named data queries whose results are injected into the template context.
    pub data: HashMap<String, DataQuery>,
    /// Which template blocks to extract as fragments (overrides the default
    /// `content_block` from build config).
    pub fragment_blocks: Option<Vec<String>>,
    /// Path to the hero/LCP image for this page.
    ///
    /// When set, a `<link rel="preload">` hint is injected into `<head>`
    /// for this image, improving Largest Contentful Paint.
    /// The path should be relative to the site root (e.g. "/assets/hero.jpg").
    pub hero_image: Option<String>,
    /// SEO metadata for Open Graph and Twitter Card tags.
    pub seo: SeoMeta,
}

impl Default for Frontmatter {
    fn default() -> Self {
        Self {
            collection: None,
            slug_field: "slug".into(),
            item_as: "item".into(),
            data: HashMap::new(),
            fragment_blocks: None,
            hero_image: None,
            seo: SeoMeta::default(),
        }
    }
}

/// Describes where and how to fetch a piece of data.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DataQuery {
    /// Path to a local file in `_data/`, e.g. `"nav.yaml"`.
    pub file: Option<String>,
    /// Name of a source defined in `[sources.*]` in `site.toml`.
    pub source: Option<String>,
    /// URL path appended to the source's base URL.
    pub path: Option<String>,
    /// Dot-separated path into the response JSON to extract the data from,
    /// e.g. `"data.posts"`.
    pub root: Option<String>,
    /// Sort specification: `"field"` for ascending, `"-field"` for descending.
    pub sort: Option<String>,
    /// Maximum number of items to return.
    pub limit: Option<usize>,
    /// Key-value filters: only keep items where `item[key] == value`.
    /// Values may contain `{{ item.field }}` for interpolation in dynamic pages.
    pub filter: Option<HashMap<String, String>>,
}

/// Per-page SEO metadata for Open Graph and Twitter Card tags.
///
/// All fields are optional. When absent, site-level defaults from
/// `[site.seo]` in site.toml are used.
///
/// For dynamic pages, field values may contain minijinja template
/// expressions (e.g. `{{ post.title }}`) which are resolved per-item
/// during rendering.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SeoMeta {
    /// Page title for `og:title` and `twitter:title`.
    /// Falls back to `site.seo.title`, then `site.name`.
    pub title: Option<String>,

    /// Page description for `og:description` and `twitter:description`.
    /// Falls back to `site.seo.description`.
    pub description: Option<String>,

    /// Share image URL for `og:image` and `twitter:image`.
    /// Can be a site-relative path (e.g. "/assets/hero.jpg") or
    /// absolute URL. Relative paths are resolved to absolute URLs
    /// using `site.base_url` during injection.
    /// Falls back to `site.seo.image`.
    pub image: Option<String>,

    /// Open Graph type for `og:type`.
    /// Falls back to `site.seo.og_type`, then "website".
    pub og_type: Option<String>,

    /// Twitter card type for `twitter:card`.
    /// Falls back to `site.seo.twitter_card`, then
    /// "summary_large_image". Forced to "summary" when no image
    /// is available at any level.
    pub twitter_card: Option<String>,

    /// Override the canonical URL. By default, this is auto-generated
    /// from `site.base_url` + page URL path.
    pub canonical_url: Option<String>,
}

// ---------------------------------------------------------------------------
// Raw serde types for YAML deserialization (before mapping to the public types)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawFrontmatter {
    collection: Option<DataQuery>,
    slug_field: Option<String>,
    item_as: Option<String>,
    #[serde(default)]
    data: HashMap<String, DataQuery>,
    fragment_blocks: Option<Vec<String>>,
    hero_image: Option<String>,
    #[serde(default)]
    seo: SeoMeta,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Split a template file's content into its optional raw YAML frontmatter
/// and the template body (with frontmatter stripped).
///
/// Frontmatter is delimited by `---` lines at the very start of the file:
///
/// ```text
/// ---
/// collection:
///   source: blog_api
///   path: /posts
/// ---
/// <html>...
/// ```
///
/// If the file does not start with `---`, the entire content is returned as
/// the body with `None` for the raw YAML.
pub fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (None, content);
    }

    // Skip the opening `---` plus its trailing newline.
    let after_open = &trimmed[3..];
    let after_open = after_open
        .strip_prefix('\n')
        .or_else(|| after_open.strip_prefix("\r\n"))
        .unwrap_or(after_open);

    // The closing `---` can appear at the very start (empty frontmatter)
    // or after a newline.
    if after_open.starts_with("---") {
        let rest = &after_open[3..];
        let rest = rest
            .strip_prefix('\n')
            .or_else(|| rest.strip_prefix("\r\n"))
            .unwrap_or(rest);
        return (Some(""), rest);
    }

    if let Some(pos) = after_open.find("\n---") {
        let yaml = &after_open[..pos];
        let after_close = &after_open[pos + 4..]; // skip the `\n---`
        let body = after_close
            .strip_prefix('\n')
            .or_else(|| after_close.strip_prefix("\r\n"))
            .unwrap_or(after_close);
        (Some(yaml), body)
    } else {
        // No closing delimiter — treat entire file as body (no frontmatter).
        (None, content)
    }
}

/// Parse a raw YAML frontmatter string into a [`Frontmatter`] struct.
///
/// `file_path` is used only for error messages.
pub fn parse_frontmatter(raw_yaml: &str, file_path: &str) -> Result<Frontmatter> {
    let raw: RawFrontmatter = serde_yaml::from_str(raw_yaml)
        .wrap_err_with(|| format!("Failed to parse frontmatter YAML in {file_path}"))?;

    Ok(Frontmatter {
        collection: raw.collection,
        slug_field: raw.slug_field.unwrap_or_else(|| "slug".into()),
        item_as: raw.item_as.unwrap_or_else(|| "item".into()),
        data: raw.data,
        fragment_blocks: raw.fragment_blocks,
        hero_image: raw.hero_image,
        seo: raw.seo,
    })
}

/// Convenience: extract and parse frontmatter from a template file's full
/// content. Returns the parsed [`Frontmatter`] (or a default if none present)
/// and the template body.
pub fn extract_frontmatter<'a>(
    content: &'a str,
    file_path: &str,
) -> Result<(Frontmatter, &'a str)> {
    let (raw_yaml, body) = split_frontmatter(content);
    match raw_yaml {
        Some(yaml) => {
            let fm = parse_frontmatter(yaml, file_path)?;
            Ok((fm, body))
        }
        None => Ok((Frontmatter::default(), body)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- split_frontmatter tests ---

    #[test]
    fn test_split_no_frontmatter() {
        let content = "<html><body>Hello</body></html>";
        let (yaml, body) = split_frontmatter(content);
        assert!(yaml.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn test_split_with_frontmatter() {
        let content = "---\ntitle: Hello\n---\n<html>body</html>";
        let (yaml, body) = split_frontmatter(content);
        assert_eq!(yaml.unwrap(), "title: Hello");
        assert_eq!(body, "<html>body</html>");
    }

    #[test]
    fn test_split_multiline_frontmatter() {
        let content =
            "---\ncollection:\n  source: api\n  path: /posts\nslug_field: id\n---\n<div>tmpl</div>";
        let (yaml, body) = split_frontmatter(content);
        let yaml = yaml.unwrap();
        assert!(yaml.contains("collection:"));
        assert!(yaml.contains("slug_field: id"));
        assert_eq!(body, "<div>tmpl</div>");
    }

    #[test]
    fn test_split_no_closing_delimiter() {
        let content = "---\ntitle: Hello\nno closing";
        let (yaml, body) = split_frontmatter(content);
        assert!(yaml.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn test_split_empty_frontmatter() {
        let content = "---\n---\n<html></html>";
        let (yaml, body) = split_frontmatter(content);
        assert_eq!(yaml.unwrap(), "");
        assert_eq!(body, "<html></html>");
    }

    // --- parse_frontmatter tests ---

    #[test]
    fn test_parse_static_page_frontmatter() {
        let yaml = concat!(
            "data:\n",
            "  nav:\n",
            "    file: \"nav.yaml\"\n",
            "  recent_posts:\n",
            "    source: blog_api\n",
            "    path: /posts\n",
            "    root: data.posts\n",
            "    sort: \"-date\"\n",
            "    limit: 5\n",
        );
        let fm = parse_frontmatter(yaml, "index.html").unwrap();
        assert!(fm.collection.is_none());
        assert_eq!(fm.slug_field, "slug");
        assert_eq!(fm.item_as, "item");
        assert_eq!(fm.data.len(), 2);

        let nav = &fm.data["nav"];
        assert_eq!(nav.file.as_deref(), Some("nav.yaml"));

        let posts = &fm.data["recent_posts"];
        assert_eq!(posts.source.as_deref(), Some("blog_api"));
        assert_eq!(posts.path.as_deref(), Some("/posts"));
        assert_eq!(posts.root.as_deref(), Some("data.posts"));
        assert_eq!(posts.sort.as_deref(), Some("-date"));
        assert_eq!(posts.limit, Some(5));
    }

    #[test]
    fn test_parse_dynamic_page_frontmatter() {
        let yaml = concat!(
            "collection:\n",
            "  source: blog_api\n",
            "  path: /posts\n",
            "  root: data.posts\n",
            "slug_field: slug\n",
            "item_as: post\n",
            "data:\n",
            "  author:\n",
            "    source: blog_api\n",
            "    path: /authors\n",
            "    filter:\n",
            "      id: \"{{ post.author_id }}\"\n",
            "    root: data.authors\n",
            "fragment_blocks:\n",
            "  - content\n",
            "  - sidebar\n",
        );
        let fm = parse_frontmatter(yaml, "posts/[post].html").unwrap();
        assert!(fm.collection.is_some());
        let coll = fm.collection.as_ref().unwrap();
        assert_eq!(coll.source.as_deref(), Some("blog_api"));
        assert_eq!(coll.path.as_deref(), Some("/posts"));
        assert_eq!(coll.root.as_deref(), Some("data.posts"));

        assert_eq!(fm.slug_field, "slug");
        assert_eq!(fm.item_as, "post");

        let author_q = &fm.data["author"];
        let filter = author_q.filter.as_ref().unwrap();
        assert_eq!(filter["id"], "{{ post.author_id }}");

        let blocks = fm.fragment_blocks.as_ref().unwrap();
        assert_eq!(blocks, &["content", "sidebar"]);
    }

    #[test]
    fn test_parse_missing_frontmatter_defaults() {
        let fm = Frontmatter::default();
        assert!(fm.collection.is_none());
        assert_eq!(fm.slug_field, "slug");
        assert_eq!(fm.item_as, "item");
        assert!(fm.data.is_empty());
        assert!(fm.fragment_blocks.is_none());
    }

    #[test]
    fn test_parse_malformed_yaml() {
        let yaml = "collection: [invalid yaml\n  broken:";
        let result = parse_frontmatter(yaml, "bad.html");
        assert!(result.is_err());
        let err = format!("{:#}", result.unwrap_err());
        assert!(err.contains("bad.html"));
    }

    // --- extract_frontmatter (integration) tests ---

    #[test]
    fn test_extract_with_frontmatter() {
        let content = "---\ndata:\n  nav:\n    file: \"nav.yaml\"\n---\n<html>{{ nav }}</html>";
        let (fm, body) = extract_frontmatter(content, "index.html").unwrap();
        assert_eq!(fm.data.len(), 1);
        assert_eq!(body, "<html>{{ nav }}</html>");
    }

    #[test]
    fn test_extract_without_frontmatter() {
        let content = "<html>No frontmatter here</html>";
        let (fm, body) = extract_frontmatter(content, "plain.html").unwrap();
        assert!(fm.collection.is_none());
        assert!(fm.data.is_empty());
        assert_eq!(body, content);
    }

    // --- hero_image frontmatter tests ---

    #[test]
    fn test_parse_hero_image_frontmatter() {
        let yaml = "hero_image: /assets/hero-banner.jpg\n";
        let fm = parse_frontmatter(yaml, "index.html").unwrap();
        assert_eq!(fm.hero_image.as_deref(), Some("/assets/hero-banner.jpg"));
    }

    #[test]
    fn test_parse_no_hero_image() {
        let yaml = "data:\n  nav:\n    file: \"nav.yaml\"\n";
        let fm = parse_frontmatter(yaml, "index.html").unwrap();
        assert!(fm.hero_image.is_none());
    }

    #[test]
    fn test_default_hero_image_is_none() {
        let fm = Frontmatter::default();
        assert!(fm.hero_image.is_none());
    }

    // --- SEO frontmatter tests ---

    #[test]
    fn test_parse_seo_frontmatter_full() {
        let yaml = concat!(
            "seo:\n",
            "  title: \"About Us\"\n",
            "  description: \"Learn about our team\"\n",
            "  image: /assets/about-hero.jpg\n",
            "  og_type: website\n",
            "  twitter_card: summary_large_image\n",
            "  canonical_url: https://example.com/about\n",
        );
        let fm = parse_frontmatter(yaml, "about.html").unwrap();
        assert_eq!(fm.seo.title.as_deref(), Some("About Us"));
        assert_eq!(fm.seo.description.as_deref(), Some("Learn about our team"));
        assert_eq!(fm.seo.image.as_deref(), Some("/assets/about-hero.jpg"));
        assert_eq!(fm.seo.og_type.as_deref(), Some("website"));
        assert_eq!(fm.seo.twitter_card.as_deref(), Some("summary_large_image"));
        assert_eq!(fm.seo.canonical_url.as_deref(), Some("https://example.com/about"));
    }

    #[test]
    fn test_parse_seo_frontmatter_partial() {
        let yaml = concat!(
            "seo:\n",
            "  title: \"My Page\"\n",
            "  description: \"A description\"\n",
        );
        let fm = parse_frontmatter(yaml, "page.html").unwrap();
        assert_eq!(fm.seo.title.as_deref(), Some("My Page"));
        assert_eq!(fm.seo.description.as_deref(), Some("A description"));
        assert!(fm.seo.image.is_none());
        assert!(fm.seo.og_type.is_none());
        assert!(fm.seo.twitter_card.is_none());
        assert!(fm.seo.canonical_url.is_none());
    }

    #[test]
    fn test_parse_seo_frontmatter_absent() {
        let yaml = "data:\n  nav:\n    file: \"nav.yaml\"\n";
        let fm = parse_frontmatter(yaml, "index.html").unwrap();
        assert!(fm.seo.title.is_none());
        assert!(fm.seo.description.is_none());
        assert!(fm.seo.image.is_none());
        assert!(fm.seo.og_type.is_none());
        assert!(fm.seo.twitter_card.is_none());
        assert!(fm.seo.canonical_url.is_none());
    }

    #[test]
    fn test_parse_seo_with_template_expressions() {
        let yaml = concat!(
            "seo:\n",
            "  title: \"{{ post.title }} | My Blog\"\n",
            "  description: \"{{ post.excerpt }}\"\n",
            "  image: \"{{ post.cover_image }}\"\n",
        );
        let fm = parse_frontmatter(yaml, "post.html").unwrap();
        // Expressions are stored as literal strings, not evaluated at parse time.
        assert_eq!(fm.seo.title.as_deref(), Some("{{ post.title }} | My Blog"));
        assert_eq!(fm.seo.description.as_deref(), Some("{{ post.excerpt }}"));
        assert_eq!(fm.seo.image.as_deref(), Some("{{ post.cover_image }}"));
    }

    #[test]
    fn test_default_seo_is_empty() {
        let fm = Frontmatter::default();
        assert!(fm.seo.title.is_none());
        assert!(fm.seo.description.is_none());
        assert!(fm.seo.image.is_none());
        assert!(fm.seo.og_type.is_none());
        assert!(fm.seo.twitter_card.is_none());
        assert!(fm.seo.canonical_url.is_none());
    }
}
