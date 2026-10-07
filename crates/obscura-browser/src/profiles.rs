use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct BrowserProfile {
    pub user_agent: &'static str,
    pub platform: &'static str,
    pub ua_platform: &'static str,
    pub ua_platform_version: &'static str,
}

macro_rules! windows_profile {
    ($major:literal, $platform_version:literal) => {
        BrowserProfile {
            user_agent: concat!(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) ",
                "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/",
                stringify!($major),
                ".0.0.0 Safari/537.36"
            ),
            platform: "Win32",
            ua_platform: "Windows",
            ua_platform_version: $platform_version,
        }
    };
}

macro_rules! macos_profile {
    ($major:literal, $platform_version:literal) => {
        BrowserProfile {
            user_agent: concat!(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) ",
                "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/",
                stringify!($major),
                ".0.0.0 Safari/537.36"
            ),
            platform: "MacIntel",
            ua_platform: "macOS",
            ua_platform_version: $platform_version,
        }
    };
}

macro_rules! linux_profile {
    ($major:literal) => {
        BrowserProfile {
            user_agent: concat!(
                "Mozilla/5.0 (X11; Linux x86_64) ",
                "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/",
                stringify!($major),
                ".0.0.0 Safari/537.36"
            ),
            platform: "Linux x86_64",
            ua_platform: "Linux",
            // Chromium reduces Linux UA-CH platformVersion to an empty value.
            ua_platform_version: "",
        }
    };
}

// Keep the original first eight entries stable so OBSCURA_PROFILE indices used
// by existing callers retain their meaning. Additional compatible identities
// extend the same pool for stealth-mode rotation.
pub static PROFILES: &[BrowserProfile] = &[
    windows_profile!(143, "10.0.0"),
    windows_profile!(144, "10.0.0"),
    windows_profile!(145, "15.0.0"),
    windows_profile!(146, "15.0.0"),
    macos_profile!(143, "13.6.7"),
    macos_profile!(144, "14.4.1"),
    macos_profile!(145, "14.5.0"),
    macos_profile!(146, "14.6.0"),

    // Chrome 142
    windows_profile!(142, "10.0.0"),
    windows_profile!(142, "15.0.0"),
    macos_profile!(142, "13.6.7"),
    macos_profile!(142, "14.6.0"),
    macos_profile!(142, "15.5.0"),
    linux_profile!(142),

    // Additional Chrome 143 variants
    windows_profile!(143, "15.0.0"),
    macos_profile!(143, "14.6.0"),
    macos_profile!(143, "15.5.0"),
    linux_profile!(143),

    // Additional Chrome 144 variants
    windows_profile!(144, "15.0.0"),
    macos_profile!(144, "13.6.7"),
    macos_profile!(144, "15.5.0"),
    linux_profile!(144),

    // Additional Chrome 145 variants
    windows_profile!(145, "10.0.0"),
    macos_profile!(145, "13.6.7"),
    macos_profile!(145, "14.6.0"),
    macos_profile!(145, "15.5.0"),
    linux_profile!(145),

    // Additional Chrome 146 variants
    windows_profile!(146, "10.0.0"),
    macos_profile!(146, "13.6.7"),
    macos_profile!(146, "15.5.0"),
    linux_profile!(146),

    // Chrome 147
    windows_profile!(147, "10.0.0"),
    windows_profile!(147, "15.0.0"),
    macos_profile!(147, "13.6.7"),
    macos_profile!(147, "14.6.0"),
    macos_profile!(147, "15.5.0"),
    linux_profile!(147),

    // Chrome 148
    windows_profile!(148, "10.0.0"),
    windows_profile!(148, "15.0.0"),
    macos_profile!(148, "13.6.7"),
    macos_profile!(148, "14.6.0"),
    macos_profile!(148, "15.5.0"),
    linux_profile!(148),
];

static PROFILE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn random_index(len: usize) -> usize {
    if len <= 1 {
        return 0;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = PROFILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut value = (nanos as u64)
        ^ ((nanos >> 64) as u64)
        ^ ((std::process::id() as u64) << 32)
        ^ counter.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 31;
    (value as usize) % len
}

pub fn random_profile() -> &'static BrowserProfile {
    &PROFILES[random_index(PROFILES.len())]
}

