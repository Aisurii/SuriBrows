//! Servo engine preferences tuned for privacy and performance.
//!
//! Builds a [`servo::Preferences`] struct with hardened defaults:
//! - Thread pools sized to available CPU cores (clamped)
//! - HTTPS enforcement, MIME sniff disabled
//! - Tracking APIs disabled (geolocation, Bluetooth, WebRTC, notifications)
//! - Generic Chrome user-agent to reduce fingerprinting
//!
//! All values are driven by [`crate::config::ServoConfig`] and
//! [`crate::config::PrivacyConfig`] so users can tune them from `config.toml`.

use crate::config::{PrivacyConfig, ServoConfig};
use tracing::{info, warn};

/// Default privacy-oriented user agent (used when config UA is empty).
const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Builds Servo `Preferences` from the user's config sections.
///
/// `servo_cfg.layout_threads == 0` means auto-detect from CPU count.
/// An empty `servo_cfg.user_agent` falls back to the default privacy UA.
#[allow(clippy::field_reassign_with_default)]
pub fn build_servo_preferences(
    servo_cfg: &ServoConfig,
    privacy_cfg: &PrivacyConfig,
) -> servo::Preferences {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get() as i64)
        .unwrap_or(4);

    let mut prefs = servo::Preferences::default();

    // ── Performance Tuning ────────────────────────────────────────────────
    prefs.layout_threads = if servo_cfg.layout_threads == 0 {
        cpus.min(8) // auto-detect
    } else {
        servo_cfg.layout_threads.min(8)
    };
    prefs.threadpools_async_runtime_workers_max = (cpus * 2).min(16);
    prefs.threadpools_image_cache_workers_max = cpus.min(8);
    prefs.threadpools_webrender_workers_max = (cpus / 2).clamp(2, 8);
    prefs.threadpools_resource_workers_max = cpus.min(8);
    prefs.network_http_cache_size = servo_cfg.cache_size.max(0) as u64;
    prefs.gfx_precache_shaders = servo_cfg.precache_shaders;

    // ── Privacy & Security Settings ───────────────────────────────────────

    // User Agent: use config value, or default privacy UA if empty
    prefs.user_agent = if servo_cfg.user_agent.is_empty() {
        DEFAULT_USER_AGENT.to_string()
    } else {
        servo_cfg.user_agent.clone()
    };

    // Network Security
    prefs.network_enforce_tls_enabled = privacy_cfg.enforce_tls;
    prefs.network_mime_sniff = !privacy_cfg.disable_mime_sniff;

    // Privacy Features - Tracking APIs
    prefs.dom_geolocation_enabled = !privacy_cfg.disable_geolocation;
    prefs.dom_bluetooth_enabled = !privacy_cfg.disable_bluetooth;
    prefs.dom_notification_enabled = !privacy_cfg.disable_notifications;

    // WebRTC: can reveal local/public IP even through VPN via STUN
    prefs.dom_webrtc_enabled = !privacy_cfg.disable_webrtc;

    // Keep enabled for compatibility (balanced mode)
    // - dom_cookiestore_enabled: true (default) - needed for logins
    // - dom_indexeddb_enabled: true (default) - needed for web apps

    // NOTE: Servo doesn't expose these privacy preferences yet:
    // - Referrer policy control (would use strict-origin-when-cross-origin)
    // - Third-party cookie blocking
    // - Canvas fingerprinting randomization
    // - WebRTC IP leak prevention (only full disable available)
    // Ad-blocking via filter lists compensates for some of these gaps.

    // SECURITY: Disable JIT if --secure-mode flag is set
    // This is REQUIRED for ACG (Arbitrary Code Guard) to work.
    // ACG forbids runtime code generation, which conflicts with JavaScript JIT.
    let args: Vec<String> = std::env::args().collect();
    if args.contains(&"--secure-mode".to_string()) {
        // LIMITATION: Servo doesn't expose a JIT disable preference in the Preferences struct.
        // The field "js_jit_content_enabled" doesn't exist in the current Servo version.
        // Possible field names checked: js_jit_content_enabled, js_jit_enabled, dom_jit_enabled
        //
        // IMPACT: --secure-mode will enable ACG without disabling JIT, causing an IMMEDIATE CRASH
        // when JavaScript is executed. This is a known limitation until Servo exposes JIT control.
        //
        // WORKAROUND OPTIONS:
        // 1. File upstream Servo issue requesting js.jit.content preference exposure
        // 2. Disable ACG entirely (remove apply_dynamic_code_policy call in security.rs)
        // 3. Only use --secure-mode on sites without JavaScript
        //
        // For now, we'll warn the user and still enable ACG (will crash on JS execution).
        warn!(
            "⚠️  --secure-mode enabled but Servo doesn't expose JIT disable preference. \
             Browser will crash when loading JavaScript. \
             Only use --secure-mode on static HTML sites."
        );
    }

    info!(
        cpus,
        layout_threads = prefs.layout_threads,
        network_workers = prefs.threadpools_async_runtime_workers_max,
        cache_size = prefs.network_http_cache_size,
        tls_enforced = prefs.network_enforce_tls_enabled,
        "Servo preferences configured (performance + privacy)"
    );

    prefs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_prefs() -> servo::Preferences {
        build_servo_preferences(&ServoConfig::default(), &PrivacyConfig::default())
    }

    #[test]
    fn test_preferences_layout_threads_bounded() {
        let prefs = default_prefs();
        assert!(prefs.layout_threads >= 1, "layout_threads should be >= 1");
        assert!(prefs.layout_threads <= 8, "layout_threads should be <= 8");
    }

    #[test]
    fn test_preferences_tls_enforced() {
        let prefs = default_prefs();
        assert!(prefs.network_enforce_tls_enabled);
    }

    #[test]
    fn test_preferences_mime_sniff_disabled() {
        let prefs = default_prefs();
        assert!(!prefs.network_mime_sniff);
    }

    #[test]
    fn test_preferences_geolocation_disabled() {
        let prefs = default_prefs();
        assert!(!prefs.dom_geolocation_enabled);
    }

    #[test]
    fn test_preferences_bluetooth_disabled() {
        let prefs = default_prefs();
        assert!(!prefs.dom_bluetooth_enabled);
    }

    #[test]
    fn test_preferences_webrtc_disabled() {
        let prefs = default_prefs();
        assert!(!prefs.dom_webrtc_enabled);
    }

    #[test]
    fn test_preferences_notification_disabled() {
        let prefs = default_prefs();
        assert!(!prefs.dom_notification_enabled);
    }

    #[test]
    fn test_preferences_user_agent_set() {
        let prefs = default_prefs();
        assert!(
            prefs.user_agent.contains("Chrome"),
            "UA should contain Chrome"
        );
        assert!(!prefs.user_agent.is_empty(), "UA should not be empty");
    }

    #[test]
    fn test_preferences_cache_size() {
        let prefs = default_prefs();
        assert_eq!(prefs.network_http_cache_size, 50_000);
    }

    #[test]
    fn test_preferences_precache_shaders() {
        let prefs = default_prefs();
        assert!(prefs.gfx_precache_shaders);
    }

    #[test]
    fn test_preferences_webrender_workers_bounded() {
        let prefs = default_prefs();
        assert!(prefs.threadpools_webrender_workers_max >= 2);
        assert!(prefs.threadpools_webrender_workers_max <= 8);
    }

    #[test]
    fn test_preferences_custom_layout_threads() {
        let servo_cfg = ServoConfig {
            layout_threads: 4,
            ..Default::default()
        };
        let prefs = build_servo_preferences(&servo_cfg, &PrivacyConfig::default());
        assert_eq!(prefs.layout_threads, 4);
    }

    #[test]
    fn test_preferences_custom_user_agent() {
        let servo_cfg = ServoConfig {
            user_agent: "MyBrowser/1.0".to_string(),
            ..Default::default()
        };
        let prefs = build_servo_preferences(&servo_cfg, &PrivacyConfig::default());
        assert_eq!(prefs.user_agent, "MyBrowser/1.0");
    }

    #[test]
    fn test_preferences_privacy_toggles_off() {
        let privacy_cfg = PrivacyConfig {
            enforce_tls: false,
            disable_mime_sniff: false,
            disable_geolocation: false,
            disable_bluetooth: false,
            disable_notifications: false,
            disable_webrtc: false,
        };
        let prefs = build_servo_preferences(&ServoConfig::default(), &privacy_cfg);
        assert!(!prefs.network_enforce_tls_enabled);
        assert!(prefs.network_mime_sniff); // sniff enabled when toggle is off
        assert!(prefs.dom_geolocation_enabled);
        assert!(prefs.dom_bluetooth_enabled);
        assert!(prefs.dom_notification_enabled);
        assert!(prefs.dom_webrtc_enabled);
    }
}
