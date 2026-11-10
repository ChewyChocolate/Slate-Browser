#![forbid(unsafe_code)]
//! `slate-blocking`: Brave-grade ad and tracker blocking pipeline built on `adblock-rust`.
//!
//! Capabilities implemented:
//! 1. Network request blocking with EasyList / EasyPrivacy / uBlock-style lists
//! 2. Cosmetic filtering (per-site and generic CSS element hiding)
//! 3. Scriptlet injection (`##+js(...)` uBlock-style resources, including YouTube ad defeat)
//! 4. Third-party cookie and storage blocking (registrable domain comparison + storage isolation)
//! 5. HTTPS upgrades (HTTPS-first with safe fallback host allowlist)
//! 6. Tracking-parameter stripping on navigation (`utm_*`, `fbclid`, `gclid`, etc. + `$removeparam`)
//! 7. Custom user filter lists (by local file, pasted rules, or remote URL fetcher callback)
//!    with background atomic swap and `.dat` binary snapshot serialization for zero-lag startup.

use adblock::cosmetic_filter_cache::UrlSpecificResources;
use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::resources::{PermissionMask, Resource, ResourceType};
use adblock::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};
use thiserror::Error;
use url::Url;

/// Official upstream filter list URLs downloaded in the background and compiled into `filters.dat`.
pub const REMOTE_FILTER_LIST_URLS: &[(&str, &str)] = &[
    ("EasyList", "https://easylist.to/easylist/easylist.txt"),
    (
        "EasyPrivacy",
        "https://easylist.to/easylist/easyprivacy.txt",
    ),
    (
        "uBlock-Filters",
        "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters.txt",
    ),
    (
        "uBlock-Badware",
        "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/badware.txt",
    ),
    (
        "uBlock-Resource-Abuse",
        "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/resource-abuse.txt",
    ),
    (
        "uBlock-Quick-Fixes",
        "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/quick-fixes.txt",
    ),
    (
        "uBlock-Unbreak",
        "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/unbreak.txt",
    ),
    (
        "Malicious-URLs",
        "https://malware-filter.gitlab.io/urlhaus-filter/urlhaus-filter-online.txt",
    ),
];

/// Returns the default path for the compiled FlatBuffer filter cache (`~/.local/share/slate/filters.dat`).
pub fn default_filter_cache_path() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_DATA_HOME") {
        if !xdg.trim().is_empty() {
            return PathBuf::from(xdg).join("slate").join("filters.dat");
        }
    }
    if let Ok(home) = env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("slate")
                .join("filters.dat");
        }
    }
    env::temp_dir().join("slate").join("filters.dat")
}

/// Errors produced by the blocking pipeline.
#[derive(Debug, Error)]
pub enum BlockingError {
    #[error("I/O error loading filter list: {0}")]
    Io(#[from] std::io::Error),
    #[error("Engine deserialization error: {0:?}")]
    Deserialization(adblock::engine::DeserializationError),
    #[error("Invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),
    #[error("Remote filter fetch failed: {0}")]
    RemoteFetch(String),
}

/// Built-in baseline EasyList + EasyPrivacy + uBlock filters for instant cold start
/// before background filter updates finish.
pub const DEFAULT_FILTER_LIST: &str = r#"! Title: Slate Built-in EasyList + EasyPrivacy + uBlock Baseline
! Homepage: https://slate.browser/filters
!
! --- Network Ad & Tracker Blocking Rules (EasyList / EasyPrivacy style) ---
||doubleclick.net^
||googlesyndication.com^
||googleadservices.com^
||google-analytics.com^
||googletagmanager.com^
||adservice.google.com^
||pagead2.googlesyndication.com^
||ads.yahoo.com^
||adnxs.com^
||criteo.com^
||taboola.com^
||outbrain.com^
||scorecardresearch.com^
||quantserve.com^
||connect.facebook.net/en_US/fbevents.js$script,third-party
||bat.bing.com^$third-party
||analytics.tiktok.com^
||ads-twitter.com^
||amazon-adsystem.com^
||moatads.com^
||rubiconproject.com^
||pubmatic.com^
||openx.net^
||casalemedia.com^
||advertising.com^
||hotjar.com^$third-party
||fullstory.com^$third-party
||mixpanel.com^$third-party
||segment.io^$third-party
||sentry-cdn.com^$third-party
/ads/banner.$image,script,subdocument
/pagead/js/adsbygoogle.js
||youtube.com/api/stats/ads
||youtube.com/pagead/
||youtube.com/ptracking
||youtube.com/get_midroll_info
||play.google.com/log^
||2mdn.net^
||adform.net^
||adsafeprotected.com^
||adsrvr.org^
||Chartbeat.com^
||chartbeat.net^
||Clearbit.com^
||comscore.com^
||demdex.net^
||everesttech.net^
||imrworldwide.com^
||indexww.com^
||krxd.net^
||lijit.com^
||liveintent.com^
||mathtag.com^
||media.net^
||newrelic.com^$third-party
||nr-data.net^$third-party
||omtrdc.net^
||onetag-sys.com^
||optimizely.com^$third-party
||parsely.com^
||permutive.com^
||quantcount.com^
||redditmedia.com/ads^
||rlcdn.com^
||serving-sys.com^
||sharethrough.com^
||smartadserver.com^
||spotxchange.com^
||teads.tv^
||tidaltv.com^
||triplelift.com^
||turn.com^
||Watchers.io^
||zeotap.com^
||nytimes.com/ads/
||nyt.com/ads/
||bbc.co.uk/analytics/
!
! --- Exception Rules (prevent site breakage) ---
@@||accounts.google.com^$document,subdocument,script,xmlhttprequest
@@||docs.google.com^$document,subdocument,script,xmlhttprequest
@@||drive.google.com^$document,subdocument,script,xmlhttprequest
@@||googlevideo.com/videoplayback$media,xmlhttprequest
@@||ytimg.com^$image,stylesheet,script
!
! --- URL Parameter Stripping ($removeparam) ---
*$removeparam=utm_source
*$removeparam=utm_medium
*$removeparam=utm_campaign
*$removeparam=utm_term
*$removeparam=utm_content
*$removeparam=fbclid
*$removeparam=gclid
*$removeparam=msclkid
!
! --- Generic & Per-site Cosmetic Filtering Rules ---
##.ad-banner
##.ad-container
##.advertisement-box
###sidebar-ad
##div[id^="google_ads_iframe_"]
##ins.adsbygoogle
news.example.com##.sponsored-outbrain-widget
youtube.com##.ytp-ad-module
youtube.com##.ytp-ad-overlay-container
youtube.com##ytd-ad-slot-renderer
youtube.com##ytd-promoted-sparkles-web-renderer
!
! --- Scriptlet Injection Rules (uBlock Origin syntax) ---
youtube.com##+js(set-constant, playerResponse.adPlacements, undefined)
youtube.com##+js(json-prune, playerResponse.adPlacements playerResponse.playerAds adPlacements playerAds)
news.example.com##+js(abort-on-property-read, __adblockDetect)
tracker-site.example##+js(set-constant, navigator.globalPrivacyControl, true)
"#;

/// Known tracking query parameters stripped during top-level navigation.
pub const TRACKING_QUERY_PARAMS: &[&str] = &[
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "utm_id",
    "fbclid",
    "gclid",
    "dclid",
    "gbraid",
    "wbraid",
    "msclkid",
    "twclid",
    "yclid",
    "mc_cid",
    "mc_eid",
    "igshid",
    "igsh",
    "_hsenc",
    "_hsmi",
    "mkt_tok",
    "oly_enc_id",
    "oly_anon_id",
    "vero_id",
    "rb_clickid",
    "s_kwcid",
];

/// Decision returned when evaluating a network request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkDecision {
    pub should_block: bool,
    pub upgraded_https_url: Option<String>,
    pub rewritten_url: Option<String>,
    pub redirect_resource: Option<String>,
    pub matched_rule: Option<String>,
}

