#!/usr/bin/env python3
"""
Computes IQR Statistics and Process Breakdown for the 5-Tab Diagnostic Matrix.
Reads benchmarks/results/realworld/diagnostics_5tabs_matrix_raw.json only.
Discards Run 1 as warmup, analyzes Runs 2-7.
"""

import sys, os, json, statistics

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
RAW_FILE = os.path.join(ROOT_DIR, 'benchmarks/results/realworld/diagnostics_5tabs_matrix_raw.json')
OUT_FILE = os.path.join(ROOT_DIR, 'benchmarks/results/realworld/diagnostics_5tabs_summary.json')

def compute_iqr(vals):
    if not vals: return {}
    s = sorted(vals)
    n = len(s)
    med = statistics.median(s)
    if n == 1: q1, q3 = med, med
    elif n < 4: q1, q3 = s[0], s[-1]
    else:
        q1 = statistics.median(s[:n//2])
        q3 = statistics.median(s[(n+1)//2:]) if n%2 != 0 else statistics.median(s[n//2:])
    return {
        'median': round(med, 1),
        'q1': round(q1, 1),
        'q3': round(q3, 1),
        'iqr': round(q3 - q1, 1),
        'min': round(min(s), 1),
        'max': round(max(s), 1),
        'raw': [round(x, 1) for x in s]
    }

def main():
    if not os.path.exists(RAW_FILE):
        print(f"Error: {RAW_FILE} does not exist")
        sys.exit(1)

    with open(RAW_FILE) as f:
        data = json.load(f)

    summary = {
        'timestamp': data.get('timestamp'),
        'scenarios': {},
        'comparisons': {},
        'discard_count': len(data.get('discard_log', []))
    }

    scenarios = data.get('scenarios', {})
    for name, runs in scenarios.items():
        analyzed = runs[1:] if len(runs) > 1 else runs
        pss_stats = compute_iqr([r['pss_mb'] for r in analyzed])
        rss_stats = compute_iqr([r['rss_mb'] for r in analyzed])
        proc_stats = compute_iqr([r['process_count'] for r in analyzed])

        # By process kind
        kinds = ['browser', 'renderer', 'gpu', 'network', 'utility', 'zygote']
        kind_breakdown = {}
        for k in kinds:
            vals = [r.get('by_type_mb', {}).get(k, 0) for r in analyzed]
            kind_breakdown[k] = compute_iqr(vals)

        # Video time tracking
        v_starts = [r.get('video_info', {}).get('start_time', -1) for r in analyzed if r.get('video_info')]
        v_ends = [r.get('video_info', {}).get('end_time', -1) for r in analyzed if r.get('video_info')]
        resolutions = list(set([r.get('video_info', {}).get('resolution') for r in analyzed if r.get('video_info')]))

        summary['scenarios'][name] = {
            'runs_total': len(runs),
            'runs_analyzed': len(analyzed),
            'pss': pss_stats,
            'rss': rss_stats,
            'process_count': proc_stats,
            'by_process_kind': kind_breakdown,
            'video_advancement': {
                'start_median': round(statistics.median(v_starts), 2) if v_starts else None,
                'end_median': round(statistics.median(v_ends), 2) if v_ends else None,
                'resolutions': resolutions
            }
        }

    # Comparisons
    def compare(a_name, b_name):
        if a_name in summary['scenarios'] and b_name in summary['scenarios']:
            a_pss = summary['scenarios'][a_name]['pss']
            b_pss = summary['scenarios'][b_name]['pss']
            overlap = not (a_pss['q3'] < b_pss['q1'] or b_pss['q3'] < a_pss['q1'])
            diff = round(a_pss['median'] - b_pss['median'], 1)
            summary['comparisons'][f"{a_name}_vs_{b_name}"] = {
                'median_diff_mb': diff,
                'iqr_overlap': overlap,
                'verdict': 'no meaningful difference' if overlap else 'probable difference'
            }

    compare('slate_opt3_blocking_on', 'slate_opt3_blocking_off')
    compare('slate_opt0_blocking_on', 'slate_opt0_blocking_off')
    compare('slate_opt3_blocking_on', 'slate_opt0_blocking_on')
    compare('slate_opt3_blocking_on', 'brave_seeded')

    with open(OUT_FILE, 'w') as f:
        json.dump(summary, f, indent=2)

    print("==========================================================================================")
    print(" 5-TAB DIAGNOSTIC MATRIX SUMMARY")
    print("==========================================================================================")
    print(f"{'Scenario':<26} | {'Total PSS Median (IQR)':<24} | {'Renderer PSS':<14} | {'Browser PSS':<13} | {'Procs'}")
    print("-" * 90)
    for name, s in summary['scenarios'].items():
        pss = s['pss']
        rend = s['by_process_kind']['renderer']['median']
        brows = s['by_process_kind']['browser']['median']
        pcount = int(s['process_count']['median'])
        print(f"{name:<26} | {pss['median']:>6.1f} MB [{pss['q1']:>5.1f}, {pss['q3']:>5.1f}] | {rend:>6.1f} MB     | {brows:>6.1f} MB   | {pcount:>2}")

    print("\nPairwise Comparisons:")
    for comp, res in summary['comparisons'].items():
        print(f"  {comp}: diff = {res['median_diff_mb']:>+6.1f} MB, overlap = {res['iqr_overlap']} -> {res['verdict']}")

    print(f"\nDiscards logged: {summary['discard_count']}")
    print(f"Summary saved to: {OUT_FILE}")

if __name__ == '__main__':
    main()
