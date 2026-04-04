//! Data layer: global data loading, URL fetching with caching, query resolution.
//!
//! - **Global data**: load YAML/JSON files from `_data/` into a context map
//! - **DataFetcher**: fetch data from local files or remote sources with caching
//! - **Transforms**: filter, sort, and limit on fetched arrays
//! - **Nested query interpolation**: resolve `{{ item.field }}` in filter values
//! - **Query executor**: high-level entry point for resolving all data queries

mod cache;
mod fetcher;
mod global;
mod query;
mod transforms;

pub use cache::DataCache;
pub use fetcher::DataFetcher;

/// Open the data cache unless `fresh` mode is active.
///
/// Returns `None` if fresh mode is on or if the cache fails to open
/// (with a warning logged).
pub fn open_data_cache(project_root: &std::path::Path, fresh: bool) -> Option<DataCache> {
    if fresh {
        return None;
    }
    match DataCache::open(project_root) {
        Ok(cache) => Some(cache),
        Err(e) => {
            tracing::warn!("Failed to open data cache: {}", e);
            None
        }
    }
}
pub use global::load_global_data;
pub use query::{resolve_page_data, resolve_dynamic_page_data, resolve_dynamic_page_data_for_item};
