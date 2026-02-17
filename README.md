# SuriBrows

![CI Status](https://github.com/Aisurii/SuriBrows/workflows/CI/badge.svg)

**A minimalist, privacy-first web browser built on Servo.** No telemetry, no tracking, no bloat. Just you and the web.

---

## What Is This?

SuriBrows is a lightweight browser I built because I was tired of Chrome phoning home to Google every 5 seconds. It's written in Rust, runs on Servo (Mozilla's experimental engine), and has ad-blocking baked in.

Think of it as: **"What if a browser just browsed the web and left you alone?"**

---

## Features

### Privacy-First
- **Built-in ad-blocking** — 142,000+ filters (EasyList + EasyPrivacy)
- **No telemetry** — Zero data collection, period
- **DuckDuckGo search** — No Google tracking
- **WebRTC disabled** — Can't leak your IP through VPNs
- **Generic user-agent** — Reduces browser fingerprinting
- **No cloud sync** — Everything stays on your machine

### Fast & Lightweight
- **25MB binary** — Chrome is 180MB
- **~200MB RAM** — Chrome uses 350MB+ for one tab
- **Hardware-accelerated** — WebRender GPU rendering
- **Startup: 1-2 seconds** — Includes ad-blocker initialization

### Security Hardening (Windows)
- **Control Flow Guard (CFG)** — Blocks ROP/JOP exploits
- **Job Object** — Kills child processes automatically
- **Image Load Policy** — Blocks remote DLL injection
- **ACG support** — Blocks shellcode (optional, breaks JavaScript)

### Simple
- **2,500 lines of Rust** — Chrome is millions of lines of C++
- **No extensions API** — No attack surface
- **No devtools** — Just browse
- **Single tab** — Keeps it focused

---

## Why Use This Instead of...

### vs. **Chrome**
- ✅ No Google telemetry
- ✅ Built-in ad-blocking
- ✅ 7× smaller binary
- ✅ Uses less RAM
- ❌ Missing: Extensions, devtools, multiple tabs

### vs. **Firefox**
- ✅ No Pocket integration
- ✅ No sponsored tiles
- ✅ Rust memory safety
- ✅ Faster startup
- ❌ Missing: Mature web compatibility, DRM support

### vs. **Brave**
- ✅ Simpler (not Chromium-based)
- ✅ Smaller binary
- ✅ No crypto wallet
- ✅ No "Brave Rewards"
- ❌ Missing: Brave's sandboxing, extension support

**Who this is for:**
Privacy nerds, developers, security researchers, anyone tired of bloated browsers.

**Who this is NOT for:**
People who need Netflix (no DRM), heavy JavaScript sites (Twitter/Discord), or Chrome extensions.

---

## Known Limitations (I'm Being Honest Here)

### Critical
- **No process sandboxing** — If Servo gets exploited, attacker owns the whole process
  *(Mitigated by: CFG, Job Object, Image Load Policy)*
- **No DRM support** — Netflix/Hulu/Disney+ won't work
  *(Can't be fixed - Servo doesn't support EME)*

### Major
- **Single tab only** — No tab bar yet (v0.2 will add this)
- **JavaScript JIT + ACG conflict** — Can't enable ACG without breaking JavaScript
  *(Waiting on Servo to expose JIT disable API)*
