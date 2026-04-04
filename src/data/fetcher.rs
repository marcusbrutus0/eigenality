//! Step 3.2: DataFetcher — fetch data from local files or remote sources with
//! caching, root extraction, and transforms.

use eyre::{bail, Result, WrapErr};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::SourceConfig;
use crate::frontmatter::{DataQuery, HttpMethod};
use crate::plugins::registry::PluginRegistry;

use super::transforms::apply_transforms;

/// Fetches and caches data from local files and remote HTTP sources.
pub struct DataFetcher {
    /// Source definitions from `site.toml`.
    sources: HashMap<String, SourceConfig>,
    /// Cache for HTTP responses keyed by full URL.
    url_cache: HashMap<String, Value>,
    /// Cache for local file data keyed by file path (relative to `_data/`).
    file_cache: HashMap<String, Value>,
    /// Path to the project's `_data/` directory.
    data_dir: PathBuf,
    /// HTTP client (reused across requests).
    client: reqwest::blocking::Client,
    /// Optional disk cache for conditional HTTP requests.
    data_cache: Option<super::cache::DataCache>,
}

impl DataFetcher {
    /// Create a new fetcher with the given sources and project root.
    pub fn new(
        sources: &HashMap<String, SourceConfig>,
        project_root: &Path,
        data_cache: Option<super::cache::DataCache>,
    ) -> Self {
        Self {
            sources: sources.clone(),
            url_cache: HashMap::new(),
            file_cache: HashMap::new(),
            data_dir: project_root.join("_data"),
            client: reqwest::blocking::Client::new(),
            data_cache,
        }
    }

    /// Fetch data for a single `DataQuery`.
    ///
    /// The query may reference a local file (`file` field) or a remote source
    /// (`source` + `path` fields). After fetching the raw data, `root`
    /// extraction, plugin transforms, and transforms (filter, sort, limit)
    /// are applied.
    pub fn fetch(
        &mut self,
        query: &DataQuery,
        plugin_registry: Option<&PluginRegistry>,
    ) -> Result<Value> {
        let source_name = query.source.as_deref();
        let query_path = query.path.as_deref();

        // Warn if body is set on a GET request — it will be ignored.
        if matches!(query.method, HttpMethod::Get) && query.body.is_some() {
            tracing::warn!(
                "Data query has 'body' set but method is GET — body will be ignored. \
                 Did you mean to set method: post?"
            );
        }

        let raw = if let Some(ref file) = query.file {
            self.fetch_file(file)?
        } else if let Some(ref source_name) = query.source {
            self.fetch_source(
                source_name,
                query.path.as_deref().unwrap_or(""),
                &query.method,
                query.body.as_ref(),
            )?
        } else {
            bail!(
                "DataQuery has neither `file` nor `source` set. \
                 At least one must be provided."
            );
        };

        // Apply root extraction.
        let extracted = if let Some(ref root) = query.root {
            extract_root(&raw, root)?
        } else {
            raw
        };

        // Apply plugin data transforms (e.g., Strapi flattening).
        let transformed = if let Some(registry) = plugin_registry {
            registry.transform_data(extracted, source_name, query_path)?
        } else {
            extracted
        };

        // Apply transforms: filter → sort → limit.
        let result = apply_transforms(transformed, &query.filter, &query.sort, &query.limit);

        Ok(result)
    }

    /// Load a local file from `_data/`.
    fn fetch_file(&mut self, file_path: &str) -> Result<Value> {
        if let Some(cached) = self.file_cache.get(file_path) {
            return Ok(cached.clone());
        }

        let full_path = self.data_dir.join(file_path);
        let content = std::fs::read_to_string(&full_path)
            .wrap_err_with(|| format!("Failed to read data file: {}", full_path.display()))?;

        let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let value: Value = match ext {
            "yaml" | "yml" => serde_yaml::from_str(&content)
                .wrap_err_with(|| format!("Failed to parse YAML: {}", full_path.display()))?,
            "json" => serde_json::from_str(&content)
                .wrap_err_with(|| format!("Failed to parse JSON: {}", full_path.display()))?,
            _ => bail!(
                "Unsupported data file extension '.{}' for {}. Use .yaml, .yml, or .json.",
                ext,
                full_path.display()
            ),
        };

        self.file_cache.insert(file_path.to_string(), value.clone());
        Ok(value)
    }