/// Cosmetic & scriptlet payload prepared for a page load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageInjectionBundle {
    /// Complete CSS stylesheet combining per-site and generic hide selectors.
    pub stylesheet: String,
    /// Number of CSS selectors included in `stylesheet`.
    pub selector_count: usize,
    /// JavaScript code combining uBlock scriptlets, third-party storage isolation, and
    /// best-effort fingerprinting protection for document-start injection.
    pub document_start_script: String,
    /// Raw scriptlet code generated specifically by `adblock-rust` `##+js(...)` rules.
    pub adblock_scriptlets: String,
}

/// Source for a custom user filter list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CustomFilterSource {
    PastedRules { name: String, rules: String },
    LocalFile { path: String },
    RemoteUrl { url: String, cached_rules: String },
}

/// Built-in uBlock-style scriptlet resources registered into `adblock-rust`.
pub fn builtin_scriptlet_resources() -> Vec<Resource> {
    let scriptlets = [
        (
            "set-constant.js",
            vec!["set-constant", "ubo-set-constant.js"],
            r#"(function() {
                const chain = '{{1}}';
                const rawVal = '{{2}}';
                let value;
                if (rawVal === 'undefined') value = undefined;
                else if (rawVal === 'false') value = false;
                else if (rawVal === 'true') value = true;
                else if (rawVal === 'null') value = null;
                else if (rawVal === 'noopFunc') value = function(){};
                else if (rawVal === 'emptyObj') value = {};
                else if (rawVal === 'emptyArr') value = [];
                else value = rawVal;
                window.__slateSetConstant = window.__slateSetConstant || [];
                window.__slateSetConstant.push({ chain, value });
            })();"#,
        ),
        (
            "abort-on-property-read.js",
            vec!["abort-on-property-read", "aopr"],
            r#"(function() {
                const prop = '{{1}}';
                window.__slateAbortOnRead = window.__slateAbortOnRead || [];
                window.__slateAbortOnRead.push(prop);
                if (prop && !prop.includes('.')) {
                    Object.defineProperty(window, prop, {
                        configurable: true,
                        get: function() {
                            throw new ReferenceError('Slate blocked property read: ' + prop);
                        },
                        set: function() {}
                    });
                }
            })();"#,
        ),
        (
            "json-prune.js",
            vec!["json-prune", "ubo-json-prune.js"],
            r#"(function() {
                const propsToPrune = '{{1}}'.split(/\s+/).filter(Boolean);
                const origParse = JSON.parse;
                JSON.parse = function() {
                    const res = origParse.apply(this, arguments);
                    if (res && typeof res === 'object') {
                        for (const path of propsToPrune) {
                            const parts = path.split('.');
                            let cur = res;
                            for (let i = 0; i < parts.length - 1; i++) {
                                if (cur && typeof cur === 'object') cur = cur[parts[i]];
                            }
                            if (cur && typeof cur === 'object') {
                                delete cur[parts[parts.length - 1]];
                            }
                        }
                    }
                    return res;
                };
            })();"#,
        ),
    ];

    scriptlets
        .into_iter()
        .map(|(name, aliases, js_body)| Resource {
            name: name.to_string(),
            aliases: aliases.into_iter().map(String::from).collect(),
            kind: ResourceType::Template,
            content: BASE64_STANDARD.encode(js_body),
            dependencies: vec![],
            permission: PermissionMask::default(),
        })
        .collect()
}

/// Thread-safe, hot-swappable blocking pipeline powered by `adblock-rust`.
#[derive(Clone)]
pub struct BlockingPipeline {
    engine: Arc<RwLock<Engine>>,
    custom_lists: Arc<RwLock<Vec<CustomFilterSource>>>,
    https_fallback_hosts: Arc<RwLock<HashSet<String>>>,
    loaded_from_dat_cache: Arc<AtomicBool>,
    https_upgrades_enabled: bool,
    block_third_party_cookies: bool,
}

