//! Steps 3.3 & 3.4: Nested query interpolation and the high-level data query
//! executor.
//!
//! - **Nested query interpolation**: resolve `{{ item.field }}` patterns in
//!   `DataQuery.filter` values when rendering dynamic pages.
//! - **Query executor**: the main entry point that takes a `Frontmatter` and
//!   an optional current item, resolves all `data` entries, and returns a
//!   context map.

use eyre::{bail, Result, WrapErr};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use tokio::sync::Mutex as AsyncMutex;

static INTERPOLATION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{\s*([A-Za-z_][A-Za-z0-9_.]*)\s*\}\}").unwrap());

static ENV_VAR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap());

static REMAINING_INTERP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{.*?\}\}").unwrap());

use crate::frontmatter::{DataQuery, Frontmatter};
use crate::plugins::registry::PluginRegistry;

use super::fetcher::{
    DataFetcher, SourceCacheCheck, StoreResult, execute_source_request,
};
use super::transforms::apply_transforms;

/// Resolve all `data` queries in a static page's frontmatter.
///
/// Returns a map of query name → resolved value, ready to be merged into the
/// template context.
pub async fn resolve_page_data(
    frontmatter: &Frontmatter,
    fetcher: &mut DataFetcher,
    plugin_registry: Option<&PluginRegistry>,
) -> Result<HashMap<String, Value>> {
    let mut result = HashMap::new();

    for (name, query) in &frontmatter.data {
        let value = fetcher
            .fetch(query, plugin_registry)
            .await
            .wrap_err_with(|| format!("Failed to resolve data query '{}'", name))?;
        result.insert(name.clone(), value);
    }

    Ok(result)
}

/// Resolve all data for a dynamic page: fetch the collection first, then for
/// each item, resolve the `data` queries (with interpolation).
///
/// Returns a tuple of:
/// - The collection items (a `Vec<Value>`)
/// - A function-like closure isn't easy here, so instead we return the raw
///   collection and let the caller iterate, calling `resolve_item_data` for
///   each item.
pub async fn resolve_dynamic_page_data(
    frontmatter: &Frontmatter,
    fetcher: &mut DataFetcher,
    plugin_registry: Option<&PluginRegistry>,
) -> Result<Vec<Value>> {
    let collection_query = frontmatter
        .collection
        .as_ref()
        .ok_or_else(|| eyre::eyre!("Dynamic page has no `collection` in frontmatter"))?;

    let collection = fetcher
        .fetch(collection_query, plugin_registry)
        .await
        .wrap_err("Failed to fetch collection")?;

    match collection {
        Value::Array(items) => Ok(items),
        _ => {
            // Not an array — return empty. The build engine will skip silently.
            Ok(Vec::new())
        }
    }
}

/// Resolve the `data` queries for a single item of a dynamic page.
///
/// Filter values containing `{{ item.field }}` patterns are interpolated using
/// the current item's data. Interpolation is ONE level deep — if an
/// interpolated query itself references `{{ }}`, an error is returned.
pub async fn resolve_item_data(
    frontmatter: &Frontmatter,
    item: &Value,
    item_as: &str,
    fetcher: &mut DataFetcher,
    plugin_registry: Option<&PluginRegistry>,
) -> Result<HashMap<String, Value>> {
    let mut result = HashMap::new();

    for (name, query) in &frontmatter.data {
        let interpolated = interpolate_query(query, item, item_as).wrap_err_with(|| {
            format!(
                "Failed to interpolate data query '{}' for current item",
                name,
            )
        })?;

        // Verify the interpolated query doesn't still contain {{ }} patterns.
        verify_no_remaining_interpolation(&interpolated, name)?;

        let value = fetcher
            .fetch(&interpolated, plugin_registry)
            .await
            .wrap_err_with(|| format!("Failed to resolve data query '{}'", name))?;
        result.insert(name.clone(), value);
    }

    Ok(result)
}

/// Convenience wrapper: resolve data queries for a single item of a dynamic page.
///
/// Uses the frontmatter's `item_as` field as the interpolation prefix.
pub async fn resolve_dynamic_page_data_for_item(
    frontmatter: &Frontmatter,
    item: &Value,
    fetcher: &mut DataFetcher,
    plugin_registry: Option<&PluginRegistry>,
) -> Result<HashMap<String, Value>> {
    resolve_item_data(
        frontmatter,
        item,
        &frontmatter.item_as,
        fetcher,
        plugin_registry,
    )
    .await
}

/// Interpolate `{{ item_as.field }}` patterns in a DataQuery's filter values.
///
/// Given an item `{"author_id": 42}` and `item_as = "post"`, a filter value
/// of `"{{ post.author_id }}"` becomes `"42"`.
fn interpolate_query(query: &DataQuery, item: &Value, item_as: &str) -> Result<DataQuery> {
    let new_filter = match &query.filter {
        Some(filters) => {
            let mut interpolated = HashMap::new();
            for (key, value_template) in filters {
                let resolved = interpolate_string(value_template, item, item_as)?;
                interpolated.insert(key.clone(), resolved);
            }
            Some(interpolated)
        }
        None => None,
    };

    // Also interpolate `path` in case it contains item references.
    let new_path = match &query.path {
        Some(path_template) => Some(interpolate_string(path_template, item, item_as)?),
        None => query.path.clone(),
    };

    let new_body = match &query.body {
        Some(body) => Some(interpolate_value(body, item, item_as)?),
        None => None,
    };

    Ok(DataQuery {
        file: query.file.clone(),
        source: query.source.clone(),
        path: new_path,
        root: query.root.clone(),
        sort: query.sort.clone(),
        limit: query.limit,
        filter: new_filter,
        method: query.method.clone(),
        body: new_body,
    })
}

