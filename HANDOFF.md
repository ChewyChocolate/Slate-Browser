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
  - Raw benchmark runner streaming to disk: [`scripts/run_7pass_benchmarks.py`](file:///home/earl/Desktop/Slate/scripts/run_7pass_benchmarks.py) $\to$ [`benchmarks/results/realworld/benchmark_7runs_raw.json`](file:///home/earl/Desktop/Slate/benchmarks/results/realworld/benchmark_7runs_raw.json).
  - Statistics script computing strict IQR overlap: [`scripts/compute_iqr_stats.py`](file:///home/earl/Desktop/Slate/scripts/compute_iqr_stats.py) $\to$ [`benchmarks/results/realworld/7run_iqr_summary.json`](file:///home/earl/Desktop/Slate/benchmarks/results/realworld/7run_iqr_summary.json).
  - Heavy 7-run scenario runner: [`scripts/bench_heavy_7runs.py`](file:///home/earl/Desktop/Slate/scripts/bench_heavy_7runs.py) $\to$ [`benchmarks/results/realworld/heavy_scenario_7run.json`](file:///home/earl/Desktop/Slate/benchmarks/results/realworld/heavy_scenario_7run.json).
  - Idle CPU breakdown with DevTools closed: [`scripts/measure_idle_breakdown.py`](file:///home/earl/Desktop/Slate/scripts/measure_idle_breakdown.py) $\to$ [`benchmarks/results/realworld/idle_cpu_breakdown.json`](file:///home/earl/Desktop/Slate/benchmarks/results/realworld/idle_cpu_breakdown.json).
  - All 10 sites verified with automated readiness gates (`scripts/verify_readiness_gates.py`).
  - 8 filter lists active, 216 Brave `adblock-resources` scriptlets compiled, `filters.dat` serialized in 1.7 ms warm start.

- **Tasks 3, 4, 5 Architectural Reports**: **Complete & Recorded**
  - Task 3 (CEF Flag Security Audit): [`docs/task-3-security-flags.md`](file:///home/earl/Desktop/Slate/docs/task-3-security-flags.md)
  - Task 4 (H.264 / AAC Codec Strategy & Software Fallbacks): [`docs/task-4-h264-aac-codecs.md`](file:///home/earl/Desktop/Slate/docs/task-4-h264-aac-codecs.md)
  - Task 5 (CEF Security Update Cadence & Maintenance Plan): [`docs/task-5-security-updates.md`](file:///home/earl/Desktop/Slate/docs/task-5-security-updates.md)

- **Next Phase**: **Phase 1 Milestone 1 (crates/app binary & vertical tab shell skeleton)**.

---

## 2. Key Verified Measurements (Traceable Raw 7-Run Data)

| Metric / Workload | Slate Result | Brave Result | Strict Verdict (IQR Rule) |
| :--- | :--- | :--- | :--- |
| **1 Tab (YouTube) PSS** | 391.5 MB (IQR $[355.2, 469.4]$) | 403.7 MB (IQR $[371.6, 407.1]$) | **No meaningful difference** (IQRs overlap) |
| **5 Tabs (Tech / News) PSS** | **411.8 MB** (IQR $[388.6, 444.9]$) | 620.4 MB (IQR $[594.2, 674.0]$) | **Probable difference** (Slate lower by 208.6 MB) |
| **10 Tabs (Heavy Mixed) PSS** | 777.5 MB (IQR $[652.6, 883.4]$) | 694.5 MB (IQR $[512.6, 868.2]$) | **No meaningful difference** (IQRs overlap) |
| **10 Tabs Sleep (Baseline Overhead) PSS** | **379.1 MB** (IQR $[363.8, 382.1]$) | 511.9 MB (IQR $[468.8, 582.4]$) | **Probable difference** (Slate lower by 132.8 MB) |
| **Heavy (4K/60fps BBB + The Verge) PSS** | 1,257.5 MB (IQR $[1,208.3, 1,311.8]$) | **979.5 MB** (IQR $[808.2, 1,053.1]$) | **Probable difference** (Brave lower by 278.0 MB) |
| **Blank Tab Idle CPU (60s)** | **5.15%** of 1 core (0.25% at opt-0) | 19.06% of 1 core | Slate lower |
| **1 Static Page Idle CPU (60s)** | 10.16% of 1 core (0% renderer) | 15.83% of 1 core (0% renderer) | Minor gap in browser main loop |
| **10 Static Pages Idle CPU (60s)** | 99.62% of 1 core | 91.49% of 1 core | ~1 core on both due to background JS timers |
| **Filter Cache Warm Start** | **1.72 ms** (release mode) | N/A (built-in) | 8 lists + 216 scriptlets from `filters.dat` |

---

## 3. Important Rules & Gotchas
1. **Never use gdb, never dump `libcef.so` symbols**: Work strictly from git, tests, and source.
2. **Smoke test and Site Isolation test in `./scripts/check.sh`**:
   - Live CEF smoke test (`slate-spike-cef --tabs 1 --exit-after-load`)
   - Live Site Isolation test (`slate-spike-cef --test-site-isolation --tabs 2 --real-sites`) asserting each site runs in a distinct renderer PID.
3. **Never overwrite `--disable-features` or disable `MediaRouter` in CEF 156**: CEF Chrome runtime dereferences `MediaRouter` on initialization. Always append switches using `append_csv_switch`.
4. **Site Isolation must remain intact**: Never pass `--no-sandbox`, `--disable-web-security`, or `--renderer-process-limit` by default in production.
5. **Cold start with `--no-blocking`**: Turning blocking ON accelerates page load by ~1.5s vs `--no-blocking` (3,017 ms vs 4,583 ms) by dropping network tracker requests.

---

## 4. Immediate Next Step
Begin **Phase 1 Milestone 1**: Create the production app binary (`crates/app`) with the shell skeleton (vertical tabs, address bar) running on CEF.