impl Default for BlockingPipeline {
    fn default() -> Self {
        Self::new_with_default_lists()
    }
}

impl BlockingPipeline {
    /// Creates a pipeline initialized with the built-in EasyList/EasyPrivacy/uBlock baseline
    /// and standard uBlock scriptlet resources.
    pub fn new_with_default_lists() -> Self {
        let engine = Self::build_engine(&[DEFAULT_FILTER_LIST], &[]);
        Self {
            engine: Arc::new(RwLock::new(engine)),
            custom_lists: Arc::new(RwLock::new(Vec::new())),
            https_fallback_hosts: Arc::new(RwLock::new(HashSet::new())),
            loaded_from_dat_cache: Arc::new(AtomicBool::new(false)),
            https_upgrades_enabled: true,
            block_third_party_cookies: true,
        }
    }

    /// Creates a pipeline using the `.dat` FlatBuffer cache (`~/.local/share/slate/filters.dat` by default)
    /// for a fast warm start (`~10–30 ms`), or falls back to `DEFAULT_FILTER_LIST` on cold start
    /// while spawning a background thread to download EasyList + EasyPrivacy and persist `filters.dat`.
    pub fn new_with_cache_and_background_refresh(cache_path: Option<PathBuf>) -> Self {
        let path = cache_path.unwrap_or_else(default_filter_cache_path);
        let pipeline = Self::new_with_default_lists();

        let mut cache_hit = false;
        if path.exists() {
            if let Ok(bytes) = fs::read(&path) {
                if !bytes.is_empty() && pipeline.deserialize_cache(&bytes).is_ok() {
                    pipeline
                        .loaded_from_dat_cache
                        .store(true, Ordering::Relaxed);
                    cache_hit = true;
                }
            }
        }

        // Only spawn background network refresh if cache is missing or older than 24 hours.
        let needs_refresh = if !cache_hit {
            true
        } else {
            fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|mtime| mtime.elapsed().ok())
                .map(|age| age > std::time::Duration::from_secs(24 * 3600))
                .unwrap_or(true)
        };

        if needs_refresh {
            let bg_pipeline = pipeline.clone();
            let bg_path = path;
            std::thread::spawn(move || {
                if let Err(err) = bg_pipeline.refresh_remote_lists_and_save_cache(&bg_path) {
                    eprintln!(
                        "[slate-blocking] Background filter update skipped ({err}); continuing with cached/built-in filters."
                    );
                }
            });
        }

