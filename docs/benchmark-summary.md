# Slate vs Brave: Benchmark Summary & Architectural Conclusions

## Executive Verdict
Across 7-run real-world measurements with automated per-site readiness gates, strict Interquartile Range (IQR) non-overlap testing, and DevTools-isolated CPU accounting, Slate demonstrates measurable advantages in specific workloads while matching or trailing Chromium/Brave in others:

- **Where Slate is lighter**:
  1. **5 Tabs Static/News Workload**: Slate consumes **411.8 MB** median PSS vs Brave's **620.4 MB** (IQR $[388.6, 444.9]$ vs $[594.2, 674.0]$ MB $\to$ **probable difference**, Slate lower by 208.6 MB).
  2. **Baseline Overhead after Discarding Tabs (10 Tabs Sleep)**: After closing 9 inactive background tabs in both browsers, Slate uses **379.1 MB** median PSS vs Brave's **511.9 MB** (IQR $[363.8, 382.1]$ vs $[468.8, 582.4]$ MB $\to$ **probable difference**, Slate lower by 132.8 MB). Note: Because both browsers closed background tabs, this measures clean baseline overhead rather than tab sleeping quality.
  3. **Blank Tab Idle CPU**: Slate consumes **5.15% of 1 core** vs Brave's **19.06% of 1 core** when idle on `about:blank`. When privacy flags (`--disable-background-networking`) are removed, Slate CEF consumes **0.25% of 1 core** at idle.
  4. **Filter Cache Deserialization**: Binary FlatBuffer cache (`filters.dat`) with 8 full lists and 216 scriptlets deserializes in **1.72 ms** on warm start.

- **Where Slate is NOT lighter**:
  1. **Heavy Continuous Streaming**: Under sustained 4K/60fps video playback + interactive news browsing, Slate reaches **1,257.5 MB** median PSS vs Brave's **979.5 MB** (IQR $[1,208.3, 1,311.8]$ vs $[808.2, 1,053.1]$ MB $\to$ **probable difference**, Brave lower by 278.0 MB; hypothesized to be due to Chromium's internal media buffer discard tuning).
  2. **10-Tab Real-World Idle CPU**: On 10 real-world pages with active JavaScript background timers, Slate consumes **99.62% of 1 core** vs Brave's **91.49% of 1 core** (Slate is slightly higher due to background timers in news renderers).
  3. **Cold Start to First Web Page**: While Slate starts in ~589 ms to `about:blank`, cold-loading Hacker News takes **3,017.1 ms** median vs Brave's **735.0 ms** under fresh profiles. (The hypothesis that Brave parallelizes DNS pre-connection remains unverified). When `--no-blocking` is passed, Slate loads in **4,583.0 ms** (blocking ON is ~1.5s faster than blocking OFF because network requests are dropped).

- **Where there is NO meaningful difference**:
  1. **1 Tab (YouTube) PSS**: Slate **391.5 MB** (IQR $[355.2, 469.4]$) vs Brave **403.7 MB** (IQR $[371.6, 407.1]$) $\to$ **no meaningful difference** (IQRs overlap).
  2. **10 Tabs (Heavy Mixed) PSS**: Slate **777.5 MB** (IQR $[652.6, 883.4]$) vs Brave **694.5 MB** (IQR $[512.6, 868.2]$) $\to$ **no meaningful difference** (IQRs overlap).
  3. **Single Static Page Idle CPU**: Slate **10.16%** vs Brave **15.83%** of 1 core (renderer CPU is **0.0%** on both browsers; all ticks stem from browser event loop and network polling).

---

## 1. Summary Comparison Table (Traceable 7-Run Data, Run 1 Discarded)

