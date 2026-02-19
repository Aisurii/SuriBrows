//! HTML settings page generation and save URL intercept.
//!
//! Generates a dark-themed settings form rendered by Servo via `data:` URLs.
//! The save action is intercepted in [`crate::servo_glue`] via the
//! `suribrows.settings` domain pattern.

use crate::config::Config;

/// Domain used for the settings save action (intercepted in load_web_resource).
const SAVE_DOMAIN: &str = "suribrows.settings";

/// Returns `true` if the URL is a settings save request.
pub fn is_settings_save_url(url: &str) -> bool {
    url.starts_with(&format!("http://{SAVE_DOMAIN}/save"))
        || url.starts_with(&format!("https://{SAVE_DOMAIN}/save"))
}

/// Extracts query params from a save URL and builds a Config.
pub fn parse_settings_url(url: &str) -> Option<Config> {
    let query = url.split('?').nth(1)?;
    Some(Config::from_query_params(query))
}

/// Generates the settings HTML page with current config values pre-filled.
pub fn generate_settings_html(config: &Config) -> String {
    let c = config;
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>SuriBrows Settings</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    background: #1a1a1a; color: #e0e0e0;
    max-width: 700px; margin: 0 auto; padding: 24px;
}}
h1 {{ font-size: 22px; margin-bottom: 20px; color: #fff; }}
h2 {{
    font-size: 14px; text-transform: uppercase; letter-spacing: 1px;
    color: #888; margin: 24px 0 12px; padding-bottom: 6px;
    border-bottom: 1px solid #333;
}}
label {{
    display: flex; justify-content: space-between; align-items: center;
    margin-bottom: 10px; font-size: 14px;
}}
label span {{ flex: 0 0 200px; }}
input[type="text"], input[type="number"] {{
    flex: 1; background: #2a2a2a; border: 1px solid #444;
    color: #e0e0e0; padding: 6px 10px; border-radius: 4px;
    font-size: 13px; font-family: monospace;
}}
input:focus {{ border-color: #6a9eff; outline: none; }}
.toggle {{
    display: flex; justify-content: space-between; align-items: center;
    margin-bottom: 10px; font-size: 14px;
}}
.toggle input[type="checkbox"] {{
    width: 18px; height: 18px; accent-color: #6a9eff;
}}
.save-bar {{
    position: sticky; bottom: 0; background: #1a1a1a;
    padding: 16px 0; border-top: 1px solid #333; margin-top: 24px;
    display: flex; gap: 12px;
}}
button {{
    padding: 8px 24px; border: none; border-radius: 4px;
    font-size: 14px; cursor: pointer;
}}
.btn-save {{ background: #6a9eff; color: #000; font-weight: 600; }}
.btn-save:hover {{ background: #85b0ff; }}
.btn-cancel {{ background: #333; color: #e0e0e0; }}
.btn-cancel:hover {{ background: #444; }}
.note {{ font-size: 12px; color: #666; margin-top: 4px; }}
</style>
</head>
<body>
<h1>Settings</h1>

<h2>General</h2>
<label><span>Default URL</span>
<input type="text" id="default_url" value="{default_url}"></label>
<label><span>Window Title</span>
<input type="text" id="window_title" value="{window_title}"></label>

<h2>Window</h2>
<label><span>Width</span>
<input type="number" id="window_width" value="{window_width}" min="320"></label>
<label><span>Height</span>
<input type="number" id="window_height" value="{window_height}" min="240"></label>

<h2>Chrome</h2>
<label><span>Bar Height (px)</span>
<input type="number" id="chrome_height" value="{chrome_height}" min="20" max="100"></label>
<label><span>Font Size</span>
<input type="number" id="font_size" value="{font_size}" step="0.5" min="8" max="32"></label>

<h2>Search</h2>
<label><span>Search Engine URL</span>
<input type="text" id="search_engine_url" value="{search_engine_url}"></label>
<p class="note">The search query is appended to this URL.</p>

<h2>Performance</h2>
<label><span>Layout Threads</span>
<input type="number" id="layout_threads" value="{layout_threads}" min="0" max="16"></label>
<p class="note">0 = auto-detect from CPU count.</p>
<label><span>Cache Size</span>
<input type="number" id="cache_size" value="{cache_size}" min="0"></label>
<label><span>User Agent</span>
<input type="text" id="user_agent" value="{user_agent}"></label>
<p class="note">Leave empty for default privacy UA.</p>
<div class="toggle"><span>Precache Shaders</span>
<input type="checkbox" id="precache_shaders" {precache_shaders_checked}></div>

<h2>Privacy</h2>
<div class="toggle"><span>Enforce TLS (HTTPS)</span>
<input type="checkbox" id="enforce_tls" {enforce_tls_checked}></div>
<div class="toggle"><span>Disable MIME Sniffing</span>
<input type="checkbox" id="disable_mime_sniff" {disable_mime_sniff_checked}></div>
<div class="toggle"><span>Disable Geolocation</span>
<input type="checkbox" id="disable_geolocation" {disable_geolocation_checked}></div>
<div class="toggle"><span>Disable Bluetooth</span>
<input type="checkbox" id="disable_bluetooth" {disable_bluetooth_checked}></div>
<div class="toggle"><span>Disable Notifications</span>
<input type="checkbox" id="disable_notifications" {disable_notifications_checked}></div>
<div class="toggle"><span>Disable WebRTC</span>
<input type="checkbox" id="disable_webrtc" {disable_webrtc_checked}></div>

<div class="save-bar">
<button class="btn-save" onclick="save()">Save Settings</button>
<button class="btn-cancel" onclick="history.back()">Cancel</button>
</div>

<script>
function enc(s) {{ return encodeURIComponent(s); }}
function val(id) {{ return document.getElementById(id).value; }}
function chk(id) {{ return document.getElementById(id).checked; }}
function save() {{
    var q = "default_url=" + enc(val("default_url"))
        + "&window_title=" + enc(val("window_title"))
        + "&window_width=" + val("window_width")
        + "&window_height=" + val("window_height")
        + "&chrome_height=" + val("chrome_height")
        + "&font_size=" + val("font_size")
        + "&search_engine_url=" + enc(val("search_engine_url"))
        + "&layout_threads=" + val("layout_threads")
        + "&cache_size=" + val("cache_size")
        + "&user_agent=" + enc(val("user_agent"))
        + "&precache_shaders=" + chk("precache_shaders")
        + "&enforce_tls=" + chk("enforce_tls")
        + "&disable_mime_sniff=" + chk("disable_mime_sniff")
        + "&disable_geolocation=" + chk("disable_geolocation")
        + "&disable_bluetooth=" + chk("disable_bluetooth")
        + "&disable_notifications=" + chk("disable_notifications")
        + "&disable_webrtc=" + chk("disable_webrtc");
    window.location.href = "http://{save_domain}/save?" + q;
}}
</script>
</body>
</html>"#,
        default_url = html_escape(&c.general.default_url),
        window_title = html_escape(&c.general.window_title),
        window_width = c.window.width,
        window_height = c.window.height,
        chrome_height = c.chrome.height,
        font_size = c.chrome.font_size,
        search_engine_url = html_escape(&c.search.engine_url),
        layout_threads = c.servo.layout_threads,
        cache_size = c.servo.cache_size,
        user_agent = html_escape(&c.servo.user_agent),
        precache_shaders_checked = if c.servo.precache_shaders {
            "checked"
        } else {
            ""
        },
        enforce_tls_checked = if c.privacy.enforce_tls { "checked" } else { "" },
        disable_mime_sniff_checked = if c.privacy.disable_mime_sniff {
            "checked"
        } else {
            ""
        },
        disable_geolocation_checked = if c.privacy.disable_geolocation {
            "checked"
        } else {
            ""
        },
        disable_bluetooth_checked = if c.privacy.disable_bluetooth {
            "checked"
        } else {
            ""
        },
        disable_notifications_checked = if c.privacy.disable_notifications {
            "checked"
        } else {
            ""
        },
        disable_webrtc_checked = if c.privacy.disable_webrtc {
            "checked"
        } else {
            ""
        },
        save_domain = SAVE_DOMAIN,
    )
}

/// Generates a confirmation page shown after settings are saved.
pub fn generate_saved_html() -> String {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Settings Saved</title>
<style>
body {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    background: #1a1a1a; color: #e0e0e0;
    display: flex; justify-content: center; align-items: center;
    height: 100vh; flex-direction: column;
}
h1 { font-size: 24px; color: #6a9eff; margin-bottom: 12px; }
p { font-size: 16px; color: #888; }
</style>
</head>
<body>
<h1>Settings saved!</h1>
<p>Restart SuriBrows to apply changes.</p>
</body>
</html>"#
        .to_string()
}

/// Percent-encodes a string for safe embedding in data: URLs.
pub fn url_encode(input: &str) -> String {
    let mut result = String::with_capacity(input.len() * 3);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'!'
            | b'*'
            | b'('
            | b')'
            | b'\'' => result.push(byte as char),
            _ => {
                result.push('%');
                result.push(char::from(HEX_CHARS[(byte >> 4) as usize]));
                result.push(char::from(HEX_CHARS[(byte & 0xf) as usize]));
            }
        }
    }
    result
}

const HEX_CHARS: &[u8; 16] = b"0123456789ABCDEF";

/// Escapes HTML special characters in attribute values.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_settings_save_url() {
        assert!(is_settings_save_url(
            "http://suribrows.settings/save?width=1280"
        ));
        assert!(is_settings_save_url(
            "https://suribrows.settings/save?width=1280"
        ));
        assert!(!is_settings_save_url("https://example.com"));
        assert!(!is_settings_save_url("http://suribrows.settings/other"));
    }

    #[test]
    fn test_parse_settings_url() {
        let url = "http://suribrows.settings/save?window_width=1920&enforce_tls=false";
        let config = parse_settings_url(url).unwrap();
        assert_eq!(config.window.width, 1920);
        assert!(!config.privacy.enforce_tls);
    }

    #[test]
    fn test_parse_settings_url_no_query() {
        assert!(parse_settings_url("http://suribrows.settings/save").is_none());
    }

    #[test]
    fn test_generate_settings_html_contains_values() {
        let config = Config::default();
        let html = generate_settings_html(&config);
        assert!(html.contains("https://example.com"));
        assert!(html.contains("SuriBrows"));
        assert!(html.contains("1280"));
        assert!(html.contains("800"));
        assert!(html.contains("duckduckgo"));
    }

    #[test]
    fn test_generate_saved_html_not_empty() {
        let html = generate_saved_html();
        assert!(html.contains("Settings saved"));
    }

    #[test]
    fn test_url_encode_basic() {
        assert_eq!(url_encode("hello"), "hello");
        assert_eq!(url_encode("a b"), "a%20b");
        assert_eq!(url_encode("<script>"), "%3Cscript%3E");
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("a&b"), "a&amp;b");
        assert_eq!(html_escape("a\"b"), "a&quot;b");
        assert_eq!(html_escape("<div>"), "&lt;div&gt;");
    }
}