    /// Build a `HeaderMap` from a source's configured headers.
    fn build_source_headers(source: &SourceConfig) -> Result<reqwest::header::HeaderMap> {
        let mut headers = reqwest::header::HeaderMap::new();
        for (key, val) in &source.headers {
            let name = reqwest::header::HeaderName::from_bytes(key.as_bytes())
                .wrap_err_with(|| format!("Invalid header name: {}", key))?;
            let value = reqwest::header::HeaderValue::from_str(val)
                .wrap_err_with(|| format!("Invalid header value for {}", key))?;
            headers.insert(name, value);
        }
        Ok(headers)
    }

    /// Send an HTTP request with the given method, headers, and optional body.
    fn send_request(
        &self,
        method: &HttpMethod,
        url: &str,
        headers: reqwest::header::HeaderMap,
        body: Option<&serde_json::Value>,
    ) -> Result<reqwest::blocking::Response> {
        let response = match method {
            HttpMethod::Get => self.client.get(url).headers(headers).send(),
            HttpMethod::Post => {
                let mut req = self.client.post(url).headers(headers);
                if let Some(b) = body {
                    req = req.json(b);
                }
                req.send()
            }
        }
        .wrap_err_with(|| format!("HTTP request failed for {}", url))?;
        Ok(response)
    }

    /// Fetch data from a remote source defined in `site.toml`.
    fn fetch_source(
        &mut self,
        source_name: &str,
        path: &str,
        method: &HttpMethod,
        body: Option<&serde_json::Value>,
    ) -> Result<Value> {
        let source = self
            .sources
            .get(source_name)
            .ok_or_else(|| {
                eyre::eyre!(
                    "Source '{}' not found in site.toml. Available: {}",
                    source_name,
                    self.sources.keys().cloned().collect::<Vec<_>>().join(", ")
                )
            })?;

        // Build full URL: base URL + path.
        let full_url = format!(
            "{}{}",
            source.url.trim_end_matches('/'),
            if path.starts_with('/') {
                path.to_string()
            } else {
                format!("/{}", path)
            }
        );

        // Build cache key: include method (and body hash for POST).
        let cache_key = match method {
            HttpMethod::Get => format!("GET:{}", full_url),
            HttpMethod::Post => {
                let body_hash = match body {
                    Some(b) => {
                        let body_str = serde_json::to_string(b).unwrap_or_default();
                        crate::data::cache::cache_key_hash(&body_str)
                    }
                    None => String::new(),
                };
                format!("POST:{}:{}", full_url, body_hash)
            }
        };

        if let Some(cached) = self.url_cache.get(&cache_key) {
            return Ok(cached.clone());
        }

        let mut headers = Self::build_source_headers(source)?;

        // Add conditional headers if disk cache has metadata.
        if let Some(meta) = self.data_cache.as_ref().and_then(|dc| dc.get(&cache_key)) {
            if let Some(ref etag) = meta.etag {
                if let Ok(hv) = reqwest::header::HeaderValue::from_str(etag) {
                    headers.insert(reqwest::header::IF_NONE_MATCH, hv);
                }
            }
            if let Some(ref lm) = meta.last_modified {
                if let Ok(hv) = reqwest::header::HeaderValue::from_str(lm) {
                    headers.insert(reqwest::header::IF_MODIFIED_SINCE, hv);
                }
            }
            tracing::debug!("Conditional request for {} (etag: {:?})", full_url, meta.etag);
        }

        let response = self.send_request(method, &full_url, headers, body)?;
        let status = response.status();

        // Handle 304 Not Modified: read body from disk cache.
        if status == reqwest::StatusCode::NOT_MODIFIED {
            if let Some(body_bytes) = self.data_cache.as_ref().and_then(|dc| dc.read(&cache_key)) {
                match serde_json::from_slice::<Value>(&body_bytes) {
                    Ok(value) => {
                        tracing::debug!("Data cache hit (304): {}", cache_key);
                        self.url_cache.insert(cache_key, value.clone());
                        return Ok(value);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "304 but cached body failed to parse for {}: {}. Re-fetching.",
                            full_url, e,
                        );
                        let retry_headers = Self::build_source_headers(source)?;
                        let retry_response = self.send_request(method, &full_url, retry_headers, body)?;
                        let retry_status = retry_response.status();
                        if !retry_status.is_success() {
                            bail!("HTTP {} from {} (retry after bad 304 cache)", retry_status, full_url);
                        }
                        return self.handle_success_response(retry_response, cache_key, &full_url);
                    }
                }
            }
            bail!("HTTP 304 from {} but no cached body available", full_url);
        }

