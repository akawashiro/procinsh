#!/usr/bin/env python3
"""Measure SSE transport bytes and arrival latency on a local procinsh server.

HTTP/TCP headers and chunk framing are excluded. Event transfer sizes attribute
compressed bytes to the event whose final SSE delimiter they decode; flush bytes
following a delimiter belong to the next event. Stream totals are exact.
"""
import argparse
import collections
import datetime
import http.client
import json
import math
import os
import queue
import threading
import socket
import time
import urllib.parse
import zlib


def percentile(values, p):
    return sorted(values)[max(0, math.ceil(len(values) * p / 100) - 1)] if values else None


def cpu_ticks(pid):
    with open(f'/proc/{pid}/stat') as stat:
        fields = stat.read().rsplit(')', 1)[1].split()
    return int(fields[11]) + int(fields[12])


def measure(url, seconds, encoding, pid=None):
    address = urllib.parse.urlsplit(url)
    if address.scheme != 'http':
        raise ValueError('use a local http URL')
    connection = http.client.HTTPConnection(address.hostname, address.port, timeout=seconds + 10)
    connection.request('GET', '/api/system/events', headers={'Accept-Encoding': encoding})
    response = connection.getresponse()
    if response.status != 200:
        raise RuntimeError(f'HTTP {response.status}')
    compressed = response.getheader('Content-Encoding') == 'gzip'
    if compressed != (encoding == 'gzip'):
        raise RuntimeError('unexpected Content-Encoding')
    decoder = zlib.decompressobj(16 + zlib.MAX_WBITS) if compressed else None
    groups = collections.defaultdict(list)
    pending = bytearray()
    transferred = raw_total = event_wire = 0
    statuses = []
    dropped = 0
    final_status = None
    wall_started = time.time()
    started = time.monotonic()
    initial_cpu = cpu_ticks(pid) if pid else None

    def consume(decoded, wire, received):
        nonlocal event_wire, dropped, final_status
        search_from = max(0, len(pending) - 1)
        pending.extend(decoded)
        event_wire += wire
        while (end := pending.find(b'\n\n', search_from)) >= 0:
            event = bytes(pending[:end])
            del pending[:end + 2]
            search_from = 0
            name = next((line[7:] for line in event.splitlines() if line.startswith(b'event: ')), b'keepalive').decode()
            payload = b'\n'.join(line[6:] for line in event.splitlines() if line.startswith(b'data: '))
            entry = {'raw_payload_bytes': len(payload), 'transferred_bytes': event_wire,
                     'received_seconds': received - started}
            event_wire = 0
            if payload:
                value = json.loads(payload)
                if value.get('captured_at'):
                    entry['arrival_age_ms'] = (wall_started + received - started) * 1000 - value['captured_at']
                if name == 'activity':
                    final_status = value['status']
                    status = {key: value['status'][key] for key in ('ipc', 'cpu', 'files')}
                    if status not in statuses:
                        statuses.append(status)
                if name == 'gap':
                    dropped += value['dropped_frames']
            groups[name].append(entry)

    chunks = queue.SimpleQueue()

    def receive():
        try:
            while (remaining := seconds - (time.monotonic() - started)) > 0:
                response.fp.raw._sock.settimeout(remaining)
                try:
                    chunk = response.read1(65536)
                except (TimeoutError, socket.timeout):
                    break
                if not chunk:
                    raise RuntimeError('SSE stream ended')
                chunks.put((chunk, time.monotonic()))
            elapsed = time.monotonic() - started
            cpu = ((cpu_ticks(pid) - initial_cpu) / os.sysconf('SC_CLK_TCK') / elapsed * 100) if pid else None
            chunks.put((None, (elapsed, cpu)))
        except Exception as error:
            chunks.put((None, error))
        finally:
            response.close()
            connection.close()

    # Drain the socket independently of the event-size analysis. Otherwise the
    # byte-wise decoder's own processing time would look like SSE buffering.
    receiver = threading.Thread(target=receive)
    receiver.start()
    try:
        while True:
            chunk, received = chunks.get()
            if chunk is None:
                if isinstance(received, Exception):
                    raise received
                elapsed, cpu = received
                break
            transferred += len(chunk)
            if decoder:
                for byte in chunk:
                    decoded = decoder.decompress(bytes([byte]))
                    raw_total += len(decoded)
                    consume(decoded, 1, received)
            else:
                offset = 0
                while offset < len(chunk):
                    # Include a delimiter split across the previous read.
                    if offset == 0 and pending.endswith(b'\n') and chunk.startswith(b'\n'):
                        stop = 1
                    else:
                        end = chunk.find(b'\n\n', offset)
                        stop = end + 2 if end >= 0 else len(chunk)
                    part = chunk[offset:stop]
                    raw_total += len(part)
                    consume(part, len(part), received)
                    offset = stop
    finally:
        receiver.join()
    report = {}
    for name, events in groups.items():
        raw = sum(e['raw_payload_bytes'] for e in events)
        wire = sum(e['transferred_bytes'] for e in events)
        ages = [e['arrival_age_ms'] for e in events if 'arrival_age_ms' in e]
        intervals = [b['received_seconds'] - a['received_seconds'] for a, b in zip(events, events[1:])]
        report[name] = dict(event_count=len(events), raw_payload_bytes=raw, transferred_bytes=wire,
                            compression_ratio=raw / wire if wire else None,
                            average_transferred_bytes=wire / len(events),
                            average_bytes_per_second=wire / elapsed,
                            max_transferred_bytes=max(e['transferred_bytes'] for e in events),
                            arrival_age_ms_p50=percentile(ages, 50), arrival_age_ms_p95=percentile(ages, 95),
                            interval_seconds_p50=percentile(intervals, 50), interval_seconds_p95=percentile(intervals, 95))
    return dict(measured_at_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                encoding=encoding, duration_seconds=elapsed, raw_sse_bytes=raw_total,
                transferred_bytes=transferred, compression_ratio=raw_total / transferred,
                average_bytes_per_second=transferred / elapsed, server_cpu_percent_one_core=cpu,
                events=report, sensor_statuses=statuses, final_sensor_status=final_status, dropped_frames=dropped)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('url')
    parser.add_argument('--seconds', type=float, default=60)
    parser.add_argument('--encoding', choices=['gzip', 'identity'], default='gzip')
    parser.add_argument('--pid', type=int, help='local server PID for CPU usage')
    args = parser.parse_args()
    if not math.isfinite(args.seconds) or args.seconds <= 0:
        parser.error('--seconds must be finite and positive')
    print(json.dumps(measure(args.url, args.seconds, args.encoding, args.pid), indent=2))
