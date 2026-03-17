//! CSS extraction: parse stylesheets, match selectors against HTML DOM,
//! collect critical rules with transitive dependencies.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

// Pre-compiled regex patterns (compiled once, reused across all calls).
// These are called per-selector and per-rule, so avoiding recompilation
// is critical for performance on large stylesheets.

/// Matches CSS pseudo-elements like `::before`, `::after`, `::placeholder`.
static PSEUDO_ELEMENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"::[a-zA-Z-]+(?:\([^)]*\))?").expect("pseudo-element regex is valid")
});

/// Structural / functional pseudo-classes that `scraper` supports natively.
/// These are preserved during stripping so selector matching works correctly.
const STRUCTURAL_PSEUDOS: &[&str] = &[
    "root", "first-child", "last-child", "nth-child", "nth-last-child",
    "nth-of-type", "nth-last-of-type", "first-of-type", "last-of-type",
    "only-child", "only-of-type", "empty", "not", "is", "where", "has",
];

/// Matches `font-family` declarations.
static FONT_FAMILY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)font-family\s*:\s*([^;]+)"#).expect("font-family regex is valid")
});

/// Matches `animation-name` declarations.
static ANIM_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)animation-name\s*:\s*([^;]+)"#).expect("animation-name regex is valid")
});

/// Matches `animation` shorthand declarations.
static ANIM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)animation\s*:\s*([^;]+)").expect("animation regex is valid")
});

/// Matches `var(--name)` references.
static VAR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"var\(\s*(--[a-zA-Z0-9_-]+)").expect("var() regex is valid")
});

/// Matches CSS custom property definitions (`--name:`).
static CUSTOM_PROP_DEF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(--[a-zA-Z0-9_-]+)\s*:").expect("custom prop regex is valid")
});