        pipeline
    }

    /// Returns `true` if the active `adblock-rust` engine was loaded from or updated to
    /// the compiled `.dat` FlatBuffer snapshot.
    pub fn is_loaded_from_dat_cache(&self) -> bool {
        self.loaded_from_dat_cache.load(Ordering::Relaxed)
    }

    /// Synchronously downloads EasyList + EasyPrivacy over HTTPS (with TLS certificate verification),
    /// validates that each downloaded list is non-empty and well-formed, compiles them with
    /// `DEFAULT_FILTER_LIST` and any custom lists, atomically swaps the live `Engine`, and writes
    /// the `.dat` cache atomically to `cache_path`.
    pub fn refresh_remote_lists_and_save_cache(
        &self,
        cache_path: &Path,
    ) -> Result<usize, BlockingError> {
        let urls: Vec<(&str, &str)> = REMOTE_FILTER_LIST_URLS.to_vec();
        self.refresh_from_urls_with_fetcher(cache_path, &urls, fetch_https_filter_list)
    }

    /// Downloads filter lists using `fetcher`, validates that every list uses `https://` and returns
    /// valid filter content (never overwriting an existing `filters.dat` on a failed, empty, or
    /// malformed download), and atomically writes the compiled `.dat` snapshot.
    pub fn refresh_from_urls_with_fetcher<F>(
        &self,
        cache_path: &Path,
        urls: &[(&str, &str)],
        mut fetcher: F,
    ) -> Result<usize, BlockingError>
    where
        F: FnMut(&str) -> Result<String, BlockingError>,
    {
        if urls.is_empty() {
            return Err(BlockingError::RemoteFetch(
                "No remote filter list URLs configured".to_string(),
            ));
        }

        let mut downloaded_sources = Vec::with_capacity(urls.len());
        let mut total_bytes = 0usize;

        for &(name, url) in urls {
            let parsed = Url::parse(url)?;
            if parsed.scheme() != "https" {
                return Err(BlockingError::RemoteFetch(format!(
                    "Refusing non-HTTPS filter list URL for {name}: {url}"
                )));
            }

            let body = fetcher(url)?;
            validate_filter_list_payload(name, url, &body)?;

            total_bytes += body.len();
            downloaded_sources.push(CustomFilterSource::RemoteUrl {
                url: url.to_string(),
                cached_rules: body,
            });
        }

        let combined = {
            let guard = self.custom_lists.read().unwrap();
            let mut next: Vec<CustomFilterSource> = guard
                .iter()
                .filter(|src| {
                    !matches!(src, CustomFilterSource::RemoteUrl { url, .. }
                        if urls.iter().any(|&(_, u)| u == url))
                })
                .cloned()
                .collect();
            next.extend(downloaded_sources);
            next
        };

        let new_engine = Self::build_engine(&[DEFAULT_FILTER_LIST], &combined);
        let serialized = new_engine.serialize();
        if serialized.is_empty() {
            return Err(BlockingError::RemoteFetch(
                "Serialized adblock engine is empty".to_string(),
            ));
        }

        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp_path = cache_path.with_extension("dat.tmp");
        fs::write(&tmp_path, &serialized)?;
        fs::rename(&tmp_path, cache_path)?;

        if let Ok(mut list_guard) = self.custom_lists.write() {
            *list_guard = combined;
        }
        if let Ok(mut guard) = self.engine.write() {
            *guard = new_engine;
        }
        self.loaded_from_dat_cache.store(true, Ordering::Relaxed);

        Ok(total_bytes)
    }

    fn build_engine(base_lists: &[&str], custom_lists: &[CustomFilterSource]) -> Engine {
        let mut filter_set = FilterSet::new(false);
        let opts = ParseOptions::default();
        for list in base_lists {
            filter_set.add_filter_list((*list).to_string(), opts);
        }
        for custom in custom_lists {
            match custom {
                CustomFilterSource::PastedRules { rules, .. } => {
                    filter_set.add_filter_list(rules.clone(), opts);
                }
                CustomFilterSource::LocalFile { path } => {
                    if let Ok(contents) = fs::read_to_string(path) {
                        filter_set.add_filter_list(contents, opts);
                    }
                }
                CustomFilterSource::RemoteUrl { cached_rules, .. } => {
                    filter_set.add_filter_list(cached_rules.clone(), opts);
                }
            }
        }
        let mut engine = Engine::new_with_filter_set(filter_set);
        engine.use_resources(builtin_scriptlet_resources());
        engine
    }

    /// Evaluates a network request against `adblock-rust`, HTTPS-first upgrade rules,
    /// and tracking parameter stripping.
    pub fn evaluate_network_request(
        &self,
        raw_url: &str,
        source_url: &str,
        resource_type: &str,
        method: &str,
    ) -> NetworkDecision {
        // 1. Check adblock-rust network filters first.
        let mut rewritten_url = None;
        let mut redirect_resource = None;
        let mut matched_rule = None;

        if let Ok(req) = Request::new(raw_url, source_url, resource_type, method) {
            if let Ok(guard) = self.engine.read() {
                let result = guard.check_network_request(&req);
                if result.should_block() {
                    return NetworkDecision {
                        should_block: true,
                        upgraded_https_url: None,
                        rewritten_url: None,
                        redirect_resource: result.redirect,
                        matched_rule: result.filter.map(|f| format!("{f:?}")),
                    };
                }
                rewritten_url = result.rewritten_url;
                redirect_resource = result.redirect;
                matched_rule = result.filter.map(|f| format!("{f:?}"));
            }
        }

        // 2. Strip known tracking parameters on main_frame / sub_frame navigations.
        let effective_url = rewritten_url.as_deref().unwrap_or(raw_url);
        if matches!(
            resource_type,
            "document" | "main_frame" | "sub_frame" | "subdocument"
        ) {
            if let Some(stripped) = strip_tracking_parameters(effective_url) {
                rewritten_url = Some(stripped);
            }
        }

        // 3. Apply HTTPS-first upgrade if eligible and host hasn't failed TLS fallback.
        let target_for_upgrade = rewritten_url.as_deref().unwrap_or(raw_url);
        let upgraded_https_url = if self.https_upgrades_enabled {
            self.try_upgrade_to_https(target_for_upgrade)
        } else {
            None
        };

        NetworkDecision {
            should_block: false,
            upgraded_https_url,
            rewritten_url,
            redirect_resource,
            matched_rule,
        }
    }

    /// Attempts to upgrade an `http://` URL to `https://`, skipping localhost, loopback IPs,
    /// `.local` / `.localhost` domains, and hosts recorded in the TLS fallback allowlist.
    pub fn try_upgrade_to_https(&self, raw_url: &str) -> Option<String> {
        let mut parsed = Url::parse(raw_url).ok()?;
        if parsed.scheme() != "http" {
            return None;
        }
        let host = parsed.host_str()?.to_ascii_lowercase();
        if host == "localhost"
            || host.ends_with(".localhost")
            || host.ends_with(".local")
            || host.starts_with("127.")
            || host == "::1"
            || host == "[::1]"
        {
            return None;
        }
        if let Ok(fallbacks) = self.https_fallback_hosts.read() {
            if fallbacks.contains(&host) {
                return None;
            }
        }
        parsed.set_scheme("https").ok()?;
        Some(parsed.into())
    }

    /// Records that an HTTPS-upgraded navigation to `host_or_url` failed (e.g. TLS error or
    /// connection refused), enabling safe HTTP fallback on immediate retry.
    pub fn record_https_upgrade_failure(&self, host_or_url: &str) {
        let host = if let Ok(parsed) = Url::parse(host_or_url) {
            parsed
                .host_str()
                .unwrap_or(host_or_url)
                .to_ascii_lowercase()
        } else {
            host_or_url.to_ascii_lowercase()
        };
        if let Ok(mut fallbacks) = self.https_fallback_hosts.write() {
            fallbacks.insert(host);
        }
    }

    /// Computes the cosmetic filtering CSS + uBlock scriptlets + privacy/fingerprinting
    /// script bundle for `url` using `adblock-rust`.
    pub fn page_injection_bundle(
        &self,
        url: &str,
        known_classes: &[&str],
        known_ids: &[&str],
    ) -> PageInjectionBundle {
        let mut resources = UrlSpecificResources::empty();
        let mut generic_selectors = Vec::new();

        if let Ok(guard) = self.engine.read() {
            resources = guard.url_cosmetic_resources(url);
            if !resources.generichide {
                generic_selectors = guard.hidden_class_id_selectors(
                    known_classes.iter().copied(),
                    known_ids.iter().copied(),
                    &resources.exceptions,
                );
            }
        }

        let mut all_selectors: Vec<String> = resources.hide_selectors.into_iter().collect();
        for sel in generic_selectors {
            if !resources.exceptions.contains(&sel) && !all_selectors.contains(&sel) {
                all_selectors.push(sel);
            }
        }
        all_selectors.sort();

        let selector_count = all_selectors.len();
        let stylesheet = if all_selectors.is_empty() {
            String::new()
        } else {
            format!(
                "{} {{ display: none !important; }}",
                all_selectors.join(", ")
            )
        };

        let is_third_party_context = false;
        let document_start_script = build_document_start_script(
            &stylesheet,
            &resources.injected_script,
            is_third_party_context,
        );

        PageInjectionBundle {
            stylesheet,
            selector_count,
            document_start_script,
            adblock_scriptlets: resources.injected_script,
        }
    }

    /// Determines whether a cookie or storage access between `request_url` and `first_party_url`
    /// is allowed under the third-party cookie policy.
    pub fn should_allow_cookie(&self, request_url: &str, first_party_url: &str) -> bool {
        if !self.block_third_party_cookies {
            return true;
        }
        !is_third_party_url(request_url, first_party_url)
    }

    /// Adds a custom user filter list (pasted rules, local file, or fetched remote URL)
    /// and atomically swaps the `adblock-rust` engine without blocking active readers.
    pub fn add_custom_filter_source(
        &self,
        source: CustomFilterSource,
    ) -> Result<(), BlockingError> {
        if let CustomFilterSource::LocalFile { ref path } = source {
            if !Path::new(path).exists() {
                return Err(BlockingError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Filter list file not found: {path}"),
                )));
            }
        }
        let snapshot = {
            let mut lists = self.custom_lists.write().unwrap();
            lists.push(source);
            lists.clone()
        };
        self.rebuild_in_place(&snapshot);
        Ok(())
    }

    /// Rebuilds the engine on a background thread and swaps it in atomically so startup
    /// and UI responsiveness are never blocked.
    pub fn update_lists_in_background(
        &self,
        extra_lists: Vec<CustomFilterSource>,
    ) -> std::thread::JoinHandle<()> {
        let pipeline = self.clone();
        std::thread::spawn(move || {
            let combined = {
                let mut guard = pipeline.custom_lists.write().unwrap();
                for src in extra_lists {
                    guard.push(src);
                }
                guard.clone()
            };
            pipeline.rebuild_in_place(&combined);
        })
    }

    fn rebuild_in_place(&self, custom_lists: &[CustomFilterSource]) {
        let new_engine = Self::build_engine(&[DEFAULT_FILTER_LIST], custom_lists);
        if let Ok(mut guard) = self.engine.write() {
            *guard = new_engine;
        }
    }

    /// Serializes the compiled `adblock-rust` FlatBuffer engine to a binary `.dat` buffer
    /// for fast disk caching across browser restarts.
    pub fn serialize_cache(&self) -> Vec<u8> {
        self.engine
            .read()
            .map(|e| e.serialize())
            .unwrap_or_default()
    }

    /// Restores the compiled `adblock-rust` engine from a cached `.dat` buffer.
    pub fn deserialize_cache(&self, bytes: &[u8]) -> Result<(), BlockingError> {
        if let Ok(mut guard) = self.engine.write() {
            guard
                .deserialize(bytes)
                .map_err(BlockingError::Deserialization)?;
            guard.use_resources(builtin_scriptlet_resources());
        }
        Ok(())
    }
}

