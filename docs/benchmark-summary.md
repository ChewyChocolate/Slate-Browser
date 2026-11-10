# Slate vs Brave: Benchmark Summary & Architectural Conclusions

## Executive Verdict
Across verified 7-run real-world measurements with automated per-site readiness gates, strict Interquartile Range (IQR) non-overlap testing, and DevTools-isolated CPU accounting:

- **1 Tab (YouTube `jNQXAC9IVRw`)**: **PSS Tie (No meaningful difference)**.
  - Slate: **598.1 MB** (IQR $[577.7, 740.6]$ MB)
  - Brave: **681.3 MB** (IQR $[504.1, 690.2]$ MB)
  - The IQRs overlap.

- **5 Tabs (Tech & Media)**: **Brave is lower by ~18% PSS**.
  - Slate: **1,310.6 MB** (IQR $[1,284.4, 1,336.5]$ MB)
  - Brave: **1,078.5 MB** (IQR $[1,045.9, 1,091.1]$ MB)
  - IQRs do not overlap $\to$ **Probable difference** (Brave lower by 232.1 MB PSS).

- **10 Tabs (Heavy Mixed)**: **Brave is lower by ~18% PSS**.
  - Slate: **1,497.2 MB** (IQR $[1,464.0, 1,691.8]$ MB)
  - Brave: **1,235.3 MB** (IQR $[1,104.9, 1,397.5]$ MB)
  - IQRs do not overlap $\to$ **Probable difference** (Brave lower by 261.9 MB PSS).

- **Heavy Scenario (YouTube 4K/60fps + The Verge)**: **Brave is lower by ~15–21% PSS**.
  - Slate: **1,136.0 MB** (IQR $[1,119.1, 1,204.0]$ MB)
  - Brave: **963.7 MB** (IQR $[942.9, 1,006.8]$ MB)
  - IQRs do not overlap $\to$ **Probable difference** (Brave lower by 172.3 MB PSS).
  - *Hypothesis*: It is hypothesized that Chromium's internal media pipeline manages demuxer/video buffer discard thresholds differently than stock CEF, but this remains an unverified hypothesis.

- **Cold Start to First Web Page (Hacker News)**: **Brave is faster**.
  - Slate (Blocking ON): Median **1,850.6 ms** (IQR $[1,584.6, 2,042.9]$ ms)
  - Slate (Blocking OFF): Median **1,706.0 ms** (IQR $[1,649.8, 1,924.3]$ ms)
  - Brave (Fresh Profile): Median **793.4 ms** (IQR $[781.7, 807.8]$ ms)
  - *Hypothesis*: The startup latency gap between Slate and Brave is hypothesized to stem from CEF runtime initialization and FFI IPC setup overhead relative to a native monolithic Chromium binary, but this has not been individually instrumented or proven.

- **Install Footprint**: **Slate has a smaller binary install footprint**.
  - Slate application binary is ~6.8 MB (release build) + FlatBuffer filter cache (~21.8 MB), compared to Brave's complete `/opt/brave.com/brave` distribution of ~350 MB.

- **Idle CPU**: **NOT ESTABLISHED**.
  - Idle CPU measurements vary substantially depending on background timer throttling, network discovery loops, and optimization flags. On `about:blank`, Slate consumes between 7.28% (Opt 3) and 14.36% (Opt 0), while Brave consumes between 11.77% (Rewards/News/Wallet disabled) and 21.04% (Slate flags). Across 10 static pages, background JavaScript timers drive CPU usage to 71–114% of 1 core on both engines. A conclusive, reproducible idle CPU verdict between the two browsers has not been established.

---

## 1. Traceable Multi-Tab Data (7 Runs, Run 1 Discarded)

Data read directly from [`benchmarks/results/realworld/benchmark_7runs_gated.json`](file:///home/earl/Desktop/Slate/benchmarks/results/realworld/benchmark_7runs_gated.json):

| Workload | Slate Analyzed Runs (MB) | Slate Median & IQR | Brave Analyzed Runs (MB) | Brave Median & IQR | IQR Overlap? | Strict Verdict |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **1 Tab (YouTube)** | 471.4, 577.7, 580.1, 616.1, 740.6, 811.8 | **598.1 MB**<br>IQR: $[577.7, 740.6]$ | 421.3, 504.1, 677.4, 685.3, 690.2, 750.1 | **681.3 MB**<br>IQR: $[504.1, 690.2]$ | **Yes** | **No meaningful difference** |
| **5 Tabs (Tech & Media)** | 1205.1, 1284.4, 1306.1, 1315.1, 1336.5, 1392.1 | **1,310.6 MB**<br>IQR: $[1,284.4, 1,336.5]$ | 1034.6, 1045.9, 1071.6, 1085.4, 1091.1, 1117.7 | **1,078.5 MB**<br>IQR: $[1,045.9, 1,091.1]$ | **No** | **Probable difference**<br>(Brave lower by 232.1 MB PSS) |
| **10 Tabs (Heavy Mixed)** | 1318.3, 1464.0, 1464.5, 1530.0, 1691.8, 1813.0 | **1,497.2 MB**<br>IQR: $[1,464.0, 1,691.8]$ | 1074.4, 1104.9, 1152.0, 1318.7, 1397.5, 1397.6 | **1,235.3 MB**<br>IQR: $[1,104.9, 1,397.5]$ | **No** | **Probable difference**<br>(Brave lower by 261.9 MB PSS) |

---

## 2. Heavy Scenario Data (YouTube 4K/60fps + The Verge)

Data read directly from [`benchmarks/results/realworld/heavy_scenario_7run_verified.json`](file:///home/earl/Desktop/Slate/benchmarks/results/realworld/heavy_scenario_7run_verified.json):

| Workload | Slate Analyzed Runs (MB) | Slate Median & IQR | Brave Analyzed Runs (MB) | Brave Median & IQR | IQR Overlap? | Strict Verdict |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Heavy (4K/60fps BBB + The Verge)** | 1052.8, 1119.1, 1131.0, 1140.9, 1204.0, 1204.3 | **1,136.0 MB**<br>IQR: $[1,119.1, 1,204.0]$ | 917.5, 942.9, 947.6, 979.7, 1006.8, 1037.1 | **963.7 MB**<br>IQR: $[942.9, 1,006.8]$ | **No** | **Probable difference**<br>(Brave lower by 172.3 MB PSS) |

- YouTube `currentTime` confirmed video playback advancement in every analyzed run for both browsers.
