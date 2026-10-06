use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use obscura_net::{CookieJar, ObscuraHttpClient, RobotsCache};

static FINGERPRINT_SEED_BASE: OnceLock<u32> = OnceLock::new();
static FINGERPRINT_SEED_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Allocate one fingerprint seed per browser context.
///
/// The timestamp/process mix makes the sequence differ across process starts,
/// while the odd Weyl increment guarantees distinct seeds within one process
/// until the u32 counter wraps. A context keeps this seed for its full lifetime;
/// every page, navigation, and child frame derives its spoofed surfaces from it.
fn next_fingerprint_seed() -> u32 {
    let base = *FINGERPRINT_SEED_BASE.get_or_init(|| {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let mut value = nanos ^ ((std::process::id() as u64) << 32);
        value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^= value >> 31;
        (value as u32) ^ ((value >> 32) as u32)
    });
    let index = FINGERPRINT_SEED_COUNTER.fetch_add(1, Ordering::Relaxed);
    base.wrapping_add(index.wrapping_mul(0x9e37_79b9))
}

pub struct BrowserContext {
    pub id: String,
    pub cookie_jar: Arc<CookieJar>,
    pub http_client: Arc<ObscuraHttpClient>,
    pub user_agent: String,
    pub platform: String,
    pub ua_platform: String,
    pub ua_platform_version: String,
    /// Seed for JS-visible fingerprint surfaces. Stable for the lifetime of
    /// this BrowserContext and shared by all pages/documents/frames in it.
    pub fingerprint_seed: u32,
    pub proxy_url: Option<String>,
    pub robots_cache: Arc<RobotsCache>,
    pub obey_robots: bool,
    pub stealth: bool,
    /// When true, CDP-driven navigation to file:// URLs is permitted.
    /// Default is false: a remote CDP client cannot point the browser
    /// at /etc/shadow even if Obscura is running as a privileged user.
    /// Flip on via `obscura serve --allow-file-access` for legitimate
    /// local-HTML testing workflows. Enforced by `Page` navigation itself,
    /// so every CDP and MCP route is covered; the CLI's own `obscura fetch
    /// file://...` opts its local context in. A page can never drive
    /// itself from a web origin into file:// regardless of this flag.
    pub allow_file_access: bool,
    pub storage_dir: Option<PathBuf>,
    /// localStorage is owned by the BrowserContext and partitioned by origin,
    /// matching the sharing boundary used by pages in one browser context.
    pub(crate) local_storage: Mutex<HashMap<String, HashMap<String, String>>>,
    /// Serialized state for the lightweight IndexedDB shim, partitioned by
    /// origin and owned by the BrowserContext like real IndexedDB storage.
    pub(crate) indexed_db_storage: Mutex<HashMap<String, String>>,
    /// When true, the http client allows fetching localhost / RFC1918 /
    /// link-local addresses. Set via `--allow-private-network` (issue #33).
    /// Independent of `allow_file_access` because they cover different threat
    /// models: file:// is a local file-system read, while private-network is
    /// the broader SSRF gate from issue #4.
    pub allow_private_network: bool,
}

impl BrowserContext {
    pub fn new(id: String) -> Self {
        Self::_new_inner(id, None, false, None, None, false)
    }

    /// Create a BrowserContext with an optional storage directory.
    /// When `storage_dir` is set, cookies are automatically loaded from
    /// `{storage_dir}/cookies.json` on creation.
    pub fn with_storage(
        id: String,
        storage_dir: Option<PathBuf>,
    ) -> Self {
        Self::_new_inner(id, None, false, None, storage_dir, false)
    }

    /// Create a BrowserContext with full options including storage_dir.
    pub fn with_storage_full(
        id: String,
        proxy_url: Option<String>,
        stealth: bool,
        user_agent: Option<String>,
        storage_dir: Option<PathBuf>,
    ) -> Self {
        Self::_new_inner(id, proxy_url, stealth, user_agent, storage_dir, false)
    }

    /// Variant that also accepts the `allow_private_network` opt-in. All
    /// pre-existing constructors default it to `false`; callers that want the
    /// CLI's `--allow-private-network` (issue #33) behaviour go through here.
    pub fn with_storage_and_network(
        id: String,
        proxy_url: Option<String>,
        stealth: bool,
        user_agent: Option<String>,
        storage_dir: Option<PathBuf>,
        allow_private_network: bool,
    ) -> Self {
        Self::_new_inner(id, proxy_url, stealth, user_agent, storage_dir, allow_private_network)
    }