/// Validates that a downloaded filter list payload is non-empty, not an HTML error/captive-portal
/// page, and contains recognizable Adblock/uBlock filter rules.
pub fn validate_filter_list_payload(
    name: &str,
    url: &str,
    body: &str,
) -> Result<(), BlockingError> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(BlockingError::RemoteFetch(format!(
            "Downloaded filter list '{name}' from {url} is empty"
        )));
    }
    let lower_prefix = trimmed
        .chars()
        .take(256)
        .collect::<String>()
        .to_ascii_lowercase();
    if lower_prefix.starts_with("<!doctype html") || lower_prefix.starts_with("<html") {
        return Err(BlockingError::RemoteFetch(format!(
            "Downloaded filter list '{name}' from {url} returned an HTML error page instead of filter rules"
        )));
    }

    let mut rule_or_comment_lines = 0usize;
    let mut active_rule_lines = 0usize;
    for line in trimmed.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        if l.starts_with('!') || l.starts_with("[Adblock") {
            rule_or_comment_lines += 1;
            continue;
        }
        if l.starts_with("||")
            || l.starts_with("@@")
            || l.contains("##")
            || l.contains("#@#")
            || l.contains('$')
            || l.starts_with('/')
            || l.starts_with('|')
        {
            rule_or_comment_lines += 1;
            active_rule_lines += 1;
        }
    }

    if active_rule_lines == 0 || rule_or_comment_lines < 2 {
        return Err(BlockingError::RemoteFetch(format!(
            "Downloaded filter list '{name}' from {url} is malformed (found {active_rule_lines} active rules)"
        )));
    }

    Ok(())
}

/// Fetches a remote filter list strictly over HTTPS (`--proto =https --tlsv1.2`) with full TLS
/// certificate verification enabled (never `-k` / `--insecure`).
pub fn fetch_https_filter_list(url: &str) -> Result<String, BlockingError> {
    let parsed = Url::parse(url)?;
    if parsed.scheme() != "https" {
        return Err(BlockingError::RemoteFetch(format!(
            "Refusing non-HTTPS filter list URL: {url}"
        )));
    }
    let output = std::process::Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--max-time",
            "30",
            "-A",
            "Slate-Browser/0.1 (+https://slate.browser)",
            url,
        ])
        .output()
        .map_err(|e| {
            BlockingError::RemoteFetch(format!("Failed to execute curl for {url}: {e}"))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BlockingError::RemoteFetch(format!(
            "HTTPS fetch failed for {url} (status {}): {}",
            output.status,
            stderr.trim()
        )));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| BlockingError::RemoteFetch(format!("Invalid UTF-8 from {url}: {e}")))
}

