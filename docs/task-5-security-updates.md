# Task 5: Security Update Cadence, Automation, and Maintenance Plan

## 1. Upstream Cadence & Versioning Model
Chromium follows a disciplined release schedule:
- **Major Milestone Releases**: Every 4 weeks (e.g. Chrome 156 $\to$ 157).
- **Security & Bugfix Point Releases**: Bi-weekly (every 2 weeks) or emergency out-of-band updates for zero-day CVEs.

CEF (Chromium Embedded Framework) tracks upstream Chromium tags within 24–72 hours via automated builds published to the Spotify CDN and GitHub releases (`cef-project/cef`).

In Slate's Rust workspace:
- The CEF C-API binding is managed via the `cef` crate (`cef = "156.4.0+156.0.26"` in `crates/engine-adapter/Cargo.toml`).
- The semver notation `156.4.0+156.0.26` mirrors:
  - Major Chromium milestone: `156`
  - CEF build release: `4.0`
  - Chromium commit build: `156.0.26`

---

## 2. Automated Update Monitoring & Vulnerability Response
To guarantee rapid response to upstream security advisories:

1. **Automated Upstream Watcher (`scripts/check_cef_updates.py`)**:
   - A scheduled GitHub Action / cron job polls the Spotify CEF CDN (`https://cef-builds.spotifycdn.com/index.json`) and Chromium release feeds.
   - When a new CEF release is published for the current milestone (e.g. `156.4.1` with a zero-day patch) or next milestone (`157.0.0`), an automated tracking issue / PR is opened.

2. **Automated CI Regression & Smoke Suite**:
   - Every candidate CEF bump triggers `./scripts/check.sh`.
   - The test harness runs:
     1. `cargo check` and `cargo test --workspace`
     2. Live CEF smoke test (`slate-spike-cef --tabs 1 --exit-after-load`)
     3. Automated 10-site readiness gate suite (`scripts/verify_readiness_gates.py`) asserting zero bot walls, 100% selector match, and readyState `complete`.

---

## 3. ABI Stability & Breaking Change Policy
- **Minor / Point Releases within Milestone (e.g. 156.x $\to$ 156.y)**:
  - Chromium and CEF maintain strict C-API ABI backwards compatibility within minor point releases.
  - Updating requires updating the `cef` dependency version in `Cargo.toml` and updating the dynamic library path.
- **Major Milestone Upgrades (e.g. 156 $\to$ 157)**:
  - Chromium occasionally deprecates or refactors command-line switches, features, or C-API callbacks.
  - Step 1: Check upstream CEF release notes for removed/modified C API methods (`cef_life_span_handler_t`, `cef_request_context_handler_t`, etc.).
  - Step 2: Validate `on_before_command_line_processing` flags against the new Chromium milestone.
  - Step 3: Run the readiness gate test suite across all 10 real-world benchmark sites before tagging a stable release.