    fn _new_inner(
        id: String,
        proxy_url: Option<String>,
        stealth: bool,
        user_agent: Option<String>,
        storage_dir: Option<PathBuf>,
        allow_private_network: bool,
    ) -> Self {
        let cookie_jar = Arc::new(CookieJar::new());

        // Restore cookies from disk if storage_dir is configured
        if let Some(ref dir) = storage_dir {
            let cookie_path = dir.join("cookies.json");
            if cookie_path.exists() {
                match cookie_jar.load_from_file(&cookie_path) {
                    Ok(n) if n > 0 => {
                        tracing::info!("Loaded {} cookies from {}", n, cookie_path.display());
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("Failed to load cookies from {}: {}", cookie_path.display(), e);
                    }
                }
            }
        }

        let mut client = ObscuraHttpClient::with_full_options(
            cookie_jar.clone(),
            proxy_url.as_deref(),
            allow_private_network,
        );
        if stealth {
            client.block_trackers = true;
        }
        let profile = crate::profiles::select_profile();
        #[cfg(feature = "stealth")]
        let resolved_ua = user_agent.unwrap_or_else(|| {
            if stealth {
                obscura_net::STEALTH_USER_AGENT.to_string()
            } else {
                profile.user_agent.to_string()
            }
        });
        #[cfg(not(feature = "stealth"))]
        let resolved_ua = user_agent.unwrap_or_else(|| profile.user_agent.to_string());

        #[cfg(feature = "stealth")]
        let (platform, ua_platform, ua_platform_version) = if stealth {
            (
                obscura_net::STEALTH_NAVIGATOR_PLATFORM.to_string(),
                obscura_net::STEALTH_UA_PLATFORM.to_string(),
                obscura_net::STEALTH_UA_PLATFORM_VERSION.to_string(),
            )
        } else {
            (
                profile.platform.to_string(),
                profile.ua_platform.to_string(),
                profile.ua_platform_version.to_string(),
            )
        };
        #[cfg(not(feature = "stealth"))]
        let (platform, ua_platform, ua_platform_version) = (
            profile.platform.to_string(),
            profile.ua_platform.to_string(),
            profile.ua_platform_version.to_string(),
        );
        let fingerprint_seed = next_fingerprint_seed();
        // Sync the http client's UA at construction so navigation requests pick it
        // up before any async setup runs. The lock has no other holders here, so
        // try_write always succeeds; we fall back silently if it ever fails.
        if let Ok(mut guard) = client.user_agent.try_write() {
            *guard = resolved_ua.clone();
        }
        let http_client = Arc::new(client);
        BrowserContext {
            id,
            cookie_jar,
            http_client,
            user_agent: resolved_ua,
            platform,
            ua_platform,
            ua_platform_version,
            fingerprint_seed,
            proxy_url,
            robots_cache: Arc::new(RobotsCache::new()),
            obey_robots: false,
            stealth,
            allow_file_access: false,
            storage_dir,
            local_storage: Mutex::new(HashMap::new()),
            indexed_db_storage: Mutex::new(HashMap::new()),
            allow_private_network,
        }
    }

    pub fn with_options(id: String, proxy_url: Option<String>, stealth: bool) -> Self {
        Self::with_full_options(id, proxy_url, stealth, None)
    }

    pub fn with_full_options(
        id: String,
        proxy_url: Option<String>,
        stealth: bool,
        user_agent: Option<String>,
    ) -> Self {
        Self::_new_inner(id, proxy_url, stealth, user_agent, None, false)
    }

    pub fn with_proxy(id: String, proxy_url: Option<String>) -> Self {
        Self::with_options(id, proxy_url, false)
    }

    pub fn effective_proxy_url(&self) -> Option<String> {
        self.http_client.effective_proxy_url()
    }

    pub fn has_proxy_credentials(&self) -> bool {
        self.http_client.has_proxy_credentials()
    }

    pub fn set_proxy_credentials(
        &self,
        username: String,
        password: String,
    ) -> Result<(), obscura_net::ObscuraNetError> {
        self.http_client.set_proxy_credentials(username, password)
    }

    /// Create a context with the same browser configuration but independent
    /// mutable network state and a fresh context-scoped fingerprint seed.
    /// Persistent copies start with the template's current cookies; incognito
    /// copies start empty and never write to the template's storage directory.
    pub fn isolated_copy(&self, id: String, persistent: bool) -> Self {
        self.isolated_copy_with_proxy(id, persistent, self.proxy_url.clone())
    }

    /// As isolated_copy, but use an explicit effective proxy. This is the
    /// ownership boundary used by CDP Target.createBrowserContext so separate
    /// browser contexts never share a proxy client or connection pool.
    pub fn isolated_copy_with_proxy(
        &self,
        id: String,
        persistent: bool,
        proxy_url: Option<String>,
    ) -> Self {
        let cookie_jar = Arc::new(CookieJar::new());
        if persistent {
            cookie_jar.copy_from(&self.cookie_jar);
        }

        let mut client = ObscuraHttpClient::with_full_options(
            cookie_jar.clone(),
            proxy_url.as_deref(),
            self.allow_private_network,
        );
        if self.stealth {
            client.block_trackers = true;
        }
        if let Ok(mut guard) = client.user_agent.try_write() {
            *guard = self.user_agent.clone();
        }

        BrowserContext {
            id,
            cookie_jar,
            http_client: Arc::new(client),
            user_agent: self.user_agent.clone(),
            platform: self.platform.clone(),
            ua_platform: self.ua_platform.clone(),
            ua_platform_version: self.ua_platform_version.clone(),
            fingerprint_seed: next_fingerprint_seed(),
            proxy_url,
            robots_cache: Arc::new(RobotsCache::new()),
            obey_robots: self.obey_robots,
            stealth: self.stealth,
            allow_file_access: self.allow_file_access,
            storage_dir: persistent.then(|| self.storage_dir.clone()).flatten(),
            local_storage: Mutex::new(if persistent {
                self.local_storage
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone()
            } else {
                HashMap::new()
            }),
            indexed_db_storage: Mutex::new(if persistent {
                self.indexed_db_storage
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone()
            } else {
                HashMap::new()
            }),
            allow_private_network: self.allow_private_network,
        }
    }

