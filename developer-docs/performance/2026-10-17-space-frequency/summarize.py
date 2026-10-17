#!/usr/bin/env python3
"""Summarize whole-process CPU, SSE cadence and collector health by case."""
import json
import statistics
import sys
from pathlib import Path


def interval_seconds(record, event):
    if not record['settings']['active']:
        return []
    count = record['before']['counts'][event]
    times = record['after'][event + 'Times'][count:]
    return [(right - left) / 1000 for left, right in zip(times, times[1:])]


def summarize(data):
    groups = {}
    for record in data['records']:
        groups.setdefault(record['variant'], []).append(record)
    summary = {}
    for variant, records in groups.items():
        elapsed = sum(r['elapsed_seconds'] for r in records)
        user = sum(r['user_seconds'] for r in records) / elapsed * 100
        system = sum(r['system_seconds'] for r in records) / elapsed * 100
        snapshots = [i for r in records for i in interval_seconds(r, 'snapshot')]
        activities = [i for r in records for i in interval_seconds(r, 'activity')]
        health = []
        for r in records:
            if not r['settings']['active']:
                continue
            before, after = r['before'], r['after']
            health.append({
                'iteration': r['iteration'],
                'snapshot_count': after['counts']['snapshot'] - before['counts']['snapshot'],
                'activity_count': after['counts']['activity'] - before['counts']['activity'],
                'gap_count': after['counts']['gap'] - before['counts']['gap'],
                'map_count_before': before['snapshot']['maps'],
                'map_count_after': after['snapshot']['maps'],
                'process_count_before': before['snapshot']['processes'],
                'process_count_after': after['snapshot']['processes'],
                'map_errors': after['snapshot']['mapErrors'],
                'observer_errors': after['errors'],
                'sensors': {s: after['status'][s] for s in ('cpu', 'ipc', 'files', 'signals')},
                'counter_deltas': {
                    s: after['status'].get(s, 0) - before['status'].get(s, 0)
                    for s in ('lost', 'unresolved', 'files_lost', 'signals_lost')
                },
            })
        summary[variant] = {
            'settings': records[0]['settings'], 'runs': len(records),
            'cpu_percent': user + system, 'user_percent': user, 'system_percent': system,
            'cpu_runs': [r['cpu_percent'] for r in records],
            'snapshot_interval_seconds': statistics.mean(snapshots) if snapshots else None,
            'snapshot_interval_median_seconds': statistics.median(snapshots) if snapshots else None,
            'snapshot_interval_range_seconds': [min(snapshots), max(snapshots)] if snapshots else None,
            'snapshot_rate_hz': 1 / statistics.mean(snapshots) if snapshots else 0,
            'activity_interval_seconds': statistics.mean(activities) if activities else None,
            'health': health,
        }
    if 's1' in summary:
        baseline = summary['s1']['cpu_percent']
        for result in summary.values():
            result['cpu_reduction_percent_vs_s1'] = (baseline - result['cpu_percent']) / baseline * 100
    # A descriptive fit through three group means; not a significance test or a
    # controlled estimate of CPU per snapshot on identical process/VMA inputs.
    fit = None
    if all(s in summary for s in ('s1', 's2', 's5')):
        xs = [summary[s]['snapshot_rate_hz'] for s in ('s1', 's2', 's5')]
        ys = [summary[s]['cpu_percent'] for s in ('s1', 's2', 's5')]
        x_mean, y_mean = statistics.mean(xs), statistics.mean(ys)
        slope = sum((x-x_mean)*(y-y_mean) for x, y in zip(xs, ys)) / sum((x-x_mean)**2 for x in xs)
        intercept = y_mean - slope*x_mean
        sse = sum((y-(intercept+slope*x))**2 for x, y in zip(xs, ys))
        sst = sum((y-y_mean)**2 for y in ys)
        fit = {'cpu_percent_intercept': intercept, 'cpu_percent_per_snapshot_hz': slope,
               'r_squared_of_three_means': 1 - sse/sst if sst else None,
               'interpretation': 'Descriptive only; three means, two live-workload runs each.'}
    return {'cases': summary, 'snapshot_frequency_fit': fit}


if __name__ == '__main__':
    data = json.loads(Path(sys.argv[1]).read_text())
    result = summarize(data)
    text = json.dumps(result, indent=2) + '\n'
    if len(sys.argv) > 2:
        Path(sys.argv[2]).write_text(text)
    else:
        print(text, end='')