/// Replace all `{{ item_as.field.subfield }}` patterns in a string with the
/// corresponding value from the item.
fn interpolate_string(template: &str, item: &Value, item_as: &str) -> Result<String> {
    let mut result = template.to_string();
    for cap in INTERPOLATION_RE.captures_iter(template) {
        let full_match = cap[0].to_string();
        let path = &cap[1];
        let value = resolve_item_path(path, item, item_as)?;
        let replacement = value_to_string(&value);
        result = result.replace(&full_match, &replacement);
    }
    Ok(result)
}

/// Recursively walk a `serde_json::Value` and interpolate `{{ item.field }}`
/// patterns in string nodes. Also replaces `${ENV_VAR}` patterns.
fn interpolate_value(value: &Value, item: &Value, item_as: &str) -> Result<Value> {
    match value {
        Value::String(s) => {
            // First, interpolate {{ item.field }} patterns.
            let resolved = interpolate_string(s, item, item_as)?;
            // Then, interpolate ${ENV_VAR} patterns (fast-path: skip if no $ present).
            let resolved = if resolved.contains("${") {
                interpolate_env_in_string(&resolved)
            } else {
                resolved
            };
            Ok(Value::String(resolved))
        }
        Value::Array(arr) => {
            let items: Result<Vec<Value>> = arr
                .iter()
                .map(|v| interpolate_value(v, item, item_as))
                .collect();
            Ok(Value::Array(items?))
        }
        Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (k, v) in map {
                result.insert(k.clone(), interpolate_value(v, item, item_as)?);
            }
            Ok(Value::Object(result))
        }
        // Numbers, booleans, nulls are passed through unchanged.
        other => Ok(other.clone()),
    }
}

/// Replace `${VAR_NAME}` patterns with environment variable values.
/// Unresolved patterns are left as-is (no error).
/// `$${VAR_NAME}` is an escape sequence that produces a literal `${VAR_NAME}`.
fn interpolate_env_in_string(s: &str) -> String {
    // Phase 1: shelter escaped $${...} patterns behind sentinels.
    static ESCAPED_ENV_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\$\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap());

    let mut sheltered: Vec<String> = Vec::new();
    let working = ESCAPED_ENV_RE.replace_all(s, |caps: &regex::Captures| {
        let var_name = caps[1].to_string();
        sheltered.push(var_name);
        format!("\x00EIGEN_ESC_{}\x00", sheltered.len() - 1)
    }).into_owned();

    // Phase 2: normal env var substitution on the sheltered string.
    let captures: Vec<(String, String)> = ENV_VAR_RE
        .captures_iter(&working)
        .map(|cap| (cap[0].to_string(), cap[1].to_string()))
        .collect();

    let mut result = working;
    for (full_match, var_name) in &captures {
        if let Ok(val) = std::env::var(var_name) {
            result = result.replace(full_match.as_str(), &val);
        }
    }

    // Phase 3: restore sentinels to literal ${VAR_NAME}.
    for (i, var_name) in sheltered.iter().enumerate() {
        result = result.replace(
            &format!("\x00EIGEN_ESC_{}\x00", i),
            &format!("${{{}}}", var_name),
        );
    }

    result
}

/// Resolve a dot-separated path like `"post.author_id"` against the current item.
///
/// The first segment must match `item_as` (e.g., `"post"`). The remaining
/// segments walk into the item's value.
fn resolve_item_path(path: &str, item: &Value, item_as: &str) -> Result<Value> {
    let segments: Vec<&str> = path.split('.').collect();

    if segments.is_empty() {
        bail!("Empty interpolation path");
    }

    if segments[0] != item_as {
        bail!(
            "Interpolation path '{}' does not start with '{}'. \
             In dynamic page data queries, interpolation paths must begin \
             with the item_as name.",
            path,
            item_as,
        );
    }

    let mut current = item;
    for &segment in &segments[1..] {
        match current.get(segment) {
            Some(inner) => current = inner,
            None => bail!(
                "Interpolation path '{}': field '{}' not found in item. \
                 Available fields: {}",
                path,
                segment,
                available_fields(current),
            ),
        }
    }

    Ok(current.clone())
}

/// Convert a JSON value to a string for interpolation.
fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        // For complex types, use JSON serialization.
        other => other.to_string(),
    }
}

