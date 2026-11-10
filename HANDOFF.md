# Slate Browser — Session Handoff (`HANDOFF.md`)

**Last Updated**: 2026-11-10  
**Target Platform**: Arch Linux (`x86_64`, Wayland/X11) first; Windows and Android later  
**Selected Engine**: **Option A — CEF (`cef = "156.4.0+156.0.26"`, Chromium `156.0.8087.17`)**  

This document contains everything a new session needs to continue building **Slate** without prior conversation history.

---

## 1. Current Status

- **Phase 0 (Engine Prototype & Comparison)**: **Complete & Approved**
  - Full report: [`docs/phase0-report.md`](file:///home/earl/Desktop/Slate/docs/phase0-report.md)
  - Built and benchmarked both `slate-spike-cef` (CEF 156) and `slate-spike-webkit` (WebKitGTK 2.54.2) against Brave `1.99.55` (`Chromium 156.0.8087.17`).
  - Selected and approved **Option A (CEF)**.

- **Phase 1 Early Milestones & Benchmarking (Tasks 1, 1b, 2)**: **Complete & Verified**
  - Full summary report: [`docs/benchmark-summary.md`](file:///home/earl/Desktop/Slate/docs/benchmark-summary.md)
  - Raw benchmark runner & gate validator: [`scripts/verify_readiness_gates.py`](file:///home/earl/Desktop/Slate/scripts/verify_readiness_gates.py), [`scripts/bench_heavy_7runs.py`](file:///home/earl/Desktop/Slate/scripts/bench_heavy_7runs.py), [`scripts/measure_idle_breakdown.py`](file:///home/earl/Desktop/Slate/scripts/measure_idle_breakdown.py).
  - All 10 sites verified with automated readiness gates (body text length $\ge 50\%$ of Brave, readyState `complete`, 0 bot-walls).
  - 7 runs per scenario with Run 1 discarded and strict IQR non-overlap testing.
  - True 60s idle CPU measured with DevTools disconnected.
  - Scriptlets migrated from 3 prototypes to Brave's maintained `adblock-resources` (**216 scriptlets**, MPL-2.0).

- **Tasks 3, 4, 5 Architectural Reports**: **Complete & Recorded**
  - Task 3 (CEF Flag Security Audit): [`docs/task-3-security-flags.md`](file:///home/earl/Desktop/Slate/docs/task-3-security-flags.md)
  - Task 4 (H.264 / AAC Codec Strategy & Software Fallbacks): [`docs/task-4-h264-aac-codecs.md`](file:///home/earl/Desktop/Slate/docs/task-4-h264-aac-codecs.md)
  - Task 5 (CEF Security Update Cadence & Maintenance Plan): [`docs/task-5-security-updates.md`](file:///home/earl/Desktop/Slate/docs/task-5-security-updates.md)

- **Next Phase**: **Phase 1 Full Browser Shell & UI (Ready for user approval to begin)**.

---

## 2. Key Verified Measurements & Rigorous Findings

| Metric / Workload | Slate Result | Brave Result | Strict Verdict (IQR Rule) |
| :--- | :--- | :--- | :--- |
| **1 Tab (Hacker News) PSS** | **368.1 MB** (IQR $[356.6, 385.5]$) | 409.7 MB (IQR $[409.0, 410.3]$) | **Probable difference** (Slate lower by 41.6 MB) |
| **5 Tabs (Tech / News) PSS** | 758.2 MB (IQR $[620.5, 857.1]$) | 685.2 MB (IQR $[643.9, 756.4]$) | **No meaningful difference** (IQRs overlap) |
| **10 Tabs (Heavy Mixed) PSS** | 840.4 MB (IQR $[779.7, 909.8]$) | 835.1 MB (IQR $[753.9, 909.7]$) | **No meaningful difference** (IQRs overlap) |
| **10 Tabs (Sleeping / Suspended) PSS** | **423.4 MB** (IQR $[408.9, 437.9]$) | 552.0 MB (IQR $[471.2, 620.0]$) | **Probable difference** (Slate lower by 128.6 MB) |
| **1 Tab YouTube 1080p PSS** | 371.9 MB (IQR $[363.3, 381.1]$) | 372.1 MB (IQR $[367.7, 376.8]$) | **No meaningful difference** (IQRs overlap) |
| **Heavy (4K/60fps BBB + The Verge) PSS** | 1,257.5 MB (IQR $[1,208.3, 1,311.8]$) | **979.5 MB** (IQR $[808.2, 1,053.1]$) | **Probable difference** (Brave lower by 278.0 MB) |
| **Blank Tab Idle CPU (60s)** | **5.15%** of 1 core | 19.06% of 1 core | Slate lower by 13.9% CPU |
| **1 Static Page Idle CPU (60s)** | 10.16% of 1 core (0% renderer) | 15.83% of 1 core (0% renderer) | Minor gap in browser main loop |
| **10 Static Pages Idle CPU (60s)** | 99.62% of 1 core | 91.49% of 1 core | ~1 core on both due to background JS timers |
| **Filter Cache Warm Start** | **1.72 ms** (release mode) | N/A (built-in) | 8 lists + 216 scriptlets from `filters.dat` |

---

## 3. Important Rules & Gotchas
1. **Never use gdb, never dump `libcef.so` symbols**: Work strictly from git, tests, and source.
2. **Smoke test before adding C/asm linked dependencies**: The crash bisected in ADR-008 showed that linking `ring` in the main binary broke CEF's internal hooks. Always run `./scripts/check.sh` which executes the live CEF smoke test (`slate-spike-cef --tabs 1 --exit-after-load`).
3. **Never overwrite `--disable-features` or disable `MediaRouter` in CEF 156**: CEF Chrome runtime dereferences `MediaRouter` on initialization. Always append switches using `append_csv_switch`.
4. **Site Isolation must remain intact**: Never pass `--no-sandbox`, `--disable-web-security`, or `--renderer-process-limit` by default in production.

---

## 4. Immediate Next Step
Wait for the user's explicit go-ahead before beginning the **Phase 1 UI Build** (single-window multi-tab browser shell in `crates/app`).