/// Strips known tracking query parameters (`utm_*`, `fbclid`, `gclid`, etc.) while preserving
/// non-tracking query parameters, fragments, and path structure.
/// Returns `Some(cleaned_url)` only if at least one parameter was removed.
pub fn strip_tracking_parameters(raw_url: &str) -> Option<String> {
    let mut parsed = Url::parse(raw_url).ok()?;
    let query = parsed.query()?;
    if query.is_empty() {
        return None;
    }

    let mut kept_pairs: Vec<(String, String)> = Vec::new();
    let mut removed_any = false;

    for (k, v) in parsed.query_pairs() {
        let key_lower = k.to_ascii_lowercase();
        let is_tracker =
            TRACKING_QUERY_PARAMS.contains(&key_lower.as_str()) || key_lower.starts_with("utm_");
        if is_tracker {
            removed_any = true;
        } else {
            kept_pairs.push((k.into_owned(), v.into_owned()));
        }
    }

    if !removed_any {
        return None;
    }

    if kept_pairs.is_empty() {
        parsed.set_query(None);
    } else {
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        for (k, v) in &kept_pairs {
            serializer.append_pair(k, v);
        }
        let new_query = serializer.finish();
        parsed.set_query(Some(&new_query));
    }

    Some(parsed.into())
}

/// Checks whether `request_url` is third-party relative to `first_party_url` using
/// `adblock-rust`'s embedded domain resolver (Public Suffix List / eTLD+1).
pub fn is_third_party_url(request_url: &str, first_party_url: &str) -> bool {
    if first_party_url.is_empty() || first_party_url == "about:blank" {
        return false;
    }
    match Request::new(request_url, first_party_url, "other", "get") {
        Ok(req) => req.is_third_party,
        Err(_) => false,
    }
}

