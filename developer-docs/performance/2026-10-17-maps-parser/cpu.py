#!/usr/bin/env python3
"""Read process and thread CPU counters without observing through procinsh."""
import json
import os
import pathlib
import sys
import time


def stat(path):
    text = pathlib.Path(path).read_text()
    name = text[text.index('(') + 1:text.rindex(')')]
    fields = text[text.rindex(')') + 2:].split()
    return dict(name=name, user=int(fields[11]), system=int(fields[12]),
                start=int(fields[19]), rss=int(fields[21]) * os.sysconf('SC_PAGE_SIZE'))


def snapshot(pid):
    process = stat(f'/proc/{pid}/stat')
    threads = {}
    for entry in pathlib.Path(f'/proc/{pid}/task').iterdir():
        try:
            item = stat(entry / 'stat')
            threads[f'{entry.name}:{item["start"]}'] = dict(tid=int(entry.name), **item)
        except (FileNotFoundError, ProcessLookupError):
            pass
    return dict(monotonic=time.monotonic(), process=process, threads=threads)


def measure(pid, seconds, output):
    hz = os.sysconf('SC_CLK_TCK')
    first = snapshot(pid)
    previous = first
    rows = []
    totals = {}
    start_boot_ticks = float(pathlib.Path('/proc/uptime').read_text().split()[0]) * hz
    deadline = first['monotonic'] + seconds
    next_tick = first['monotonic'] + 1
    while previous['monotonic'] < deadline:
        time.sleep(max(0, min(next_tick, deadline) - time.monotonic()))
        current = snapshot(pid)
        if current['process']['start'] != first['process']['start']:
            raise RuntimeError('backend PID was reused')
        for key, thread in current['threads'].items():
            old = previous['threads'].get(key)
            total = totals.setdefault(key, dict(tid=thread['tid'], name=thread['name'],
                                                start=thread['start'], user_ticks=0, system_ticks=0))
            if old is not None:
                total['user_ticks'] += max(0, thread['user'] - old['user'])
                total['system_ticks'] += max(0, thread['system'] - old['system'])
            elif thread['start'] >= start_boot_ticks:
                total['user_ticks'] += thread['user']
                total['system_ticks'] += thread['system']
        rows.append(current)
        previous = current
        next_tick += 1
    elapsed = previous['monotonic'] - first['monotonic']
    user = (previous['process']['user'] - first['process']['user']) / hz
    system = (previous['process']['system'] - first['process']['system']) / hz
    for thread in totals.values():
        thread['cpu_percent'] = (thread['user_ticks'] + thread['system_ticks']) / hz / elapsed * 100
    result = dict(pid=pid, clock_ticks_per_second=hz, elapsed_seconds=elapsed,
                  user_seconds=user, system_seconds=system,
                  cpu_percent=(user + system) / elapsed * 100,
                  peak_rss_bytes=max(row['process']['rss'] for row in [first, *rows]),
                  monotonic_start=first['monotonic'], monotonic_end=previous['monotonic'],
                  threads=sorted(totals.values(), key=lambda t: -t['cpu_percent']),
                  thread_accounting='Lower bound for threads that exit between one-second reads.',
                  samples=[first, *rows])
    pathlib.Path(output).write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({key: value for key, value in result.items() if key not in ('samples', 'threads')}))


if __name__ == '__main__':
    measure(int(sys.argv[1]), float(sys.argv[2]), sys.argv[3])
