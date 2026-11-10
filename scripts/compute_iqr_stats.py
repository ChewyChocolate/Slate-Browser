#!/usr/bin/env python3
"""
Compute IQR Statistics & Strict Non-Overlap Verdicts
- Reads raw run data exclusively from `benchmarks/results/realworld/benchmark_7runs_raw.json`.
- Discards Run 1 as warmup, analyzes Runs 2-7.
- Evaluates strict IQR rule:
  Probable difference if [Q1_slate, Q3_slate] and [Q1_brave, Q3_brave] do not overlap.
  No meaningful difference if they do overlap.
- Outputs `benchmarks/results/realworld/7run_iqr_summary.json`.
"""

import sys, os, json, statistics

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
RESULTS_DIR = os.path.join(ROOT_DIR, 'benchmarks/results/realworld')
RAW_IN_FILE = os.path.join(RESULTS_DIR, 'benchmark_7runs_raw.json')
SUMMARY_OUT_FILE = os.path.join(RESULTS_DIR, '7run_iqr_summary.json')

def compute_iqr_stats(values):
    # values: list of 6 numbers
    s = sorted(values)
    n = len(s)
    med = round(statistics.median(s), 1)
    q1 = round(statistics.median(s[:n//2]), 1)
    q3 = round(statistics.median(s[(n+1)//2:]) if n%2 != 0 else statistics.median(s[n//2:]), 1)
    iqr = round(q3 - q1, 1)
    return {
        'median': med,
        'q1': q1,
        'q3': q3,
        'iqr': iqr,
        'min': round(s[0], 1),
        'max': round(s[-1], 1),
        'analyzed_runs': s
    }

def main():
    if not os.path.exists(RAW_IN_FILE):
        print(f"Error: {RAW_IN_FILE} does not exist. Run scripts/run_7pass_benchmarks.py first.")
        sys.exit(1)

    with open(RAW_IN_FILE) as f:
        raw_data = json.load(f)

    summary = {
        "timestamp": raw_data.get("timestamp"),
        "source_raw_file": RAW_IN_FILE,
        "scenarios": {}
    }

    print("==============================================================================================================")
    print(" 7-RUN BENCHMARK SUMMARY (STRICT IQR RULE: RUN 1 DISCARDED, RUNS 2-7 ANALYZED)")
    print("==============================================================================================================")
    print(f"{'Scenario':<16} | {'Slate Raw Runs (MB)':<34} | {'Slate Median/IQR':<20} | {'Brave Raw Runs (MB)':<34} | {'Brave Median/IQR':<20} | {'Verdict':<22}")
    print("-" * 155)

    for sc_name, sc_data in raw_data.get("scenarios", {}).items():
        s_runs_all = sc_data.get("slate_runs", [])
        b_runs_all = sc_data.get("brave_runs", [])

        if len(s_runs_all) < 7 or len(b_runs_all) < 7:
            print(f"Warning: scenario {sc_name} has fewer than 7 runs (Slate: {len(s_runs_all)}, Brave: {len(b_runs_all)})")
            continue

        # Discard run 1 (warmup)
        s_analyzed = s_runs_all[1:]
        b_analyzed = b_runs_all[1:]

        s_pss_vals = [r["pss_mb"] for r in s_analyzed]
        b_pss_vals = [r["pss_mb"] for r in b_analyzed]

        s_pss = compute_iqr_stats(s_pss_vals)
        b_pss = compute_iqr_stats(b_pss_vals)

        s_rss_vals = [r["rss_mb"] for r in s_analyzed]
        b_rss_vals = [r["rss_mb"] for r in b_analyzed]
        s_rss = compute_iqr_stats(s_rss_vals)
        b_rss = compute_iqr_stats(b_rss_vals)

        s_procs = int(statistics.median([r["process_count"] for r in s_analyzed]))
        b_procs = int(statistics.median([r["process_count"] for r in b_analyzed]))

        # Strict IQR overlap rule
        iqr_overlap = not (s_pss['q3'] < b_pss['q1'] or b_pss['q3'] < s_pss['q1'])
        if iqr_overlap:
            verdict = "No meaningful diff"
        else:
            diff = round(s_pss['median'] - b_pss['median'], 1)
            winner = "Slate lower" if diff < 0 else "Brave lower"
            verdict = f"Probable diff ({winner} {abs(diff)} MB)"

        summary["scenarios"][sc_name] = {
            "slate": {
                "pss": s_pss,
                "rss": s_rss,
                "process_count_median": s_procs,
                "raw_runs": s_runs_all
            },
            "brave": {
                "pss": b_pss,
                "rss": b_rss,
                "process_count_median": b_procs,
                "raw_runs": b_runs_all
            },
            "iqr_overlap": iqr_overlap,
            "verdict": verdict
        }

        s_raw_str = ", ".join(str(x) for x in s_pss['analyzed_runs'])
        b_raw_str = ", ".join(str(x) for x in b_pss['analyzed_runs'])
        s_stat_str = f"{s_pss['median']} [{s_pss['q1']}, {s_pss['q3']}]"
        b_stat_str = f"{b_pss['median']} [{b_pss['q1']}, {b_pss['q3']}]"

        print(f"{sc_name:<16} | {s_raw_str:<34} | {s_stat_str:<20} | {b_raw_str:<34} | {b_stat_str:<20} | {verdict:<22}")

    with open(SUMMARY_OUT_FILE, 'w') as f:
        json.dump(summary, f, indent=2)

    print("-" * 155)
    print(f"Summary saved to: {SUMMARY_OUT_FILE}")

if __name__ == '__main__':
    main()