    /// Persist cookies to disk if storage_dir is configured.
    /// Called during graceful shutdown.
    pub fn save_cookies(&self) {
        if let Some(ref dir) = self.storage_dir {
            let _ = std::fs::create_dir_all(dir);
            let cookie_path = dir.join("cookies.json");
            if let Err(e) = self.cookie_jar.save_to_file(&cookie_path) {
                tracing::warn!("Failed to save cookies to {}: {}", cookie_path.display(), e);
            } else {
                tracing::info!("Saved cookies to {}", cookie_path.display());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn with_full_options_propagates_user_agent_to_http_client() {
        let ctx = BrowserContext::with_full_options(
            "test".to_string(),
            None,
            false,
            Some("Custom-UA/1.0".to_string()),
        );
        assert_eq!(ctx.user_agent, "Custom-UA/1.0");
        let client_ua = ctx.http_client.user_agent.read().await.clone();
        assert_eq!(client_ua, "Custom-UA/1.0");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn with_full_options_falls_back_to_chrome_default() {
        let ctx = BrowserContext::with_full_options(
            "test".to_string(),
            None,
            false,
            None,
        );
        assert!(ctx.user_agent.contains("Chrome"));
        let client_ua = ctx.http_client.user_agent.read().await.clone();
        assert!(client_ua.contains("Chrome"));
        assert_eq!(ctx.user_agent, client_ua);
    }

    #[cfg(feature = "stealth")]
    #[tokio::test(flavor = "current_thread")]
    async fn stealth_context_uses_transport_user_agent_as_its_identity() {
        let ctx = BrowserContext::with_options("stealth".to_string(), None, true);
        assert_eq!(ctx.user_agent, obscura_net::STEALTH_USER_AGENT);
        assert_eq!(
            ctx.http_client.user_agent.read().await.as_str(),
            obscura_net::STEALTH_USER_AGENT
        );
        assert_eq!(ctx.platform, obscura_net::STEALTH_NAVIGATOR_PLATFORM);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn with_options_keeps_default_user_agent() {
        let ctx = BrowserContext::with_options("test".to_string(), None, false);
        assert!(ctx.user_agent.contains("Chrome"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn isolated_copy_does_not_share_mutable_network_state() {
        let source = BrowserContext::with_full_options(
            "source".to_string(),
            None,
            false,
            Some("Template-UA/1.0".to_string()),
        );
        source.cookie_jar.set_cookie("sid=source", &url::Url::parse("https://example.com").unwrap());

        let persistent = source.isolated_copy("persistent".to_string(), true);
        let incognito = source.isolated_copy("incognito".to_string(), false);

        assert_eq!(persistent.cookie_jar.get_all_cookies().len(), 1);
        assert!(incognito.cookie_jar.get_all_cookies().is_empty());
        assert!(persistent
            .cookie_jar
            .get_cookie_header(&url::Url::parse("https://sub.example.com").unwrap())
            .is_empty());
        persistent.cookie_jar.clear();
        persistent.http_client.set_user_agent("Changed-UA/2.0").await;

        assert_eq!(source.cookie_jar.get_all_cookies().len(), 1);
        assert_eq!(source.http_client.user_agent.read().await.as_str(), "Template-UA/1.0");
        assert_ne!(source.fingerprint_seed, persistent.fingerprint_seed);
        assert_ne!(persistent.fingerprint_seed, incognito.fingerprint_seed);
    }

    #[test]
    fn isolated_copy_can_override_proxy_without_changing_the_source() {
        let source = BrowserContext::with_proxy(
            "source".to_string(),
            Some("http://proxy-a.example:8080".to_string()),
        );
        let copy = source.isolated_copy_with_proxy(
            "copy".to_string(),
            false,
            Some("http://proxy-b.example:8080".to_string()),
        );

        assert_eq!(source.proxy_url.as_deref(), Some("http://proxy-a.example:8080"));
        assert_eq!(copy.proxy_url.as_deref(), Some("http://proxy-b.example:8080"));
        assert_eq!(copy.http_client.proxy_url(), Some("http://proxy-b.example:8080"));
    }
}
