#!/usr/bin/env python3
"""
Compute Summary Statistics (Median, IQR, Bounds) for Idle CPU 7-Run Benchmarks.
Reads: benchmarks/results/realworld/idle_cpu_7runs_raw.json
Writes: benchmarks/results/realworld/idle_cpu_7runs_summary.json
"""

import os, sys, json

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
RAW_FILE = os.path.join(ROOT_DIR, 'benchmarks/results/realworld/idle_cpu_7runs_raw.json')
OUT_FILE = os.path.join(ROOT_DIR, 'benchmarks/results/realworld/idle_cpu_7runs_summary.json')

def percentile(sorted_arr, p):
    if not sorted_arr: return 0.0
    k = (len(sorted_arr) - 1) * p
    f = int(k)
    c = min(f + 1, len(sorted_arr) - 1)
    d = k - f
    return sorted_arr[f] + d * (sorted_arr[c] - sorted_arr[f])

def median(arr):
    if not arr: return 0.0
    s = sorted(arr)
    n = len(s)
    mid = n // 2
    if n % 2 == 1:
        return s[mid]
    else:
        return (s[mid - 1] + s[mid]) / 2.0

def compute_stats(arr):
    arr_sorted = sorted(arr)
    n = len(arr_sorted)
    q1 = percentile(arr_sorted, 0.25)
    med = median(arr_sorted)
    q3 = percentile(arr_sorted, 0.75)
    iqr = round(q3 - q1, 2)
    return {
        'count': n,
        'values': arr_sorted,
        'min': min(arr_sorted),
        'max': max(arr_sorted),
        'median': round(med, 2),
        'q1': round(q1, 2),
        'q3': round(q3, 2),
        'iqr': iqr
    }

def main():
    if not os.path.exists(RAW_FILE):
        print(f"Error: {RAW_FILE} does not exist")
        sys.exit(1)

    with open(RAW_FILE, 'r') as f:
        raw_data = json.load(f)

    summary = {}
    for scenario, runs in raw_data.items():
        total_cpus = [r['total_cpu_pct'] for r in runs]
        stats = compute_stats(total_cpus)

        # Compute per-kind median
        all_kinds = set()
        for r in runs:
            all_kinds.update(r.get('by_process_kind', {}).keys())

        kind_medians = {}
        for k in sorted(all_kinds):
            k_vals = [r.get('by_process_kind', {}).get(k, 0.0) for r in runs]
            kind_medians[k] = round(float(median(k_vals)), 2)

        stats['by_process_kind_median'] = kind_medians
        summary[scenario] = stats

    with open(OUT_FILE, 'w') as f:
        json.dump(summary, f, indent=2)

    print(f"Summary statistics computed and saved to {OUT_FILE}:\n")
    print(f"{'Scenario':<28} | {'Median %':<9} | {'IQR Range':<15} | {'Min-Max %':<12}")
    print("-" * 72)
    for s, st in summary.items():
        print(f"{s:<28} | {st['median']:<9.2f} | [{st['q1']:.2f}, {st['q3']:.2f}] (IQR {st['iqr']:.2f}) | {st['min']:.2f} - {st['max']:.2f}")

if __name__ == '__main__':
    main()
