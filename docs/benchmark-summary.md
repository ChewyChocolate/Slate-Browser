# Slate vs Brave: Benchmark Summary & Architectural Conclusions

## Executive Verdict
Across 7-run rigorous real-world measurements with automated per-site readiness gates, strict Interquartile Range (IQR) non-overlap testing, and DevTools-isolated CPU accounting, Slate demonstrates measurable advantages in specific workloads while matching or trailing Chromium/Brave in others:

- **Where Slate is lighter**:
  1. **Single Tab Memory (PSS)**: Slate consumes **368.1 MB** median PSS vs Brave's **409.7 MB** (IQR $[356.6, 385.5]$ vs $[409.0, 410.3]$ MB $\to$ **probable difference**, Slate lower by 41.6 MB).
  2. **Tab Suspension / Sleeping Tabs**: After discarding inactive tabs, Slate uses **423.4 MB** median PSS vs Brave's **552.0 MB** (IQR $[408.9, 437.9]$ vs $[471.2, 620.0]$ MB $\to$ **probable difference**, Slate lower by 128.6 MB).
  3. **Blank Tab Idle CPU**: Slate uses **5.15% of 1 core** vs Brave's **19.06% of 1 core** when idle on `about:blank`.
  4. **Filter Cache Deserialization**: Binary FlatBuffer cache (`filters.dat`) with 8 full lists and 216 scriptlets deserializes in **1.72 ms** on warm start.

- **Where Slate is NOT lighter**:
  1. **Heavy Continuous Streaming**: Under sustained 4K/60fps video playback + interactive news browsing, Slate reaches **1,257.5 MB** median PSS vs Brave's **979.5 MB** (IQR $[1,208.3, 1,311.8]$ vs $[808.2, 1,053.1]$ MB $\to$ **probable difference**, Brave lower by 278.0 MB due to Chromium's internal media buffer discard tuning).
  2. **10-Tab Real-World Idle CPU**: On 10 real-world pages with active JavaScript background timers, both browsers consume ~1 full core (Slate **99.6%** vs Brave **91.5%**).
  3. **Cold Start to First Web Page**: While Slate starts in ~589 ms to `about:blank`, cold-loading a live web page (Hacker News) takes **1,683.5 ms** median vs Brave's **735.0 ms** (~948 ms gap because Chromium pre-connects and parallelizes DNS/network resolution with process initialization, whereas Slate's spike initializes sequentially and tests run against ephemeral clean profiles).

- **Where there is NO meaningful difference**:
  1. **5 Tabs Static/News Workload**: Slate **758.2 MB** vs Brave **685.2 MB** PSS (IQRs overlap: $[620.5, 857.1]$ vs $[643.9, 756.4]$ MB $\to$ **no meaningful difference**).
  2. **10 Tabs Static/News Workload**: Slate **840.4 MB** vs Brave **835.1 MB** PSS (IQRs overlap: $[779.7, 909.8]$ vs $[753.9, 909.7]$ MB $\to$ **no meaningful difference**).
  3. **YouTube 1080p Single Tab**: Slate **371.9 MB** vs Brave **372.1 MB** PSS (IQRs overlap: $[363.3, 381.1]$ vs $[367.7, 376.8]$ MB $\to$ **no meaningful difference**).
  4. **Single Static Page Idle CPU**: Slate **10.16%** vs Brave **15.83%** of 1 core (renderer CPU is **0.0%** on both browsers; all ticks stem from browser event loop and network polling).

---

## 1. Summary Comparison Table (7 Runs per Scenario, Run 1 Discarded)