/// Strip dynamic pseudo-classes and pseudo-elements from a selector string
/// to produce a version matchable against a static DOM.
///
/// Returns `Some(stripped)` if the result is non-empty, or `None` if stripping
/// produces an empty selector (e.g. `::selection`) -- meaning the rule should
/// be included unconditionally.
///
/// Structural pseudo-classes (`:root`, `:first-child`, `:nth-child`, etc.) are
/// preserved because `scraper` supports them natively.
pub fn strip_pseudo_for_matching(selector: &str) -> Option<String> {
    // First strip pseudo-elements (::before, ::after, etc.) unconditionally.
    let after_elements = PSEUDO_ELEMENT_RE.replace_all(selector, "");

    // Then strip dynamic pseudo-classes while preserving structural ones.
    // We cannot use a single regex with look-ahead (not supported by regex crate),
    // so we use a match-and-check approach.
    let mut result = String::with_capacity(after_elements.len());
    let bytes = after_elements.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b':' && (i + 1 >= len || bytes[i + 1] != b':') {
            // Single colon -- this is a pseudo-class. Extract the name.
            let start = i;
            i += 1; // skip ':'
            let name_start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
                i += 1;
            }
            let name = &after_elements[name_start..i];

            if STRUCTURAL_PSEUDOS.iter().any(|&s| s == name) {
                // Preserve this pseudo-class (including any parenthesized args).
                let mut end = i;
                if end < len && bytes[end] == b'(' {
                    // Find matching close paren.
                    let mut depth = 1;
                    end += 1;
                    while end < len && depth > 0 {
                        if bytes[end] == b'(' { depth += 1; }
                        if bytes[end] == b')' { depth -= 1; }
                        end += 1;
                    }
                }
                result.push_str(&after_elements[start..end]);
                i = end;
            } else {
                // Dynamic pseudo-class -- strip it (including any parenthesized args).
                if i < len && bytes[i] == b'(' {
                    let mut depth = 1;
                    i += 1;
                    while i < len && depth > 0 {
                        if bytes[i] == b'(' { depth += 1; }
                        if bytes[i] == b')' { depth -= 1; }
                        i += 1;
                    }
                }
                // Stripped: don't append anything.
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    // Collapse any leftover whitespace.
    let result = result.split_whitespace().collect::<Vec<_>>().join(" ");
    let result = result.trim().to_string();

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Test whether a CSS selector matches any element in the HTML document.
///
/// If the selector cannot be parsed by scraper, returns `true` (conservative:
/// include the rule rather than risk dropping it).
pub fn selector_matches(selector: &str, document: &scraper::Html) -> bool {
    match scraper::Selector::parse(selector) {
        Ok(sel) => document.select(&sel).next().is_some(),
        Err(_) => {
            tracing::debug!("Could not parse selector for matching: {}", selector);
            true // conservative: include the rule
        }
    }
}

/// Tracks which global rules (font-face, keyframes, custom properties)
/// are transitively referenced by matched style rules.
#[derive(Debug, Default)]
pub struct GlobalDependencies {
    /// Font family names referenced by matched rules.
    pub font_families: HashSet<String>,
    /// Animation names referenced by matched rules.
    pub animation_names: HashSet<String>,
    /// CSS custom property names (e.g. "--color-primary") referenced
    /// via var() in matched rules.
    pub custom_properties: HashSet<String>,
}

/// Scan a CSS declaration block (serialized text) for references to font
/// families, animation names, and CSS custom properties.
pub fn collect_global_deps(declarations: &str) -> GlobalDependencies {
    let mut deps = GlobalDependencies::default();

    // Font family: match `font-family:` property values.
    // Extract quoted or unquoted family names.
    if let Some(cap) = FONT_FAMILY_RE.captures(declarations) {
        let families = &cap[1];
        for family in families.split(',') {
            let name = family.trim().trim_matches('"').trim_matches('\'').trim();
            if !name.is_empty() && !is_generic_font_family(name) {
                deps.font_families.insert(name.to_string());
            }
        }
    }

    // Animation names: match `animation-name:` declaration.
    if let Some(cap) = ANIM_NAME_RE.captures(declarations) {
        let names = &cap[1];
        for name in names.split(',') {
            let name = name.trim();
            if !name.is_empty() && name != "none" && name != "initial" && name != "inherit" {
                deps.animation_names.insert(name.to_string());
            }
        }
    }

    // Also check `animation:` shorthand -- animation name is typically the
    // first non-numeric, non-timing-function, non-keyword token.
    if let Some(cap) = ANIM_RE.captures(declarations) {
        // Only process if animation-name wasn't already found.
        if deps.animation_names.is_empty() {
            extract_animation_names_from_shorthand(&cap[1], &mut deps.animation_names);
        }
    }

    // CSS custom properties: match `var(--name)` references.
    for cap in VAR_RE.captures_iter(declarations) {
        deps.custom_properties.insert(cap[1].to_string());
    }

    deps
}

/// Check if a font family name is a generic CSS family keyword.
fn is_generic_font_family(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "serif" | "sans-serif" | "monospace" | "cursive" | "fantasy"
        | "system-ui" | "ui-serif" | "ui-sans-serif" | "ui-monospace"
        | "ui-rounded" | "emoji" | "math" | "fangsong"
        | "inherit" | "initial" | "unset" | "revert"
    )
}

/// Extract animation names from the `animation` shorthand value.
///
/// The shorthand format is: `name duration timing-function delay ...`
/// We look for identifiers that are not timing keywords or durations.
fn extract_animation_names_from_shorthand(value: &str, names: &mut HashSet<String>) {
    let timing_keywords: HashSet<&str> = [
        "ease", "ease-in", "ease-out", "ease-in-out", "linear",
        "step-start", "step-end", "infinite", "none", "normal",
        "reverse", "alternate", "alternate-reverse", "forwards",
        "backwards", "both", "running", "paused", "initial",
        "inherit", "unset",
    ].into_iter().collect();

    // Each comma-separated value is one animation.
    for animation in value.split(',') {
        for token in animation.split_whitespace() {
            let token = token.trim();
            // Skip durations (e.g. "1s", "200ms", "0.5s").
            if token.ends_with('s') || token.ends_with("ms") {
                continue;
            }
            // Skip numeric values.
            if token.parse::<f64>().is_ok() {
                continue;
            }
            // Skip timing keywords.
            if timing_keywords.contains(token.to_lowercase().as_str()) {
                continue;
            }
            // Skip cubic-bezier/steps functions.
            if token.starts_with("cubic-bezier") || token.starts_with("steps(") {
                continue;
            }
            // This is likely the animation name.
            if !token.is_empty() {
                names.insert(token.to_string());
                break; // Only one name per animation value.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- strip_pseudo_for_matching tests ---

    #[test]
    fn test_strip_pseudo_hover() {
        assert_eq!(
            strip_pseudo_for_matching(".btn:hover"),
            Some(".btn".to_string())
        );
    }

    #[test]
    fn test_strip_pseudo_before() {
        assert_eq!(
            strip_pseudo_for_matching(".icon::before"),
            Some(".icon".to_string())
        );
    }

    #[test]
    fn test_strip_pseudo_preserves_structural() {
        // :root should NOT be stripped.
        assert_eq!(
            strip_pseudo_for_matching(":root"),
            Some(":root".to_string())
        );
        assert_eq!(
            strip_pseudo_for_matching(":first-child"),
            Some(":first-child".to_string())
        );
        assert_eq!(
            strip_pseudo_for_matching(":nth-child(2n)"),
            Some(":nth-child(2n)".to_string())
        );
        assert_eq!(
            strip_pseudo_for_matching(":not(.hidden)"),
            Some(":not(.hidden)".to_string())
        );
    }

    #[test]
    fn test_strip_pseudo_selection() {
        // ::selection with no base selector -> None (include unconditionally).
        assert_eq!(strip_pseudo_for_matching("::selection"), None);
    }

    #[test]
    fn test_strip_pseudo_compound() {
        assert_eq!(
            strip_pseudo_for_matching("a:hover > .icon::after"),
            Some("a > .icon".to_string())
        );
    }

    #[test]
    fn test_strip_pseudo_focus_visible() {
        assert_eq!(
            strip_pseudo_for_matching(".input:focus-visible"),
            Some(".input".to_string())
        );
    }

    #[test]
    fn test_strip_pseudo_visited() {
        assert_eq!(
            strip_pseudo_for_matching("a:visited"),
            Some("a".to_string())
        );
    }

    #[test]
    fn test_strip_pseudo_placeholder() {
        assert_eq!(
            strip_pseudo_for_matching("input::placeholder"),
            Some("input".to_string())
        );
    }

    // --- selector_matches tests ---

    #[test]
    fn test_selector_matches_basic() {
        let html = scraper::Html::parse_document(r#"<div class="exists">Hello</div>"#);
        assert!(selector_matches(".exists", &html));
    }

    #[test]
    fn test_selector_matches_absent() {
        let html = scraper::Html::parse_document(r#"<div class="exists">Hello</div>"#);
        assert!(!selector_matches(".missing", &html));
    }

    #[test]
    fn test_selector_matches_tag() {
        let html = scraper::Html::parse_document("<p>Hello</p>");
        assert!(selector_matches("p", &html));
        assert!(!selector_matches("span", &html));
    }

    #[test]
    fn test_selector_matches_id() {
        let html = scraper::Html::parse_document(r#"<div id="main">Hello</div>"#);
        assert!(selector_matches("#main", &html));
        assert!(!selector_matches("#other", &html));
    }

    #[test]
    fn test_selector_matches_combinator() {
        let html = scraper::Html::parse_document(
            r#"<div class="parent"><span class="child">Hi</span></div>"#,
        );
        assert!(selector_matches(".parent > .child", &html));
        assert!(!selector_matches(".parent > .other", &html));
    }

    #[test]
    fn test_selector_matches_unparseable() {
        // An unparseable selector should return true (conservative: include the rule).
        let html = scraper::Html::parse_document("<div>Hello</div>");
        assert!(selector_matches("!!invalid%%selector", &html));
    }

    // --- collect_global_deps tests ---

    #[test]
    fn test_collect_font_family() {
        let decls = r#"font-family: "Inter", sans-serif;"#;
        let deps = collect_global_deps(decls);
        assert!(deps.font_families.contains("Inter"));
        assert!(!deps.font_families.contains("sans-serif"));
    }

    #[test]
    fn test_collect_animation_name() {
        let decls = "animation-name: spin;";
        let deps = collect_global_deps(decls);
        assert!(deps.animation_names.contains("spin"));
    }

    #[test]
    fn test_collect_animation_shorthand() {
        let decls = "animation: fadeIn 0.3s ease-in-out;";
        let deps = collect_global_deps(decls);
        assert!(deps.animation_names.contains("fadeIn"));
    }

    #[test]
    fn test_collect_var_references() {
        let decls = "color: var(--color-primary); background: var(--bg-main);";
        let deps = collect_global_deps(decls);
        assert!(deps.custom_properties.contains("--color-primary"));
        assert!(deps.custom_properties.contains("--bg-main"));
    }

    #[test]
    fn test_collect_no_deps() {
        let decls = "color: red; margin: 0;";
        let deps = collect_global_deps(decls);
        assert!(deps.font_families.is_empty());
        assert!(deps.animation_names.is_empty());
        assert!(deps.custom_properties.is_empty());
    }
}
