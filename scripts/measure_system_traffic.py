#!/usr/bin/env python3
"""Measure actual /api/system/events JSON payloads without changing transport."""
import argparse
import datetime
import gzip
import json
import math
import platform
import socket
import time
import urllib.request

DECODER = json.JSONDecoder()


def fields(raw):
    """Return exact JSON value slices (including original escaping)."""
    result = {}
    pos = 1
    while True:
        while raw[pos].isspace() or raw[pos] == ',':
            pos += 1
        if raw[pos] == '}':
            return result
        key, pos = DECODER.raw_decode(raw, pos)
        while raw[pos].isspace() or raw[pos] == ':':
            pos += 1
        start = pos
        _, pos = DECODER.raw_decode(raw, pos)
        result[key] = raw[start:pos]


def size(raw):
    return len(raw.encode('utf-8'))


def snapshot(raw):
    parts = fields(raw)
    processes = parts['processes']
    pos = 1
    maps_bytes = map_count = process_count = 0
    while True:
        while processes[pos].isspace() or processes[pos] == ',':
            pos += 1
        if processes[pos] == ']':
            break
        start = pos
        process, pos = DECODER.raw_decode(processes, pos)
        maps_bytes += size(fields(processes[start:pos])['maps'])
        map_count += len(process['maps'])
        process_count += 1
    raw_bytes = size(raw)
    process_bytes = size(processes)
    fd_bytes = size(parts['fd_relations'])
    compressed = len(gzip.compress(raw.encode('utf-8'), compresslevel=6, mtime=0))
    return dict(raw_bytes=raw_bytes, processes_bytes=process_bytes,
                maps_value_bytes=maps_bytes,
                process_metadata_and_structure_bytes=process_bytes - maps_bytes,
                fd_relations_bytes=fd_bytes,
                other_bytes=raw_bytes - process_bytes - fd_bytes,
                process_count=process_count, map_entry_count=map_count,
                fd_relation_count=len(json.loads(parts['fd_relations'])),
                gzip_bytes=compressed, compression_ratio=raw_bytes / compressed)


def percentile(values, percent):
    return sorted(values)[max(0, math.ceil(len(values) * percent / 100) - 1)] if values else None


def measure(url, seconds):
    sizes = []
    largest = None
    snapshots = []
    gaps = 0
    statuses = []
    request = urllib.request.Request(url.rstrip('/') + '/api/system/events',
                                     headers={'Accept': 'text/event-stream'})
    with urllib.request.urlopen(request, timeout=seconds + 10) as response:
        started = time.monotonic()
        event = ''
        data = []
        # Limit every read to the remaining observation window, including idle streams.
        while (remaining := seconds - (time.monotonic() - started)) > 0:
            response.fp.raw._sock.settimeout(remaining)
            try:
                line = response.readline()
            except (TimeoutError, socket.timeout):
                break
            if not line:
                raise RuntimeError('SSE stream ended before measurement completed')
            line = line.decode('utf-8').rstrip('\r\n')
            if line.startswith('event:'):
                event = line[6:].lstrip(' ')
            elif line.startswith('data:'):
                data.append(line[5:].removeprefix(' '))
            elif not line:
                if data:
                    raw = '\n'.join(data)
                    if event == 'snapshot':
                        report = snapshot(raw)
                        # Retain the largest snapshot: initial subscription can be empty.
                        if not snapshots or report['raw_bytes'] > snapshots[0]['raw_bytes']:
                            snapshots[:] = [report]
                    elif event == 'activity':
                        count = size(raw)
                        sizes.append(count)
                        parts = fields(raw)
                        status = json.loads(parts['status'])
                        if status not in statuses:
                            statuses.append(status)
                        if largest is None or count > largest['raw_bytes']:
                            largest = dict(raw_bytes=count, **{
                                name + '_bytes': size(parts[name]) for name in ('files', 'ipc', 'cpu')})
                            largest['other_bytes'] = count - sum(largest[name + '_bytes'] for name in ('files', 'ipc', 'cpu'))
                    elif event == 'gap':
                        gaps += json.loads(raw)['dropped_frames']
                event, data = '', []
        elapsed = time.monotonic() - started
    return dict(measured_at_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                environment=platform.platform(), url=url, duration_seconds=elapsed,
                snapshot_largest=snapshots[0] if snapshots else None,
                activity=dict(event_count=len(sizes), total_bytes=sum(sizes),
                              average_bytes_per_second=sum(sizes) / elapsed,
                              max_event_bytes=max(sizes, default=0),
                              p50_event_bytes=percentile(sizes, 50),
                              p95_event_bytes=percentile(sizes, 95),
                              p99_event_bytes=percentile(sizes, 99), largest_event=largest,
                              sensor_statuses=statuses), dropped_frames=gaps)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('url', nargs='?', default='http://127.0.0.1:9090')
    parser.add_argument('--seconds', type=float, default=60)
    args = parser.parse_args()
    if not math.isfinite(args.seconds) or args.seconds <= 0:
        parser.error('--seconds must be finite and positive')
    print(json.dumps(measure(args.url, args.seconds), ensure_ascii=False, indent=2))