        if !status.is_success() {
            bail!("HTTP {} from {}", status, full_url);
        }

        self.handle_success_response(response, cache_key, &full_url)
    }

    /// Process a successful HTTP response: extract cache headers, store in disk
    /// cache, parse JSON, and populate the in-memory cache.
    fn handle_success_response(
        &mut self,
        response: reqwest::blocking::Response,
        cache_key: String,
        full_url: &str,
    ) -> Result<Value> {
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let last_modified = response
            .headers()
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        let raw_bytes = response
            .bytes()
            .wrap_err_with(|| format!("Failed to read response bytes from {}", full_url))?;

        if let Some(dc) = self.data_cache.as_mut() {
            if etag.is_some() || last_modified.is_some() {
                if let Err(e) = dc.store(
                    &cache_key,
                    &raw_bytes,
                    etag.as_deref(),
                    last_modified.as_deref(),
                ) {
                    tracing::warn!("Failed to store data cache for {}: {}", full_url, e);
                }
            }
        }

        let value: Value = serde_json::from_slice(&raw_bytes)
            .wrap_err_with(|| format!("Failed to parse JSON response from {}", full_url))?;

        self.url_cache.insert(cache_key, value.clone());
        Ok(value)
    }

    /// Clear the file cache (used when `_data/` files change during dev).
    pub fn clear_file_cache(&mut self) {
        self.file_cache.clear();
    }

    /// Clear the URL cache (used when frontmatter queries change during dev).
    #[allow(unused)]
    pub fn clear_url_cache(&mut self) {
        self.url_cache.clear();
    }
}

/// Walk into a JSON value using a dot-separated path.
///
/// For example, `extract_root(value, "data.posts")` returns `value["data"]["posts"]`.
fn extract_root(value: &Value, root: &str) -> Result<Value> {
    let mut current = value;
    for segment in root.split('.') {
        match current.get(segment) {
            Some(inner) => current = inner,
            None => bail!(
                "Root path '{}' not found in data. Failed at segment '{}'. \
                 Available keys: {}",
                root,
                segment,
                available_keys(current),
            ),
        }
    }
    Ok(current.clone())
}

/// List the keys of a JSON object for error messages, or describe its type.
fn available_keys(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            if map.is_empty() {
                "(empty object)".to_string()
            } else {
                map.keys().cloned().collect::<Vec<_>>().join(", ")
            }
        }
        Value::Array(_) => "(value is an array, not an object)".to_string(),
        _ => format!("(value is {}, not an object)", value_type_name(value)),
    }
}

fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    /// Create a fetcher with no remote sources, pointed at a temp dir.
    fn test_fetcher(root: &Path) -> DataFetcher {
        DataFetcher::new(&HashMap::new(), root, None)
    }

    /// Helper to write a file.
    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    // --- extract_root tests ---

    #[test]
    fn test_extract_root_single_level() {
        let value = json!({"data": [1, 2, 3]});
        let result = extract_root(&value, "data").unwrap();
        assert_eq!(result, json!([1, 2, 3]));
    }

    #[test]
    fn test_extract_root_nested() {
        let value = json!({"data": {"posts": [{"id": 1}]}});
        let result = extract_root(&value, "data.posts").unwrap();
        assert_eq!(result, json!([{"id": 1}]));
    }

    #[test]
    fn test_extract_root_missing_key() {
        let value = json!({"data": {"users": []}});
        let result = extract_root(&value, "data.posts");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("posts"));
        assert!(err.contains("users")); // should show available keys
    }

    #[test]
    fn test_extract_root_from_non_object() {
        let value = json!([1, 2, 3]);
        let result = extract_root(&value, "items");
        assert!(result.is_err());
    }

    // --- fetch_file tests ---

    #[test]
    fn test_fetch_file_yaml() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "_data/nav.yaml", "- label: Home\n  url: /\n");

        let mut fetcher = test_fetcher(root);
        let query = DataQuery {
            file: Some("nav.yaml".into()),
            ..Default::default()
        };

        let result = fetcher.fetch(&query, None).unwrap();
        assert!(result.is_array());
        assert_eq!(result.as_array().unwrap().len(), 1);
        assert_eq!(result[0]["label"], "Home");
    }

    #[test]
    fn test_fetch_file_json() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "_data/config.json", r#"{"debug": true}"#);

        let mut fetcher = test_fetcher(root);
        let query = DataQuery {
            file: Some("config.json".into()),
            ..Default::default()
        };

        let result = fetcher.fetch(&query, None).unwrap();
        assert_eq!(result["debug"], true);
    }

    #[test]
    fn test_fetch_file_with_root() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(
            root,
            "_data/response.json",
            r#"{"data": {"items": [1, 2, 3]}}"#,
        );

        let mut fetcher = test_fetcher(root);
        let query = DataQuery {
            file: Some("response.json".into()),
            root: Some("data.items".into()),
            ..Default::default()
        };

        let result = fetcher.fetch(&query, None).unwrap();
        assert_eq!(result, json!([1, 2, 3]));
    }

    #[test]
    fn test_fetch_file_with_transforms() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(
            root,
            "_data/posts.json",
            r#"[
                {"id": 3, "status": "draft"},
                {"id": 1, "status": "published"},
                {"id": 5, "status": "published"},
                {"id": 2, "status": "published"},
                {"id": 4, "status": "draft"}
            ]"#,
        );

        let mut filter = HashMap::new();
        filter.insert("status".into(), "published".into());

        let mut fetcher = test_fetcher(root);
        let query = DataQuery {
            file: Some("posts.json".into()),
            filter: Some(filter),
            sort: Some("id".into()),
            limit: Some(2),
            ..Default::default()
        };

        let result = fetcher.fetch(&query, None).unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], 1);
        assert_eq!(arr[1]["id"], 2);
    }

    #[test]
    fn test_fetch_file_caching() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "_data/nav.yaml", "- label: Home\n");

        let mut fetcher = test_fetcher(root);
        let query = DataQuery {
            file: Some("nav.yaml".into()),
            ..Default::default()
        };

        // First fetch reads file.
        let r1 = fetcher.fetch(&query, None).unwrap();

        // Delete the file — cached result should still work.
        fs::remove_file(root.join("_data/nav.yaml")).unwrap();
        let r2 = fetcher.fetch(&query, None).unwrap();

        assert_eq!(r1, r2);
    }

    #[test]
    fn test_fetch_file_missing() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("_data")).unwrap();

        let mut fetcher = test_fetcher(root);
        let query = DataQuery {
            file: Some("nonexistent.yaml".into()),
            ..Default::default()
        };

        let result = fetcher.fetch(&query, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_fetch_no_file_or_source_errors() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let mut fetcher = test_fetcher(root);
        let query = DataQuery::default();

        let result = fetcher.fetch(&query, None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("neither"));
    }

    #[test]
    fn test_fetch_unknown_source_errors() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let mut fetcher = test_fetcher(root);
        let query = DataQuery {
            source: Some("nonexistent".into()),
            path: Some("/posts".into()),
            ..Default::default()
        };

        let result = fetcher.fetch(&query, None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("nonexistent"));
    }

    #[test]
    fn test_clear_caches() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "_data/data.json", r#"{"v": 1}"#);

        let mut fetcher = test_fetcher(root);
        let query = DataQuery {
            file: Some("data.json".into()),
            ..Default::default()
        };

        let _ = fetcher.fetch(&query, None).unwrap();
        assert!(!fetcher.file_cache.is_empty());

        fetcher.clear_file_cache();
        assert!(fetcher.file_cache.is_empty());

        fetcher.url_cache.insert("http://test".into(), json!(null));
        assert!(!fetcher.url_cache.is_empty());
        fetcher.clear_url_cache();
        assert!(fetcher.url_cache.is_empty());
    }

    // --- Plugin integration tests ---

    #[test]
    fn test_fetch_with_plugin_registry_transforms_data() {
        use crate::plugins::registry::PluginRegistry;
        use crate::plugins::Plugin;

        #[derive(Debug)]
        struct AddFieldPlugin;

        impl Plugin for AddFieldPlugin {
            fn name(&self) -> &str {
                "add_field"
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
                            obj.insert("added".into(), json!(true));
                        }
                    }
                }
                Ok(value)
            }
        }

        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "_data/items.json", r#"[{"id": 1}, {"id": 2}]"#);

        let mut registry = PluginRegistry::new();
        registry.register(Box::new(AddFieldPlugin));

        let mut fetcher = test_fetcher(root);
        let query = DataQuery {
            file: Some("items.json".into()),
            ..Default::default()
        };

        let result = fetcher.fetch(&query, Some(&registry)).unwrap();
        let arr = result.as_array().unwrap();
        assert!(arr[0]["added"].as_bool().unwrap());
        assert!(arr[1]["added"].as_bool().unwrap());
    }

    #[test]
    fn test_fetch_with_none_plugin_registry_no_transform() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "_data/items.json", r#"[{"id": 1}]"#);

        let mut fetcher = test_fetcher(root);
        let query = DataQuery {
            file: Some("items.json".into()),
            ..Default::default()
        };

        let result = fetcher.fetch(&query, None).unwrap();
        let arr = result.as_array().unwrap();
        // No "added" field since no plugin.
        assert!(arr[0].get("added").is_none());
        assert_eq!(arr[0]["id"], 1);
    }

    #[test]
    fn test_fetch_plugin_runs_after_root_extraction() {
        use crate::plugins::registry::PluginRegistry;
        use crate::plugins::Plugin;

        #[derive(Debug)]
        struct CountPlugin;

        impl Plugin for CountPlugin {
            fn name(&self) -> &str {
                "count"
            }

            fn transform_data(
                &self,
                value: serde_json::Value,
                _source: Option<&str>,
                _path: Option<&str>,
            ) -> eyre::Result<serde_json::Value> {
                // This should receive the root-extracted value (the array),
                // NOT the full wrapper object.
                if let serde_json::Value::Array(ref arr) = value {
                    assert_eq!(arr.len(), 2, "Plugin should receive the extracted array");
                }
                Ok(value)
            }
        }

        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(
            root,
            "_data/response.json",
            r#"{"data": [{"id": 1}, {"id": 2}]}"#,
        );

        let mut registry = PluginRegistry::new();
        registry.register(Box::new(CountPlugin));

        let mut fetcher = test_fetcher(root);
        let query = DataQuery {
            file: Some("response.json".into()),
            root: Some("data".into()),
            ..Default::default()
        };

        let result = fetcher.fetch(&query, Some(&registry)).unwrap();
        assert_eq!(result.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_fetch_plugin_runs_before_filter_sort_limit() {
        use crate::plugins::registry::PluginRegistry;
        use crate::plugins::Plugin;

        /// Plugin that adds a "status" field to all items.
        #[derive(Debug)]
        struct StatusPlugin;

        impl Plugin for StatusPlugin {
            fn name(&self) -> &str {
                "status"
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
                            // Add status=published to all items.
                            obj.insert("status".into(), json!("published"));
                        }
                    }
                }
                Ok(value)
            }
        }

        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // Items don't have a "status" field — the plugin adds it.
        write(
            root,
            "_data/items.json",
            r#"[{"id": 1}, {"id": 2}, {"id": 3}]"#,
        );

        let mut registry = PluginRegistry::new();
        registry.register(Box::new(StatusPlugin));

        let mut filter = HashMap::new();
        filter.insert("status".into(), "published".into());

        let mut fetcher = test_fetcher(root);
        let query = DataQuery {
            file: Some("items.json".into()),
            filter: Some(filter),
            limit: Some(2),
            ..Default::default()
        };

        // The plugin adds "status" = "published" to all items,
        // then the filter keeps only those (all 3), then limit=2.
        let result = fetcher.fetch(&query, Some(&registry)).unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["status"], "published");
    }

    // --- Cache key differentiation tests ---

    #[test]
    fn test_cache_key_includes_method_and_body() {
        let url = "https://api.example.com/query";

        let get_key = format!("GET:{}", url);

        let body = serde_json::json!({"page_size": 100});
        let body_str = serde_json::to_string(&body).unwrap();
        let body_hash = crate::data::cache::cache_key_hash(&body_str);
        let post_key = format!("POST:{}:{}", url, body_hash);

        let post_no_body_key = format!("POST:{}:", url);

        let body2 = serde_json::json!({"page_size": 50});
        let body2_str = serde_json::to_string(&body2).unwrap();
        let body2_hash = crate::data::cache::cache_key_hash(&body2_str);
        let post_key2 = format!("POST:{}:{}", url, body2_hash);

        assert_ne!(get_key, post_key);
        assert_ne!(get_key, post_no_body_key);
        assert_ne!(post_key, post_no_body_key);
        assert_ne!(post_key, post_key2);
    }

    // --- DataCache conditional request tests ---

    #[test]
    fn fetch_source_uses_data_cache_on_304() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // Spawn a server thread that handles exactly two requests.
        let server = std::thread::spawn(move || {
            // --- First request: return 200 with body + ETag ---
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            // Consume remaining headers.
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
            }

            let body = r#"[{"id":1}]"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: \"test-etag\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body,
            );
            reader.get_mut().write_all(response.as_bytes()).unwrap();
            reader.get_mut().flush().unwrap();
            drop(reader);

            // --- Second request: expect If-None-Match, return 304 ---
            let (stream2, _) = listener.accept().unwrap();
            let mut reader2 = BufReader::new(stream2);
            let mut request_line2 = String::new();
            reader2.read_line(&mut request_line2).unwrap();
            // Read all headers and verify conditional header is present.
            let mut has_if_none_match = false;
            loop {
                let mut line = String::new();
                reader2.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                if line.to_lowercase().contains("if-none-match") {
                    has_if_none_match = true;
                }
            }
            assert!(has_if_none_match, "Second request should have If-None-Match header");

            let response304 = "HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n";
            reader2.get_mut().write_all(response304.as_bytes()).unwrap();
            reader2.get_mut().flush().unwrap();
            drop(reader2);
        });

        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Create a DataCache.
        let data_cache = crate::data::cache::DataCache::open(root).unwrap();

        // Create source config pointing at our test server.
        let mut sources = HashMap::new();
        sources.insert(
            "test_api".to_string(),
            SourceConfig {
                url: format!("http://127.0.0.1:{}", port),
                headers: HashMap::new(),
            },
        );

        let mut fetcher = DataFetcher::new(&sources, root, Some(data_cache));

        // First fetch: should get 200 with data.
        let result1 = fetcher
            .fetch_source("test_api", "/data", &HttpMethod::Get, None)
            .unwrap();
        assert_eq!(result1, serde_json::json!([{"id": 1}]));

        // Clear url_cache to force disk cache path on next fetch.
        fetcher.url_cache.clear();

        // Second fetch: server returns 304, data comes from disk cache.
        let result2 = fetcher
            .fetch_source("test_api", "/data", &HttpMethod::Get, None)
            .unwrap();
        assert_eq!(result2, serde_json::json!([{"id": 1}]));

        server.join().unwrap();
    }
}