| Scenario / Workload | Slate Median PSS | Slate IQR $[Q_1, Q_3]$ | Brave Median PSS | Brave IQR $[Q_1, Q_3]$ | IQR Overlap? | Strict Verdict |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **1 Tab (Hacker News)** | **368.1 MB** | $[356.6, 385.5]$ | 409.7 MB | $[409.0, 410.3]$ | **No** | **Probable difference** (Slate -41.6 MB) |
| **5 Tabs (Tech / News)** | 758.2 MB | $[620.5, 857.1]$ | 685.2 MB | $[643.9, 756.4]$ | **Yes** | **No meaningful difference** |
| **10 Tabs (Heavy Mixed)** | 840.4 MB | $[779.7, 909.8]$ | 835.1 MB | $[753.9, 909.7]$ | **Yes** | **No meaningful difference** |
| **10 Tabs (Sleeping / Suspended)** | **423.4 MB** | $[408.9, 437.9]$ | 552.0 MB | $[471.2, 620.0]$ | **No** | **Probable difference** (Slate -128.6 MB) |
| **1 Tab YouTube 1080p** | 371.9 MB | $[363.3, 381.1]$ | 372.1 MB | $[367.7, 376.8]$ | **Yes** | **No meaningful difference** |
| **Heavy (4K/60fps BBB + The Verge)** | 1,257.5 MB | $[1,208.3, 1,311.8]$ | **979.5 MB** | $[808.2, 1,053.1]$ | **No** | **Probable difference** (Brave -278.0 MB) |

---

## 2. True Idle CPU Breakdown (DevTools Closed, 60s Sample)

| Test State | Browser | Total CPU | Browser Process | Network Utility | Renderer Processes | GPU Process |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **`about:blank`** | **Slate** | **5.15%** | 4.02% | 0.98% | 0.00% | 0.15% |
| | Brave | 19.06% | 13.08% | 5.88% | 0.00% | 0.10% |
| **1 Static Page (HN)** | **Slate** | **10.16%** | 7.93% | 2.20% | 0.00% | 0.03% |
| | Brave | 15.83% | 10.84% | 4.85% | 0.03% | 0.11% |
| **10 Static Pages** | Slate | 99.62% | 14.85% | 5.21% | 79.24% | 0.32% |
| | Brave | 91.49% | 18.22% | 7.15% | 65.80% | 0.32% |

> [!NOTE]
> On both browsers, static page renderers consume **0.0% CPU** once loaded. When 10 real-world sites are open simultaneously, JavaScript timers (`setInterval`, animation frames, dynamic analytics beacons) account for ~70–80% of total CPU across renderers.

---

## 3. Cold Start Profiling Breakdown
Detailed profiling of the ~948 ms gap between Slate (1,683.5 ms) and Brave (735.0 ms) to load Hacker News:
1. **Filter Engine Deserialization**: Verified at **1.72 ms** in release mode from `filters.dat`. Does **not** cause the cold start delay.
2. **Ephemeral vs Persistent Profile**: The benchmark harness ran Slate with a freshly generated temporary directory per run (`env::temp_dir().join("slate-cef-profile-...")`), meaning DNS cache, TLS session tickets, and HTTP disk cache were cold on every invocation.
3. **Blocking Overhead**: Running Slate with blocking enabled (Opt 3) took **1,683.5 ms** vs blocking disabled (Opt 0) at **1,867.0 ms**. Blocking does **not** degrade load time (it accelerates it by dropping 30–50% of subresource requests).
4. **Window Creation vs Sequential Network**: Slate takes **~605 ms** from process launch to `OnAfterCreated` (browser window created and CEF initialized). The remaining **~1,078 ms** is spent in sequential `load_url` invocation, DNS resolution, TLS handshake, and network transfer. Brave parallelizes network pre-connection with UI process startup.

---

## 4. Scriptlet and Filter List Status
- **Lists Active**: EasyList, EasyPrivacy, uBlock Filters, uBlock Badware, uBlock Privacy, uBlock Resource Abuse, uBlock Quick Fixes, uBlock Unbreak.
- **Scriptlet Engine**: Migrated from 3 hand-written prototypes to Brave's maintained `adblock-resources` (`resources.json`, **216 scriptlets**, MPL-2.0).
- **Binary Cache**: FlatBuffer snapshot `~/.local/share/slate/filters.dat` serialized cleanly, warm start in 1.7 ms, with test coverage guaranteeing corrupt/empty downloads never overwrite valid cache.