/// List available fields in a JSON value (for error messages).
fn available_fields(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            if map.is_empty() {
                "(empty object)".to_string()
            } else {
                map.keys().cloned().collect::<Vec<_>>().join(", ")
            }
        }
        _ => format!("(value is {}, not an object)", type_name(value)),
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// Check if any string node in a JSON value contains `{{ }}` patterns.
fn value_contains_interpolation(value: &Value, re: &Regex) -> Option<String> {
    match value {
        Value::String(s) if re.is_match(s) => Some(s.clone()),
        Value::Array(arr) => {
            for v in arr {
                if let Some(found) = value_contains_interpolation(v, re) {
                    return Some(found);
                }
            }
            None
        }
        Value::Object(map) => {
            for v in map.values() {
                if let Some(found) = value_contains_interpolation(v, re) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

/// Verify that an interpolated DataQuery has no remaining `{{ }}` patterns.
/// This prevents accidental multi-level interpolation.
fn verify_no_remaining_interpolation(query: &DataQuery, query_name: &str) -> Result<()> {
    let re = &*REMAINING_INTERP_RE;

    // Check filter values.
    if let Some(ref filters) = query.filter {
        for (key, value) in filters {
            if re.is_match(value) {
                bail!(
                    "Data query '{}' still contains interpolation pattern in \
                     filter key '{}' after resolution: \"{}\". \
                     Nested interpolation (more than one level) is not supported.",
                    query_name,
                    key,
                    value,
                );
            }
        }
    }

    // Check path.
    if let Some(ref path) = query.path {
        if re.is_match(path) {
            bail!(
                "Data query '{}' still contains interpolation pattern in \
                 path after resolution: \"{}\". \
                 Nested interpolation (more than one level) is not supported.",
                query_name,
                path,
            );
        }
    }

    // Check body.
    if let Some(ref body) = query.body {
        if let Some(found) = value_contains_interpolation(body, &re) {
            bail!(
                "Data query '{}' still contains interpolation pattern in \
                 body after resolution: \"{}\". \
                 Nested interpolation (more than one level) is not supported.",
                query_name,
                found,
            );
        }
    }

    Ok(())
}

// ── Unlocked variants for concurrent page rendering ──────────────────────────
//
// These functions take `Arc<AsyncMutex<DataFetcher>>` instead of
// `&mut DataFetcher`, releasing the mutex before every HTTP `.await` point.

/// Resolve all `data` queries in a static page's frontmatter, releasing the
/// fetcher mutex before each HTTP request.
pub async fn resolve_page_data_unlocked(
    frontmatter: &Frontmatter,
    fetcher: &Arc<AsyncMutex<DataFetcher>>,
    plugin_registry: Option<&PluginRegistry>,
) -> Result<HashMap<String, Value>> {
    let mut result = HashMap::new();
    for (name, query) in &frontmatter.data {
        let value = fetch_unlocked(fetcher, query, plugin_registry)
            .await
            .wrap_err_with(|| format!("Failed to resolve data query '{}'", name))?;
        result.insert(name.clone(), value);
    }
    Ok(result)
}

/// Fetch the collection for a dynamic page, releasing the fetcher mutex
/// before the HTTP request.
pub async fn resolve_dynamic_page_data_unlocked(
    frontmatter: &Frontmatter,
    fetcher: &Arc<AsyncMutex<DataFetcher>>,
    plugin_registry: Option<&PluginRegistry>,
) -> Result<Vec<Value>> {
    let collection_query = frontmatter
        .collection
        .as_ref()
        .ok_or_else(|| eyre::eyre!("Dynamic page has no `collection` in frontmatter"))?;

    let collection = fetch_unlocked(fetcher, collection_query, plugin_registry)
        .await
        .wrap_err("Failed to fetch collection")?;

    match collection {
        Value::Array(items) => Ok(items),
        _ => Ok(Vec::new()),
    }
}

/// Resolve per-item `data` queries for a single dynamic page item, releasing
/// the fetcher mutex before each HTTP request.
pub async fn resolve_dynamic_page_data_for_item_unlocked(
    frontmatter: &Frontmatter,
    item: &Value,
    fetcher: &Arc<AsyncMutex<DataFetcher>>,
    plugin_registry: Option<&PluginRegistry>,
) -> Result<HashMap<String, Value>> {
    let item_as = &frontmatter.item_as;
    let mut result = HashMap::new();

    for (name, query) in &frontmatter.data {
        let interpolated = interpolate_query(query, item, item_as).wrap_err_with(|| {
            format!("Failed to interpolate data query '{}' for current item", name)
        })?;
        verify_no_remaining_interpolation(&interpolated, name)?;

        let value = fetch_unlocked(fetcher, &interpolated, plugin_registry)
            .await
            .wrap_err_with(|| format!("Failed to resolve data query '{}'", name))?;
        result.insert(name.clone(), value);
    }

    Ok(result)
}

/// Fetch a single `DataQuery`, releasing the fetcher mutex before HTTP.
///
/// For file queries (synchronous FS reads), the lock is held for the
/// duration — no `.await` is involved. For source queries, the pattern is:
///
/// 1. Acquire lock → check in-memory cache / build conditional headers → release.
/// 2. HTTP request with no mutex held.
/// 3. Acquire lock → store result → release.
///
/// If a 304 response has a corrupt disk-cached body, retries once (step 2→3).
async fn fetch_unlocked(
    fetcher: &Arc<AsyncMutex<DataFetcher>>,
    query: &DataQuery,
    plugin_registry: Option<&PluginRegistry>,
) -> Result<Value> {
    // Warn about GET + body (mirrors the check in DataFetcher::fetch).
    if matches!(query.method, crate::frontmatter::HttpMethod::Get) && query.body.is_some() {
        tracing::warn!(
            "Data query has 'body' set but method is GET — body will be ignored. \
             Did you mean to set method: post?"
        );
    }

    // File queries: synchronous FS — hold lock for the brief read.
    if let Some(ref file) = query.file {
        let mut f = fetcher.lock().await;
        let raw = f.fetch_file(file)?;
        return apply_post_fetch(raw, query, plugin_registry);
    }

    // Source queries: three-phase lock split.
    let source_name = query.source.as_deref().ok_or_else(|| {
        eyre::eyre!(
            "DataQuery has neither `file` nor `source` set. \
             At least one must be provided."
        )
    })?;
    let path = query.path.as_deref().unwrap_or("");

    // Phase 1: check cache under lock; clone Arc handles for HTTP.
    let (check, client, rate_limiter) = {
        let f = fetcher.lock().await;
        let check =
            f.check_source_cache(source_name, path, &query.method, query.body.as_ref())?;
        let client = f.client.clone();
        let rate_limiter = Arc::clone(&f.rate_limiter);
        (check, client, rate_limiter)
    }; // lock released here

    let (cache_key, full_url, mut headers) = match check {
        SourceCacheCheck::Hit(value) => {
            return apply_post_fetch(value, query, plugin_registry);
        }
        SourceCacheCheck::Miss { cache_key, full_url, headers } => {
            (cache_key, full_url, headers)
        }
    };

    // Phase 2: HTTP with no mutex held.
    let mut response = execute_source_request(
        &client,
        &rate_limiter,
        &query.method,
        &full_url,
        headers,
        query.body.as_ref(),
    )
    .await?;

    // Phase 3: store under lock, retrying if the disk cache is corrupt.
    loop {
        let store_result = {
            let mut f = fetcher.lock().await;
            f.store_source_result(source_name, cache_key.clone(), &full_url, response)
                .await?
        }; // lock released here

        match store_result {
            StoreResult::Value(value) => {
                return apply_post_fetch(value, query, plugin_registry);
            }
            StoreResult::RetryNeeded { fresh_headers } => {
                // Corrupt 304 disk cache — retry HTTP without the lock.
                headers = fresh_headers;
                response = execute_source_request(
                    &client,
                    &rate_limiter,
                    &query.method,
                    &full_url,
                    headers,
                    query.body.as_ref(),
                )
                .await?;
            }
        }
    }
}

/// Apply root extraction, plugin transforms, and filter/sort/limit to a
/// fetched value. Pure CPU — no I/O, no lock needed.
fn apply_post_fetch(
    raw: Value,
    query: &DataQuery,
    plugin_registry: Option<&PluginRegistry>,
) -> Result<Value> {
    let source_name = query.source.as_deref();
    let query_path = query.path.as_deref();

    let extracted = if let Some(ref root) = query.root {
        // Walk the dot-separated root path into the value.
        let mut current = &raw;
        for segment in root.split('.') {
            match current.get(segment) {
                Some(inner) => current = inner,
                None => bail!(
                    "Root path '{}' not found in data. Failed at segment '{}'. \
                     Available keys: {}",
                    root,
                    segment,
                    available_fields(current),
                ),
            }
        }
        current.clone()
    } else {
        raw
    };

    let transformed = if let Some(registry) = plugin_registry {
        registry.transform_data(extracted, source_name, query_path)?
    } else {
        extracted
    };

    Ok(apply_transforms(transformed, &query.filter, &query.sort, &query.limit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

    use crate::build::rate_limit::RateLimiterPool;

    fn no_op_pool() -> Arc<RateLimiterPool> {
        Arc::new(RateLimiterPool::new(None, &HashMap::new()))
    }

    /// Helper to write a file.
    fn write(dir: &std::path::Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    // --- interpolate_string tests ---

    #[test]
    fn test_interpolate_simple() {
        let item = json!({"author_id": 42});
        let result = interpolate_string("{{ post.author_id }}", &item, "post").unwrap();
        assert_eq!(result, "42");
    }

    #[test]
    fn test_interpolate_string_value() {
        let item = json!({"slug": "hello-world"});
        let result = interpolate_string("{{ post.slug }}", &item, "post").unwrap();
        assert_eq!(result, "hello-world");
    }

    #[test]
    fn test_interpolate_in_path() {
        let item = json!({"id": 7});
        let result = interpolate_string("/authors/{{ post.id }}/bio", &item, "post").unwrap();
        assert_eq!(result, "/authors/7/bio");
    }

    #[test]
    fn test_interpolate_multiple() {
        let item = json!({"first": "John", "last": "Doe"});
        let result =
            interpolate_string("{{ person.first }}-{{ person.last }}", &item, "person").unwrap();
        assert_eq!(result, "John-Doe");
    }

    #[test]
    fn test_interpolate_nested_field() {
        let item = json!({"meta": {"author_id": 99}});
        let result = interpolate_string("{{ post.meta.author_id }}", &item, "post").unwrap();
        assert_eq!(result, "99");
    }

    #[test]
    fn test_interpolate_wrong_prefix() {
        let item = json!({"id": 1});
        let result = interpolate_string("{{ wrong.id }}", &item, "post");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("wrong"));
        assert!(err.contains("post"));
    }

    #[test]
    fn test_interpolate_missing_field() {
        let item = json!({"id": 1});
        let result = interpolate_string("{{ post.nonexistent }}", &item, "post");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("nonexistent"));
    }

    #[test]
    fn test_interpolate_no_patterns() {
        let item = json!({"id": 1});
        let result = interpolate_string("plain string", &item, "post").unwrap();
        assert_eq!(result, "plain string");
    }

    // --- interpolate_query tests ---

    #[test]
    fn test_interpolate_query_filter() {
        let item = json!({"author_id": 42});
        let query = DataQuery {
            source: Some("api".into()),
            path: Some("/authors".into()),
            filter: Some({
                let mut m = HashMap::new();
                m.insert("id".into(), "{{ post.author_id }}".into());
                m
            }),
            ..Default::default()
        };

        let result = interpolate_query(&query, &item, "post").unwrap();
        let filter = result.filter.unwrap();
        assert_eq!(filter["id"], "42");
    }

    #[test]
    fn test_interpolate_query_path() {
        let item = json!({"id": 7});
        let query = DataQuery {
            source: Some("api".into()),
            path: Some("/posts/{{ post.id }}/comments".into()),
            ..Default::default()
        };

        let result = interpolate_query(&query, &item, "post").unwrap();
        assert_eq!(result.path.unwrap(), "/posts/7/comments");
    }

    // --- verify_no_remaining_interpolation tests ---

    #[test]
    fn test_verify_clean_query() {
        let query = DataQuery {
            filter: Some({
                let mut m = HashMap::new();
                m.insert("id".into(), "42".into());
                m
            }),
            path: Some("/posts".into()),
            ..Default::default()
        };
        assert!(verify_no_remaining_interpolation(&query, "test").is_ok());
    }

    #[test]
    fn test_verify_remaining_in_filter() {
        let query = DataQuery {
            filter: Some({
                let mut m = HashMap::new();
                m.insert("id".into(), "{{ nested.ref }}".into());
                m
            }),
            ..Default::default()
        };
        let result = verify_no_remaining_interpolation(&query, "test");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.to_lowercase().contains("nested interpolation"));
    }

    #[test]
    fn test_verify_remaining_in_path() {
        let query = DataQuery {
            path: Some("/posts/{{ nested.id }}".into()),
            ..Default::default()
        };
        let result = verify_no_remaining_interpolation(&query, "test");
        assert!(result.is_err());
    }

    // --- resolve_page_data tests ---

    #[tokio::test]
    async fn test_resolve_page_data_file() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "_data/nav.yaml", "- label: Home\n  url: /\n");

        let pool = no_op_pool();
        let mut fetcher = DataFetcher::new(&HashMap::new(), root, None, pool);
        let fm = Frontmatter {
            data: {
                let mut m = HashMap::new();
                m.insert(
                    "nav".into(),
                    DataQuery {
                        file: Some("nav.yaml".into()),
                        ..Default::default()
                    },
                );
                m
            },
            ..Default::default()
        };

        let result = resolve_page_data(&fm, &mut fetcher, None).await.unwrap();
        assert_eq!(result.len(), 1);
        assert!(result["nav"].is_array());
    }

    #[tokio::test]
    async fn test_resolve_page_data_multiple() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "_data/nav.yaml", "- label: Home\n  url: /\n");
        write(root, "_data/config.json", r#"{"debug": false}"#);

        let pool = no_op_pool();
        let mut fetcher = DataFetcher::new(&HashMap::new(), root, None, pool);
        let fm = Frontmatter {
            data: {
                let mut m = HashMap::new();
                m.insert(
                    "nav".into(),
                    DataQuery {
                        file: Some("nav.yaml".into()),
                        ..Default::default()
                    },
                );
                m.insert(
                    "config".into(),
                    DataQuery {
                        file: Some("config.json".into()),
                        ..Default::default()
                    },
                );
                m
            },
            ..Default::default()
        };

        let result = resolve_page_data(&fm, &mut fetcher, None).await.unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains_key("nav"));
        assert!(result.contains_key("config"));
    }

    #[tokio::test]
    async fn test_resolve_page_data_empty() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let pool = no_op_pool();
        let mut fetcher = DataFetcher::new(&HashMap::new(), root, None, pool);
        let fm = Frontmatter::default();

        let result = resolve_page_data(&fm, &mut fetcher, None).await.unwrap();
        assert!(result.is_empty());
    }

    // --- resolve_dynamic_page_data tests ---

    #[tokio::test]
    async fn test_resolve_dynamic_collection_from_file() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(
            root,
            "_data/posts.json",
            r#"[{"id": 1, "title": "First"}, {"id": 2, "title": "Second"}]"#,
        );

        let pool = no_op_pool();
        let mut fetcher = DataFetcher::new(&HashMap::new(), root, None, pool);
        let fm = Frontmatter {
            collection: Some(DataQuery {
                file: Some("posts.json".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let items = resolve_dynamic_page_data(&fm, &mut fetcher, None)
            .await
            .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["title"], "First");
    }

    #[tokio::test]
    async fn test_resolve_dynamic_no_collection() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let pool = no_op_pool();
        let mut fetcher = DataFetcher::new(&HashMap::new(), root, None, pool);
        let fm = Frontmatter::default();

        let result = resolve_dynamic_page_data(&fm, &mut fetcher, None).await;
        assert!(result.is_err());
    }

    // --- resolve_item_data tests ---

    #[tokio::test]
    async fn test_resolve_item_data_with_interpolation() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(
            root,
            "_data/authors.json",
            r#"[{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}]"#,
        );

        let pool = no_op_pool();
        let mut fetcher = DataFetcher::new(&HashMap::new(), root, None, pool);

        let fm = Frontmatter {
            item_as: "post".into(),
            data: {
                let mut m = HashMap::new();
                m.insert(
                    "author".into(),
                    DataQuery {
                        file: Some("authors.json".into()),
                        filter: Some({
                            let mut f = HashMap::new();
                            f.insert("id".into(), "{{ post.author_id }}".into());
                            f
                        }),
                        ..Default::default()
                    },
                );
                m
            },
            ..Default::default()
        };

        let item = json!({"author_id": 2, "title": "My Post"});
        let result = resolve_item_data(&fm, &item, "post", &mut fetcher, None)
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        let authors = result["author"].as_array().unwrap();
        assert_eq!(authors.len(), 1);
        assert_eq!(authors[0]["name"], "Bob");
    }

    #[tokio::test]
    async fn test_resolve_item_data_no_interpolation() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "_data/sidebar.yaml", "- widget: recent\n");

        let pool = no_op_pool();
        let mut fetcher = DataFetcher::new(&HashMap::new(), root, None, pool);

        let fm = Frontmatter {
            item_as: "post".into(),
            data: {
                let mut m = HashMap::new();
                m.insert(
                    "sidebar".into(),
                    DataQuery {
                        file: Some("sidebar.yaml".into()),
                        ..Default::default()
                    },
                );
                m
            },
            ..Default::default()
        };

        let item = json!({"id": 1});
        let result = resolve_item_data(&fm, &item, "post", &mut fetcher, None)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert!(result["sidebar"].is_array());
    }

    // --- Plugin registry integration tests ---

    #[tokio::test]
    async fn test_resolve_page_data_with_plugin_registry() {
        use crate::plugins::registry::PluginRegistry;
        use crate::plugins::Plugin;

        #[derive(Debug)]
        struct TagPlugin;

        impl Plugin for TagPlugin {
            fn name(&self) -> &str {
                "tag"
            }

            fn transform_data(
                &self,
                mut value: serde_json::Value,
                _source: Option<&str>,
                _path: Option<&str>,
            ) -> eyre::Result<serde_json::Value> {
                if let serde_json::Value::Array(ref mut arr) = value {
                    for item in arr.iter_mut() {
                        if let Some(obj) = item.as_object_mut() {
                            obj.insert("tagged".into(), json!(true));
                        }
                    }
                }
                Ok(value)
            }
        }

        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "_data/items.json", r#"[{"id": 1}, {"id": 2}]"#);

        let pool = no_op_pool();
        let mut fetcher = DataFetcher::new(&HashMap::new(), root, None, pool);
        let mut registry = PluginRegistry::new();
        registry.register(Box::new(TagPlugin));

        let fm = Frontmatter {
            data: {
                let mut m = HashMap::new();
                m.insert(
                    "items".into(),
                    DataQuery {
                        file: Some("items.json".into()),
                        ..Default::default()
                    },
                );
                m
            },
            ..Default::default()
        };

        let result = resolve_page_data(&fm, &mut fetcher, Some(&registry))
            .await
            .unwrap();
        let items = result["items"].as_array().unwrap();
        assert!(items[0]["tagged"].as_bool().unwrap());
        assert!(items[1]["tagged"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_resolve_dynamic_page_data_with_plugin_registry() {
        use crate::plugins::registry::PluginRegistry;
        use crate::plugins::Plugin;

        #[derive(Debug)]
        struct EnrichPlugin;

        impl Plugin for EnrichPlugin {
            fn name(&self) -> &str {
                "enrich"
            }

            fn transform_data(
                &self,
                mut value: serde_json::Value,
                _source: Option<&str>,
                _path: Option<&str>,
            ) -> eyre::Result<serde_json::Value> {
                if let serde_json::Value::Array(ref mut arr) = value {
                    for item in arr.iter_mut() {
                        if let Some(obj) = item.as_object_mut() {
                            obj.insert("enriched".into(), json!(true));
                        }
                    }
                }
                Ok(value)
            }
        }

        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(
            root,
            "_data/posts.json",
            r#"[{"slug": "a", "title": "A"}, {"slug": "b", "title": "B"}]"#,
        );

        let pool = no_op_pool();
        let mut fetcher = DataFetcher::new(&HashMap::new(), root, None, pool);
        let mut registry = PluginRegistry::new();
        registry.register(Box::new(EnrichPlugin));

        let fm = Frontmatter {
            collection: Some(DataQuery {
                file: Some("posts.json".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let items = resolve_dynamic_page_data(&fm, &mut fetcher, Some(&registry))
            .await
            .unwrap();
        assert_eq!(items.len(), 2);
        assert!(items[0]["enriched"].as_bool().unwrap());
        assert!(items[1]["enriched"].as_bool().unwrap());
    }

    // --- interpolate_query body tests ---

    #[test]
    fn test_interpolate_query_body_simple() {
        let item = json!({"db_id": "abc123"});
        let query = DataQuery {
            source: Some("notion".into()),
            path: Some("/v1/databases/abc123/query".into()),
            method: crate::frontmatter::HttpMethod::Post,
            body: Some(json!({
                "filter": {
                    "property": "Author",
                    "rich_text": {
                        "equals": "{{ post.db_id }}"
                    }
                }
            })),
            ..Default::default()
        };

        let result = interpolate_query(&query, &item, "post").unwrap();
        let body = result.body.unwrap();
        assert_eq!(body["filter"]["rich_text"]["equals"], "abc123");
    }

    #[test]
    fn test_interpolate_query_body_no_patterns() {
        let item = json!({"id": 1});
        let query = DataQuery {
            source: Some("api".into()),
            method: crate::frontmatter::HttpMethod::Post,
            body: Some(json!({"page_size": 100})),
            ..Default::default()
        };

        let result = interpolate_query(&query, &item, "post").unwrap();
        let body = result.body.unwrap();
        assert_eq!(body["page_size"], 100);
    }

    #[test]
    fn test_interpolate_query_body_none() {
        let item = json!({"id": 1});
        let query = DataQuery {
            source: Some("api".into()),
            method: crate::frontmatter::HttpMethod::Post,
            body: None,
            ..Default::default()
        };

        let result = interpolate_query(&query, &item, "post").unwrap();
        assert!(result.body.is_none());
    }

    #[test]
    fn test_interpolate_query_preserves_method() {
        let item = json!({"id": 1});
        let query = DataQuery {
            source: Some("api".into()),
            method: crate::frontmatter::HttpMethod::Post,
            ..Default::default()
        };

        let result = interpolate_query(&query, &item, "post").unwrap();
        assert_eq!(result.method, crate::frontmatter::HttpMethod::Post);
    }

    #[test]
    fn test_interpolate_query_body_env_var() {
        // SAFETY: This test runs single-threaded; no other thread reads this var.
        unsafe { std::env::set_var("TEST_EIGEN_DB_ID", "db_abc123") };
        let item = json!({});
        let query = DataQuery {
            source: Some("notion".into()),
            method: crate::frontmatter::HttpMethod::Post,
            body: Some(json!({"database_id": "${TEST_EIGEN_DB_ID}"})),
            ..Default::default()
        };
        let result = interpolate_query(&query, &item, "item").unwrap();
        assert_eq!(result.body.unwrap()["database_id"], "db_abc123");
        // SAFETY: Cleanup; same reasoning as above.
        unsafe { std::env::remove_var("TEST_EIGEN_DB_ID") };
    }

    #[test]
    fn test_interpolate_query_body_unresolved_env_var_left_as_is() {
        let item = json!({});
        let query = DataQuery {
            source: Some("api".into()),
            method: crate::frontmatter::HttpMethod::Post,
            body: Some(json!({"key": "${DEFINITELY_NOT_SET_EIGEN_TEST}"})),
            ..Default::default()
        };
        let result = interpolate_query(&query, &item, "item").unwrap();
        assert_eq!(
            result.body.unwrap()["key"],
            "${DEFINITELY_NOT_SET_EIGEN_TEST}"
        );
    }

    #[test]
    fn test_interpolate_env_escape_in_string() {
        let input = "Use $${API_KEY} here";
        let result = interpolate_env_in_string(input);
        assert_eq!(result, "Use ${API_KEY} here");
    }

    // --- verify_no_remaining_interpolation body tests ---

    #[test]
    fn test_verify_remaining_in_body() {
        let query = DataQuery {
            method: crate::frontmatter::HttpMethod::Post,
            body: Some(json!({
                "filter": {
                    "equals": "{{ nested.ref }}"
                }
            })),
            ..Default::default()
        };
        let result = verify_no_remaining_interpolation(&query, "test");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.to_lowercase().contains("nested interpolation"));
    }

    #[test]
    fn test_verify_clean_body() {
        let query = DataQuery {
            method: crate::frontmatter::HttpMethod::Post,
            body: Some(json!({
                "page_size": 100,
                "filter": {"property": "Status"}
            })),
            ..Default::default()
        };
        assert!(verify_no_remaining_interpolation(&query, "test").is_ok());
    }

    #[tokio::test]
    async fn test_resolve_dynamic_page_data_for_item_with_plugin() {
        use crate::plugins::registry::PluginRegistry;
        use crate::plugins::Plugin;

        #[derive(Debug)]
        struct PassthroughPlugin;

        impl Plugin for PassthroughPlugin {
            fn name(&self) -> &str {
                "passthrough"
            }
        }

        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "_data/sidebar.yaml", "- widget: recent\n");

        let pool = no_op_pool();
        let mut fetcher = DataFetcher::new(&HashMap::new(), root, None, pool);
        let mut registry = PluginRegistry::new();
        registry.register(Box::new(PassthroughPlugin));

        let fm = Frontmatter {
            item_as: "post".into(),
            data: {
                let mut m = HashMap::new();
                m.insert(
                    "sidebar".into(),
                    DataQuery {
                        file: Some("sidebar.yaml".into()),
                        ..Default::default()
                    },
                );
                m
            },
            ..Default::default()
        };

        let item = json!({"id": 1, "title": "Test"});
        let result =
            resolve_dynamic_page_data_for_item(&fm, &item, &mut fetcher, Some(&registry))
                .await
                .unwrap();
        assert_eq!(result.len(), 1);
        assert!(result["sidebar"].is_array());
    }

    // --- Hegel property-based tests ---

    use hegel::generators;

    #[hegel::test]
    fn prop_interpolate_string_no_pattern_passthrough(tc: hegel::TestCase) {
        let s: String = tc.draw(generators::text());
        tc.assume(!s.contains("{{"));
        let item = json!({"id": 1});
        let result = interpolate_string(&s, &item, "item").unwrap();
        assert_eq!(result, s);
    }

    #[hegel::test]
    fn prop_interpolate_string_robustness(tc: hegel::TestCase) {
        let s: String = tc.draw(generators::text());
        let item = json!({"id": 1});
        let _ = interpolate_string(&s, &item, "item");
    }

    #[hegel::test]
    fn prop_value_to_string_roundtrip_integers(tc: hegel::TestCase) {
        let n: i64 = tc.draw(generators::integers());
        let s = value_to_string(&Value::Number(serde_json::Number::from(n)));
        let parsed: i64 = s.parse().unwrap();
        assert_eq!(parsed, n);
    }

    // --- resolve_page_data_unlocked tests ---

    #[tokio::test]
    async fn resolve_page_data_unlocked_file_query() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "_data/nav.yaml", "- label: Home\n  url: /\n");

        let pool = no_op_pool();
        let fetcher = Arc::new(AsyncMutex::new(
            DataFetcher::new(&HashMap::new(), root, None, pool),
        ));

        let fm = Frontmatter {
            data: {
                let mut m = HashMap::new();
                m.insert(
                    "nav".into(),
                    DataQuery {
                        file: Some("nav.yaml".into()),
                        ..Default::default()
                    },
                );
                m
            },
            ..Default::default()
        };

        let result = resolve_page_data_unlocked(&fm, &fetcher, None)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert!(result["nav"].is_array());
    }

    #[tokio::test]
    async fn resolve_dynamic_page_data_unlocked_file_query() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "_data/posts.json", r#"[{"id": 1}, {"id": 2}]"#);

        let pool = no_op_pool();
        let fetcher = Arc::new(AsyncMutex::new(
            DataFetcher::new(&HashMap::new(), root, None, pool),
        ));

        let fm = Frontmatter {
            collection: Some(DataQuery {
                file: Some("posts.json".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let items = resolve_dynamic_page_data_unlocked(&fm, &fetcher, None)
            .await
            .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["id"], 1);
    }

    #[tokio::test]
    async fn resolve_dynamic_page_data_for_item_unlocked_file_query() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "_data/meta.json", r#"{"title": "static"}"#);

        let pool = no_op_pool();
        let fetcher = Arc::new(AsyncMutex::new(
            DataFetcher::new(&HashMap::new(), root, None, pool),
        ));

        let fm = Frontmatter {
            item_as: "post".into(),
            data: {
                let mut m = HashMap::new();
                m.insert(
                    "meta".into(),
                    DataQuery {
                        file: Some("meta.json".into()),
                        ..Default::default()
                    },
                );
                m
            },
            ..Default::default()
        };

        let item = json!({"id": 1});
        let result =
            resolve_dynamic_page_data_for_item_unlocked(&fm, &item, &fetcher, None)
                .await
                .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result["meta"]["title"], "static");
    }

    /// Two concurrent `fetch_unlocked` callers racing on the same URL both succeed.
    ///
    /// This exercises the lock-split path: both tasks reach Phase 2 (HTTP) with
    /// no mutex held, then each re-acquires the lock to store the result.
    #[tokio::test]
    async fn fetch_unlocked_concurrent_access_both_succeed() {
        use crate::config::SourceConfig;
        use crate::data::fetcher::DataFetcher;
        use std::thread;

        // Tiny HTTP server that handles two requests and returns JSON.
        let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
        let addr = server.server_addr().to_ip().expect("addr");
        let url = format!("http://{}/items", addr);

        thread::spawn(move || {
            let body: &[u8] = b"[{\"id\":1}]";
            let ct = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                .expect("header");
            for _ in 0..2 {
                if let Ok(Some(req)) = server.recv_timeout(std::time::Duration::from_secs(5)) {
                    let _ = req.respond(tiny_http::Response::new(
                        tiny_http::StatusCode(200),
                        vec![ct.clone()],
                        std::io::Cursor::new(body),
                        Some(body.len()),
                        None,
                    ));
                }
            }
        });

        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let mut sources = HashMap::new();
        sources.insert(
            "api".to_string(),
            SourceConfig {
                url: format!("http://{}", addr),
                headers: HashMap::new(),
                rate_limit: None,
            },
        );
        let pool = no_op_pool();
        let fetcher = Arc::new(AsyncMutex::new(DataFetcher::new(&sources, root, None, pool)));

        let fm = Frontmatter {
            data: {
                let mut m = HashMap::new();
                m.insert(
                    "items".into(),
                    DataQuery {
                        source: Some("api".into()),
                        path: Some("/items".into()),
                        ..Default::default()
                    },
                );
                m
            },
            ..Default::default()
        };

        let fetcher_a = Arc::clone(&fetcher);
        let fetcher_b = Arc::clone(&fetcher);
        let fm_a = fm.clone();
        let fm_b = fm.clone();

        let (r1, r2) = tokio::join!(
            tokio::spawn(async move {
                resolve_page_data_unlocked(&fm_a, &fetcher_a, None).await
            }),
            tokio::spawn(async move {
                resolve_page_data_unlocked(&fm_b, &fetcher_b, None).await
            }),
        );

        let data1 = r1.expect("task 1 panicked").expect("task 1 error");
        let data2 = r2.expect("task 2 panicked").expect("task 2 error");

        assert_eq!(data1["items"], json!([{"id": 1}]));
        assert_eq!(data2["items"], json!([{"id": 1}]));
    }
}