- **Some sites break** — Servo doesn't implement all web standards yet
  *(Twitter, Discord, heavy JS sites won't work well)*

### Minor
- **No bookmarks UI** — I just use text files
- **No history tracking** — By design (privacy)
- **No extensions** — No WebExtensions API
- **Canvas fingerprinting works** — Servo doesn't randomize canvas output
  *(Ad-blocker helps by blocking fingerprinting scripts)*

---

## Quick Start

### Prerequisites
- Rust 1.91.0+ ([install](https://rustup.rs/))
- Windows 10/11 (Linux/macOS support coming)
- ~10GB disk space (Servo dependencies are huge)

### Build & Run
```bash
# Clone the repo
git clone https://github.com/Aisurii/SuriBrows.git
cd SuriBrows

# Build (takes ~10 minutes first time)
cargo build --release

# Run
cargo run --release -- https://example.com

# With ad-block logging
RUST_LOG=debug cargo run --release -- https://cnn.com
```

### Keyboard Shortcuts
- `Ctrl+L` — Focus URL bar
- `Ctrl+R` / `F5` — Reload
- `Alt+Left` — Back
- `Alt+Right` — Forward
- `Escape` — Unfocus URL bar

---

## How Ad-Blocking Works

Every network request goes through the ad-blocker:
- **142,458 filter rules** loaded at startup
- **Blocks**: Google Analytics, Facebook trackers, ad networks, fingerprinting scripts
- **Speed**: <5ms per request (cached)

**Real test on CNN.com:**
- Without ad-blocking: **87 requests**
- With ad-blocking: **52 requests** (35 blocked!)
- Page loads **30% faster**

---

## Technical Details (For Nerds)

- **Engine:** Servo (Mozilla's Rust browser engine)
- **Windowing:** Winit 0.30
- **Rendering:** OpenGL + WebRender
- **Ad-blocking:** Brave's `adblock` crate (same engine as Brave browser)
- **Security:** Windows API mitigations (Job Object, Image Load Policy, CFG)
- **TLS:** Rustls (no OpenSSL)

**Rendering Architecture:**
Servo renders to an offscreen framebuffer, then blits to the bottom 90% of the window. I draw the URL bar in the top 40px with OpenGL.

---

## Performance

| Metric | SuriBrows | Chrome | Firefox |
|--------|-----------|--------|---------|
| **Binary size** | 25MB | 180MB | 200MB |
| **Cold start** | 1-2s | 2-3s | 2-4s |
| **RAM (1 tab)** | ~200MB | ~350MB | ~420MB |
| **Ad-blocking** | Built-in | Extension | Extension |

---

## Security Testing

```bash
# Test WebRTC leak protection
cargo run --release -- https://browserleaks.com/webrtc
# Should show "WebRTC is not available" ✅

# Test ad-blocking
RUST_LOG=debug cargo run --release -- https://cnn.com
# Look for "Requête bloquée par adblock" in logs

# Verify Control Flow Guard
dumpbin /headers target\release\suribrows.exe | findstr /i "guard"
# Should show "Guard CF Instrumented" ✅
```

---

## Roadmap

### v0.1.0 (Current) ✅
- Basic browsing
- Ad-blocking
- Privacy hardening
- Windows security policies

### v0.2.0 (Next)
- Multiple tabs
- Better keyboard shortcuts
- Cookie management UI
- Performance improvements

### v0.3.0
- Linux support
- Download manager

### v1.0.0
- Stable enough to recommend to non-technical users

---

## Contributing

1. Read the [SRS document](docs/SRS.md) to understand the architecture
2. Run `cargo fmt` and `cargo clippy` before committing
3. Test with `RUST_LOG=debug` to see what's happening
4. Keep modules under 650 lines

**Don't expect Chrome-level polish.** This is an experimental privacy browser built by one person learning Win32 APIs.

---

## FAQ

**Q: Can I watch Netflix?**
A: No. Servo doesn't support DRM (EME). Use Firefox for streaming.

**Q: Does it support extensions?**
A: No. No WebExtensions API. Ad-blocking is built-in.

**Q: Is it safe?**
A: Safer than most browsers for privacy (no telemetry), but lacks Chrome's process sandboxing. If Servo gets exploited, attacker owns the process. Mitigated by Windows security policies (CFG, Job Object).

**Q: Why Servo instead of Chromium?**
A: Chromium is 200MB, has Google telemetry baked in, and is a nightmare to compile. Servo is embeddable, Rust-based, and privacy-focused.

**Q: Will this replace my daily browser?**
A: Probably not. It's great for reading news, Wikipedia, documentation, and light browsing. For heavy JS sites (Twitter, Discord) or streaming, use Firefox/Chrome.

---

## License

*[To be determined]*

---

## Acknowledgments

- **Mozilla Servo Team** — For the Servo rendering engine
- **Brave Software** — For the `adblock` crate
- **Inter Font** — Rasmus Andersson (SIL Open Font License)
- **EasyList Community** — For maintaining filter lists

---

## Support

If you find this useful, consider supporting:
[☕ Ko-fi](https://ko-fi.com/aisuurii)