pub fn profile_for_user_agent(user_agent: &str) -> Option<&'static BrowserProfile> {
    PROFILES.iter().find(|profile| profile.user_agent == user_agent)
}

/// Pick the profile for a new browser context.
///
/// Normal mode remains stable by default. Rotation there is opt-in because the
/// transport is not browser-impersonated:
///   OBSCURA_PROFILE=<index>   pin a specific profile from PROFILES
///   OBSCURA_ROTATE_PROFILE=1  pick a random profile per context
pub fn select_profile() -> &'static BrowserProfile {
    if let Some(idx) = env_profile_index("OBSCURA_PROFILE") {
        if idx < PROFILES.len() {
            return &PROFILES[idx];
        }
    }
    if env_enabled("OBSCURA_ROTATE_PROFILE") {
        return random_profile();
    }
    &PROFILES[0]
}

/// Pick a complete identity for a stealth browser context.
///
/// Stealth mode rotates by default because the selected UA/platform is also
/// propagated to the wreq TLS/HTTP emulation layer. Pinning remains available
/// for reproducible tests or long-lived sessions:
///   OBSCURA_STEALTH_PROFILE=<index>
pub fn select_stealth_profile() -> &'static BrowserProfile {
    if let Some(idx) = env_profile_index("OBSCURA_STEALTH_PROFILE") {
        if idx < PROFILES.len() {
            return &PROFILES[idx];
        }
    }
    random_profile()
}

fn env_profile_index(key: &str) -> Option<usize> {
    std::env::var(key)
        .ok()
        .as_deref()
        .map(str::trim)
        .and_then(|s| s.parse::<usize>().ok())
}

fn env_enabled(key: &str) -> bool {
    matches!(
        std::env::var(key)
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chrome_major(ua: &str) -> u16 {
        ua.split("Chrome/")
            .nth(1)
            .and_then(|tail| tail.split('.').next())
            .and_then(|major| major.parse::<u16>().ok())
            .expect("profile UA must carry a Chrome major")
    }

    #[test]
    fn stealth_pool_covers_chrome_142_through_148_on_all_desktop_platforms() {
        for major in 142..=148 {
            for platform in ["Windows", "macOS", "Linux"] {
                assert!(
                    PROFILES.iter().any(|profile| {
                        chrome_major(profile.user_agent) == major
                            && profile.ua_platform == platform
                    }),
                    "missing Chrome {major} / {platform} profile"
                );
            }
        }
    }

    #[test]
    fn profile_platform_fields_match_the_user_agent_os() {
        for profile in PROFILES {
            match profile.ua_platform {
                "Windows" => {
                    assert!(profile.user_agent.contains("(Windows NT 10.0; Win64; x64)"));
                    assert_eq!(profile.platform, "Win32");
                }
                "macOS" => {
                    assert!(profile.user_agent.contains("(Macintosh; Intel Mac OS X 10_15_7)"));
                    assert_eq!(profile.platform, "MacIntel");
                }
                "Linux" => {
                    assert!(profile.user_agent.contains("(X11; Linux x86_64)"));
                    assert_eq!(profile.platform, "Linux x86_64");
                    assert_eq!(profile.ua_platform_version, "");
                }
                other => panic!("unsupported desktop platform in profile pool: {other}"),
            }
            assert!((142..=148).contains(&chrome_major(profile.user_agent)));
        }
    }

    #[test]
    fn profile_lookup_accepts_every_pool_user_agent() {
        for profile in PROFILES {
            assert!(profile_for_user_agent(profile.user_agent).is_some());
        }
        assert!(profile_for_user_agent("Custom-UA/1.0").is_none());
    }

    #[test]
    fn pool_has_enough_base_identities_for_large_seeded_rotation() {
        // Each base identity also receives a context-stable fingerprint seed.
        // The JS layer derives OS-aware GPU, screen, hardware, memory, canvas,
        // audio, battery, and storage surfaces from that seed, so the number of
        // complete identities is far larger than the base transport count.
        assert!(PROFILES.len() >= 40);
    }
}