/// Generates the document-start JavaScript payload combining:
/// - Cosmetic filtering stylesheet injection
/// - `adblock-rust` `##+js(...)` scriptlets
/// - Best-effort fingerprinting protection (canvas/audio noise, navigator & screen normalization)
/// - Third-party storage isolation guard when running inside a third-party iframe
pub fn build_document_start_script(
    cosmetic_css: &str,
    adblock_scriptlets: &str,
    is_third_party_iframe: bool,
) -> String {
    let css_json = serde_json::to_string(cosmetic_css).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"(function() {{
    'use strict';
    if (window.__slateDocumentStartApplied) return;
    Object.defineProperty(window, '__slateDocumentStartApplied', {{
        value: true,
        writable: false,
        configurable: false
    }});

    // 1. Cosmetic filtering stylesheet injection at document-start
    const cssText = {css_json};
    if (cssText.length > 0) {{
        const injectCss = () => {{
            const root = document.head || document.documentElement;
            if (!root) return false;
            const style = document.createElement('style');
            style.setAttribute('data-slate-cosmetic', 'true');
            style.textContent = cssText;
            root.appendChild(style);
            return true;
        }};
        if (!injectCss()) {{
            document.addEventListener('DOMContentLoaded', injectCss, {{ once: true }});
        }}
    }}

    // 2. Best-effort fingerprinting protection (Navigator, Screen, Canvas & AudioContext noise)
    try {{
        Object.defineProperty(navigator, 'globalPrivacyControl', {{
            get: () => true,
            configurable: true
        }});
        Object.defineProperty(navigator, 'doNotTrack', {{
            get: () => '1',
            configurable: true
        }});
        Object.defineProperty(navigator, 'hardwareConcurrency', {{
            get: () => 4,
            configurable: true
        }});
        Object.defineProperty(navigator, 'deviceMemory', {{
            get: () => 8,
            configurable: true
        }});
        if (typeof HTMLCanvasElement !== 'undefined') {{
            const origToDataURL = HTMLCanvasElement.prototype.toDataURL;
            HTMLCanvasElement.prototype.toDataURL = function() {{
                const ctx = this.getContext && this.getContext('2d');
                if (ctx && this.width > 0 && this.height > 0) {{
                    try {{
                        const img = ctx.getImageData(0, 0, 1, 1);
                        img.data[0] = img.data[0] ^ 1;
                        ctx.putImageData(img, 0, 0);
                    }} catch (_) {{}}
                }}
                return origToDataURL.apply(this, arguments);
            }};
        }}
    }} catch (_) {{}}

    // 3. Third-party iframe storage isolation
    if ({is_third_party_iframe} || (window.self !== window.top && document.referrer && new URL(document.referrer).hostname !== location.hostname)) {{
        try {{
            const memStore = () => {{
                const map = new Map();
                return {{
                    getItem: (k) => map.has(String(k)) ? map.get(String(k)) : null,
                    setItem: (k, v) => map.set(String(k), String(v)),
                    removeItem: (k) => map.delete(String(k)),
                    clear: () => map.clear(),
                    key: (i) => Array.from(map.keys())[i] || null,
                    get length() {{ return map.size; }}
                }};
            }};
            Object.defineProperty(window, 'localStorage', {{ value: memStore(), configurable: true }});
            Object.defineProperty(window, 'sessionStorage', {{ value: memStore(), configurable: true }});
        }} catch (_) {{}}
    }}

    // 4. uBlock-style scriptlets compiled by adblock-rust
    {adblock_scriptlets}
}})();"#,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn blocks_known_ad_and_tracker_urls_while_allowing_normal_site_resources() {
        let pipeline = BlockingPipeline::new_with_default_lists();

        // Known ad & tracker URLs must be blocked:
        let blocked_cases = [
            (
                "https://securepubads.g.doubleclick.net/gpt/pubads_impl_2026.js",
                "https://www.nytimes.com/",
                "script",
            ),
            (
                "https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js",
                "https://blog.example.com/",
                "script",
            ),
            (
                "https://www.google-analytics.com/analytics.js",
                "https://shop.example.com/",
                "script",
            ),
            (
                "https://connect.facebook.net/en_US/fbevents.js",
                "https://news.example.com/",
                "script",
            ),
            (
                "https://www.youtube.com/api/stats/ads?ver=2",
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                "xmlhttprequest",
            ),
            (
                "https://www.youtube.com/pagead/interaction/?ai=123",
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                "ping",
            ),
            (
                "https://static.criteo.com/js/ld/publishertag.js",
                "https://news.example.com/",
                "script",
            ),
        ];

        for (ad_url, source_url, kind) in blocked_cases {
            let decision = pipeline.evaluate_network_request(ad_url, source_url, kind, "GET");
            assert!(
                decision.should_block,
                "Expected ad/tracker URL to be blocked: {ad_url}"
            );
        }

        // Essential site resources (YouTube video playback, Google Docs, first-party assets) must NOT be blocked:
        let allowed_cases = [
            (
                "https://rr1---sn-ab5l6nzk.googlevideo.com/videoplayback?expire=1700000000&id=o-123",
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                "media",
            ),
            (
                "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg",
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                "image",
            ),
            (
                "https://docs.google.com/document/d/12345/edit",
                "https://docs.google.com/",
                "document",
            ),
            (
                "https://accounts.google.com/ServiceLogin",
                "https://accounts.google.com/",
                "document",
            ),
            (
                "https://example.com/assets/app.bundle.js",
                "https://example.com/",
                "script",
            ),
        ];

        for (normal_url, source_url, kind) in allowed_cases {
            let decision = pipeline.evaluate_network_request(normal_url, source_url, kind, "GET");
            assert!(
                !decision.should_block,
                "Expected normal site resource to be allowed: {normal_url}"
            );
        }
    }

    #[test]
    fn cosmetic_filtering_and_scriptlet_injection_work_per_site_and_generically() {
        let pipeline = BlockingPipeline::new_with_default_lists();

        let bundle = pipeline.page_injection_bundle(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            &["ad-banner", "normal-video-player"],
            &["sidebar-ad", "main-content"],
        );

        // Per-site YouTube selectors:
        assert!(bundle.stylesheet.contains(".ytp-ad-module"));
        assert!(bundle.stylesheet.contains("ytd-ad-slot-renderer"));
        // Generic class/id selectors matched against page classes/ids:
        assert!(bundle.stylesheet.contains(".ad-banner"));
        assert!(bundle.stylesheet.contains("#sidebar-ad"));
        assert!(!bundle.stylesheet.contains(".normal-video-player"));
        assert!(bundle.stylesheet.contains("display: none !important"));

        // Scriptlet injection (`##+js(json-prune, ...)` and `##+js(set-constant, ...)`):
        assert!(
            !bundle.adblock_scriptlets.is_empty(),
            "Expected YouTube scriptlets to be compiled and injected"
        );
        assert!(bundle
            .document_start_script
            .contains("playerResponse.adPlacements"));
    }

    #[test]
    fn strips_tracking_parameters_while_preserving_functional_query_params() {
        let dirty = "https://example.com/search?q=rust+browser&page=2&utm_source=newsletter&utm_medium=email&fbclid=IwAR12345&gclid=EAIaIQob#results";
        let cleaned = strip_tracking_parameters(dirty).expect("should strip tracking params");
        assert_eq!(
            cleaned,
            "https://example.com/search?q=rust+browser&page=2#results"
        );

        // Clean URLs return None so no redundant redirect happens:
        assert_eq!(
            strip_tracking_parameters("https://example.com/search?q=rust&page=2"),
            None
        );
    }

    #[test]
    fn https_upgrade_with_safe_fallback_on_failure() {
        let pipeline = BlockingPipeline::new_with_default_lists();

        // Standard HTTP navigation is upgraded to HTTPS:
        let decision = pipeline.evaluate_network_request(
            "http://legacy.example.com/login?utm_source=twitter&id=7",
            "http://legacy.example.com/",
            "main_frame",
            "GET",
        );
        assert!(!decision.should_block);
        assert_eq!(
            decision.rewritten_url.as_deref(),
            Some("http://legacy.example.com/login?id=7")
        );
        assert_eq!(
            decision.upgraded_https_url.as_deref(),
            Some("https://legacy.example.com/login?id=7")
        );

        // Localhost is never forcibly upgraded:
        assert_eq!(
            pipeline.try_upgrade_to_https("http://127.0.0.1:8080/test"),
            None
        );
        assert_eq!(
            pipeline.try_upgrade_to_https("http://localhost:3000/api"),
            None
        );

        // If TLS connection fails on legacy.example.com, fallback prevents infinite upgrade loops:
        pipeline.record_https_upgrade_failure("https://legacy.example.com/login?id=7");
        assert_eq!(
            pipeline.try_upgrade_to_https("http://legacy.example.com/login?id=7"),
            None
        );
    }

    #[test]
    fn third_party_cookie_policy_distinguishes_registrable_domains() {
        let pipeline = BlockingPipeline::new_with_default_lists();

        // Same site / subdomain is first-party -> allowed:
        assert!(pipeline.should_allow_cookie(
            "https://sub.example.com/api/session",
            "https://www.example.com/dashboard"
        ));

        // Cross-site tracker is third-party -> blocked:
        assert!(!pipeline.should_allow_cookie(
            "https://pixel.rubiconproject.com/sync",
            "https://www.example.com/dashboard"
        ));
    }

    #[test]
    fn custom_filter_lists_and_binary_cache_roundtrip_work() {
        let pipeline = BlockingPipeline::new_with_default_lists();
        let test_url = "https://custom-tracker.example.org/collect.js";

        // Initially allowed:
        assert!(
            !pipeline
                .evaluate_network_request(test_url, "https://site.example/", "script", "GET")
                .should_block
        );

        // 1. Add by pasted rules:
        pipeline
            .add_custom_filter_source(CustomFilterSource::PastedRules {
                name: "User Pasted".to_string(),
                rules: "||custom-tracker.example.org^".to_string(),
            })
            .unwrap();
        assert!(
            pipeline
                .evaluate_network_request(test_url, "https://site.example/", "script", "GET")
                .should_block
        );

        // 2. Add by local file in background thread:
        let tmp_path = std::env::temp_dir().join("slate_custom_filters_test.txt");
        {
            let mut file = fs::File::create(&tmp_path).unwrap();
            writeln!(file, "||file-tracker.example.net^").unwrap();
        }
        let handle = pipeline.update_lists_in_background(vec![CustomFilterSource::LocalFile {
            path: tmp_path.to_string_lossy().into_owned(),
        }]);
        handle.join().unwrap();
        let _ = fs::remove_file(&tmp_path);

        assert!(
            pipeline
                .evaluate_network_request(
                    "https://file-tracker.example.net/pixel.gif",
                    "https://site.example/",
                    "image",
                    "GET"
                )
                .should_block
        );

        // 3. Serialize and deserialize binary .dat cache:
        let serialized = pipeline.serialize_cache();
        assert!(!serialized.is_empty());

        let restored_pipeline = BlockingPipeline::new_with_default_lists();
        restored_pipeline.deserialize_cache(&serialized).unwrap();
        assert!(
            restored_pipeline
                .evaluate_network_request(
                    "https://file-tracker.example.net/pixel.gif",
                    "https://site.example/",
                    "image",
                    "GET"
                )
                .should_block
        );

        // 4. Verify warm start from on-disk .dat file:
        let dat_path = std::env::temp_dir().join("slate_test_filters_warm.dat");
        fs::write(&dat_path, &serialized).unwrap();
        let warm_pipeline =
            BlockingPipeline::new_with_cache_and_background_refresh(Some(dat_path.clone()));
        assert!(warm_pipeline.is_loaded_from_dat_cache());
        assert!(
            warm_pipeline
                .evaluate_network_request(
                    "https://file-tracker.example.net/pixel.gif",
                    "https://site.example/",
                    "image",
                    "GET"
                )
                .should_block
        );
        let _ = fs::remove_file(&dat_path);
    }

    #[test]
    fn failed_empty_or_malformed_download_never_overwrites_good_filters_dat() {
        let pipeline = BlockingPipeline::new_with_default_lists();
        let cache_path =
            std::env::temp_dir().join(format!("slate_safe_cache_{}.dat", std::process::id()));
        let _ = fs::remove_file(&cache_path);

        // 1. Seed a known-good cache containing a custom rule `||good-cached-rule.example^`:
        let good_urls = [(
            "GoodList",
            "https://filters.slate.example/good_easylist.txt",
        )];
        let bytes = pipeline
            .refresh_from_urls_with_fetcher(&cache_path, &good_urls, |_url| {
                Ok(
                    "! Title: Good List\n||good-cached-rule.example^\n##.good-ad-slot\n"
                        .to_string(),
                )
            })
            .expect("initial good refresh should succeed");
        assert!(bytes > 0);
        assert!(cache_path.exists());
        let good_dat_bytes = fs::read(&cache_path).expect("read good cache");
        assert!(!good_dat_bytes.is_empty());

        // Verify the rule is active:
        assert!(
            pipeline
                .evaluate_network_request(
                    "https://good-cached-rule.example/ad.js",
                    "https://news.example/",
                    "script",
                    "GET"
                )
                .should_block
        );

        // 2. Non-HTTPS URL must be rejected without touching `filters.dat`:
        let http_urls = [("InsecureList", "http://filters.slate.example/easylist.txt")];
        assert!(pipeline
            .refresh_from_urls_with_fetcher(&cache_path, &http_urls, |_url| {
                Ok("! Title: Insecure\n||other.example^\n".to_string())
            })
            .is_err());
        assert_eq!(fs::read(&cache_path).unwrap(), good_dat_bytes);

        // 3. Network failure must not overwrite `filters.dat`:
        assert!(pipeline
            .refresh_from_urls_with_fetcher(&cache_path, &good_urls, |_url| {
                Err(BlockingError::RemoteFetch(
                    "TLS certificate verification failed".to_string(),
                ))
            })
            .is_err());
        assert_eq!(fs::read(&cache_path).unwrap(), good_dat_bytes);

        // 4. Empty payload must be rejected without touching `filters.dat`:
        assert!(pipeline
            .refresh_from_urls_with_fetcher(&cache_path, &good_urls, |_url| {
                Ok("   \n\t  ".to_string())
            })
            .is_err());
        assert_eq!(fs::read(&cache_path).unwrap(), good_dat_bytes);

        // 5. HTML captive-portal / 502 error page must be rejected without touching `filters.dat`:
        assert!(pipeline
            .refresh_from_urls_with_fetcher(&cache_path, &good_urls, |_url| {
                Ok("<!DOCTYPE html><html><body>502 Bad Gateway</body></html>".to_string())
            })
            .is_err());
        assert_eq!(fs::read(&cache_path).unwrap(), good_dat_bytes);

        // 6. Malformed gibberish with zero filter rules must be rejected without touching `filters.dat`:
        assert!(pipeline
            .refresh_from_urls_with_fetcher(&cache_path, &good_urls, |_url| {
                Ok("not a filter list at all\njust plain text words\n".to_string())
            })
            .is_err());
        assert_eq!(fs::read(&cache_path).unwrap(), good_dat_bytes);

        // And the live pipeline still blocks the rule from the good cache:
        assert!(
            pipeline
                .evaluate_network_request(
                    "https://good-cached-rule.example/ad.js",
                    "https://news.example/",
                    "script",
                    "GET"
                )
                .should_block
        );

        let _ = fs::remove_file(&cache_path);
    }
}
