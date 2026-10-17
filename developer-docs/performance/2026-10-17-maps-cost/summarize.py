#!/usr/bin/env python3
"""Aggregate scoped maps CPU and the separate whole-backend ablation."""
import json
import statistics
import sys
from pathlib import Path


def summarize(data):
    groups = {key: [r for r in data['records'] if r['variant'] == key]
              for key in ('normal', 'no_maps')}
    output = {}
    for key, records in groups.items():
        assert records
        elapsed = sum(r['elapsed_seconds'] for r in records)
        user = sum(r['user_seconds'] for r in records)
        system = sum(r['system_seconds'] for r in records)
        intervals = []
        health = []
        for r in records:
            before, after = r['before'], r['after']
            times = after['snapshotTimes'][before['counts']['snapshot']:]
            intervals += [(b-a)/1000 for a,b in zip(times,times[1:])]
            health.append({
                'iteration': r['iteration'],
                'snapshots': after['counts']['snapshot']-before['counts']['snapshot'],
                'activities': after['counts']['activity']-before['counts']['activity'],
                'sse_gaps': after['counts']['gap']-before['counts']['gap'],
                'processes': [before['snapshot']['processes'], after['snapshot']['processes']],
                'maps': [before['snapshot']['maps'], after['snapshot']['maps']],
                'inspected_fds': [before['snapshot']['inspectedFds'], after['snapshot']['inspectedFds']],
                'map_errors': after['snapshot']['mapErrors'], 'observer_errors': after['errors'],
                'sensors': {s:after['status'][s] for s in ('cpu','ipc','files','signals')},
                'counter_deltas': {s:after['status'][s]-before['status'][s]
                                   for s in ('lost','unresolved','files_lost','signals_lost')},
            })
        result = {
            'runs': len(records), 'elapsed_seconds': elapsed,
            'cpu_seconds': user+system, 'user_percent': user/elapsed*100,
            'system_percent': system/elapsed*100, 'cpu_percent': (user+system)/elapsed*100,
            'cpu_runs': [r['cpu_percent'] for r in records],
            'snapshot_mean_period_seconds': statistics.mean(intervals), 'health': health,
        }
        if key == 'normal':
            spans = [r['mapsTiming'] for r in records]
            read = sum(s['readCpuSeconds'] for s in spans)
            parse = sum(s['parseCpuSeconds'] for s in spans)
            result['maps_scope'] = {
                'read_cpu_seconds': read, 'parse_cpu_seconds': parse,
                'cpu_seconds': read+parse,
                'read_cpu_percent': read/elapsed*100, 'parse_cpu_percent': parse/elapsed*100,
                'cpu_percent': (read+parse)/elapsed*100,
                'share_of_backend_cpu_percent': (read+parse)/(user+system)*100,
                'read_share_of_maps_cpu_percent': read/(read+parse)*100,
                'parse_share_of_maps_cpu_percent': parse/(read+parse)*100,
                **{key:sum(s[key] for s in spans)
                   for key in ('calls','boundaryCalls','rows','bytes','failedCalls')},
                'per_run': spans,
                'boundary_policy': 'Only complete read_maps spans within the process CPU interval; partial boundary spans are excluded.',
            }
        output[key] = result
    output['ablation'] = {
        'cpu_reduction_percent': (output['normal']['cpu_percent']-output['no_maps']['cpu_percent'])/output['normal']['cpu_percent']*100,
        'cpu_percentage_points_saved': output['normal']['cpu_percent']-output['no_maps']['cpu_percent'],
        'scope': 'Whole-backend effect in this private container: maps reads, parsing, downstream map handling and observer feedback; scan cadence can change.',
    }
    return output


if __name__ == '__main__':
    result = summarize(json.loads(Path(sys.argv[1]).read_text()))
    Path(sys.argv[2]).write_text(json.dumps(result,indent=2)+'\n')
