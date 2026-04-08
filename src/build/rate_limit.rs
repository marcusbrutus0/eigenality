//! Build-time HTTP rate limiting.
//!
//! `RateLimiterPool` manages per-host token-bucket rate limiters using the
//! `governor` crate.  Before each outbound request, call `pool.wait(url).await` —
//! it yields until a token is available for the target host.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use governor::{Quota, RateLimiter as GovRateLimiter};
use url::Url;

use crate::config::SourceConfig;

/// A direct (non-keyed) governor rate limiter using default clock and state.
type Limiter = GovRateLimiter<
    governor::state::NotKeyed,
    governor::state::InMemoryState,
    governor::clock::DefaultClock,
>;

/// Manages per-host rate limiters for build-time HTTP requests.
///
/// Created once at build start, shared by reference across all request sites.
pub struct RateLimiterPool {
    /// Per-host limiters, created lazily on first request.
    limiters: Mutex<HashMap<String, Arc<Limiter>>>,
    /// Global default rate limit (requests per second). `None` means no limit.
    global_rate: Option<u32>,
    /// Source host → per-source rate limit override.
    source_rates: HashMap<String, u32>,
}

impl RateLimiterPool {
    /// Create a new pool from the global rate limit and source configs.
    pub fn new(
        global_rate: Option<u32>,
        sources: &HashMap<String, SourceConfig>,
    ) -> Self {
        let mut source_rates = HashMap::new();
        for source in sources.values() {
            if let Some(rate) = source.rate_limit {
                if let Some(host) = extract_host(&source.url) {
                    source_rates.insert(host, rate);
                }
            }
        }

        Self {
            limiters: Mutex::new(HashMap::new()),
            global_rate,
            source_rates,
        }
    }

    /// Yield until a token is available for the given URL's host.
    ///
    /// If no rate limit is configured (neither global nor per-source) for the
    /// host, returns immediately.
    pub async fn wait(&self, url: &str) {
        let host = match extract_host(url) {
            Some(h) => h,
            None => return,
        };

        // Per-source config overrides global; global applies to all others.
        let rate = self.source_rates.get(&host).copied().or(self.global_rate);

        let rate = match rate {
            Some(r) if r > 0 => r,
            _ => return,
        };

        // Get or create the limiter under the lock, then release before blocking.
        // Holding the lock during `until_ready` would starve all other hosts.
        let limiter = {
            let mut limiters = self.limiters.lock().expect("rate limiter lock poisoned");
            Arc::clone(limiters.entry(host).or_insert_with(|| {
                let nz_rate = NonZeroU32::new(rate).expect("rate is non-zero");
                // Burst of 1 prevents governor's default burst=rate from letting
                // all initial tokens through at once.
                let quota = Quota::per_second(nz_rate).allow_burst(NonZeroU32::MIN);
                Arc::new(GovRateLimiter::direct(quota))
            }))
        };

        limiter.until_ready().await;
    }
}

/// Extract the host from a URL string.
fn extract_host(url: &str) -> Option<String> {
    Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::config::SourceConfig;

    fn source(url: &str, rate_limit: Option<u32>) -> SourceConfig {
        SourceConfig {
            url: url.to_string(),
            headers: HashMap::new(),
            rate_limit,
        }
    }

    #[tokio::test]
    async fn no_config_means_no_wait() {
        let pool = RateLimiterPool::new(None, &HashMap::new());
        // Should return immediately — no limiter configured.
        pool.wait("https://api.example.com/data").await;
        pool.wait("https://cdn.example.com/image.png").await;
    }

    #[tokio::test]
    async fn global_rate_limit_applies() {
        let pool = RateLimiterPool::new(Some(100), &HashMap::new());
        let start = std::time::Instant::now();
        // First request should be instant (token available).
        pool.wait("https://api.example.com/data").await;
        assert!(start.elapsed() < std::time::Duration::from_millis(50));
    }

    #[tokio::test]
    async fn per_source_overrides_global() {
        let mut sources = HashMap::new();
        sources.insert("api".to_string(), source("https://api.example.com", Some(2)));
        let pool = RateLimiterPool::new(Some(100), &sources);

        // First request is free.
        pool.wait("https://api.example.com/data").await;
        let start = std::time::Instant::now();
        // Second request should be throttled at ~2/s = ~500ms wait.
        pool.wait("https://api.example.com/other").await;
        let elapsed = start.elapsed();
        assert!(
            elapsed >= std::time::Duration::from_millis(400),
            "Expected >=400ms wait for 2 req/s, got {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn different_hosts_have_independent_limiters() {
        let mut sources = HashMap::new();
        sources.insert("slow".to_string(), source("https://slow.example.com", Some(1)));
        let pool = RateLimiterPool::new(Some(100), &sources);

        pool.wait("https://slow.example.com/a").await;
        // A different host should not be affected by slow.example.com's limiter.
        let start = std::time::Instant::now();
        pool.wait("https://fast.example.com/b").await;
        assert!(start.elapsed() < std::time::Duration::from_millis(50));
    }
}