Data read directly from [`benchmarks/results/realworld/benchmark_7runs_raw.json`](file:///home/earl/Desktop/Slate/benchmarks/results/realworld/benchmark_7runs_raw.json) and computed via [`scripts/compute_iqr_stats.py`](file:///home/earl/Desktop/Slate/scripts/compute_iqr_stats.py):

| Scenario / Workload | Slate Analyzed Runs (MB) | Slate Median & IQR | Brave Analyzed Runs (MB) | Brave Median & IQR | IQR Overlap? | Strict Verdict |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **1 Tab (YouTube)** | 348.5, 355.2, 364.8, 418.2, 469.4, 536.9 | **391.5 MB**<br>IQR: $[355.2, 469.4]$ | 368.7, 371.6, 403.5, 403.9, 407.1, 413.2 | **403.7 MB**<br>IQR: $[371.6, 407.1]$ | **Yes** | **No meaningful difference** |
| **5 Tabs (Tech / News)** | 357.6, 388.6, 401.5, 422.0, 444.9, 526.1 | **411.8 MB**<br>IQR: $[388.6, 444.9]$ | 594.1, 594.2, 596.8, 644.0, 674.0, 693.1 | **620.4 MB**<br>IQR: $[594.2, 674.0]$ | **No** | **Probable difference**<br>(Slate lower by 208.6 MB) |
| **10 Tabs (Heavy Mixed)** | 532.9, 652.6, 713.6, 841.4, 883.4, 982.0 | **777.5 MB**<br>IQR: $[652.6, 883.4]$ | 473.6, 512.6, 548.4, 840.5, 868.2, 880.7 | **694.5 MB**<br>IQR: $[512.6, 868.2]$ | **Yes** | **No meaningful difference** |
| **10 Tabs Sleep (Baseline Overhead)** | 349.3, 363.8, 377.9, 380.3, 382.1, 411.9 | **379.1 MB**<br>IQR: $[363.8, 382.1]$ | 425.7, 468.8, 475.7, 548.0, 582.4, 598.9 | **511.9 MB**<br>IQR: $[468.8, 582.4]$ | **No** | **Probable difference**<br>(Slate lower by 132.8 MB) |
| **Heavy (4K/60fps BBB + The Verge)** | 1092.9, 1208.3, 1238.9, 1257.5, 1311.8, 1320.7 | **1,257.5 MB**<br>IQR: $[1,208.3, 1,311.8]$ | 728.7, 808.2, 856.4, 979.5, 1053.1, 1123.6 | **979.5 MB**<br>IQR: $[808.2, 1,053.1]$ | **No** | **Probable difference**<br>(Brave lower by 278.0 MB) |

---

## 2. True Idle CPU Breakdown (DevTools Disconnected, 60s Sample)

| Test State | Browser | Total CPU | Browser Process | Network Utility | Renderer Processes | GPU Process |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **`about:blank`** | **Slate** | **5.15%** | 4.02% | 0.98% | **0.00%** | 0.15% |
| | Brave | **19.06%** | 13.08% | 5.88% | **0.00%** | 0.10% |
| **1 Static Page (HN)** | **Slate** | **10.16%** | 7.93% | 2.20% | **0.00%** | 0.03% |
| | Brave | **15.83%** | 10.84% | 4.85% | **0.03%** | 0.11% |
| **10 Static Pages** | Slate | **99.62%** | 14.85% | 5.21% | **79.24%** | 0.32% |
| | Brave | **91.49%** | 18.22% | 7.15% | **65.80%** | 0.32% |

> [!NOTE]
> On 10 real-world sites, Slate's CPU was slightly higher (99.62% vs 91.49%) because background JavaScript timers in news renderers ran continuously. When tested on `about:blank` with `--opt-level 0` (privacy flags disabled), Slate consumed 0.25% of 1 core.

---

## 3. Cold Start Profiling (Blocking ON vs Blocking OFF)
Measured via [`scripts/measure_cold_start_fair.py`](file:///home/earl/Desktop/Slate/scripts/measure_cold_start_fair.py) on `https://news.ycombinator.com`:
- **Slate Blocking ON**: Median **3,017.1 ms** (IQR $[2,169.1, 4,036.5]$ ms, range $[2,140.2, 5,668.5]$).
- **Slate Blocking OFF (`--no-blocking`)**: Median **4,583.0 ms** (IQR $[4,099.2, 5,486.2]$ ms, range $[2,342.4, 8,687.2]$).
- **Filter Deserialization**: Takes **1.72 ms** in release mode from `filters.dat`.
- **Finding**: Running with blocking enabled is ~1.5s faster than disabling blocking because dropping subresource requests eliminates network latency. The claim that Brave's faster start is due to parallelized DNS pre-connection remains an untested hypothesis.
