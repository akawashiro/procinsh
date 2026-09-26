"""Bounded local perf benchmark. Run after building procinsh and fixtures.

Outputs JSON; does not change kernel settings. Access denial is a failure.
CPU is percent of one core. Compare workload iterations/second as well as CPU.
"""
import argparse
import ctypes
import json
import http.client
import os
import re
import subprocess
import threading
import time
import urllib.request
from sse import Stream

parser = argparse.ArgumentParser()
parser.add_argument('--seconds', type=float, default=4)
parser.add_argument('--threads', type=int, nargs='+', default=[1, 8, 64])
parser.add_argument('--hz', type=int, nargs='+', default=[19, 49, 99])
args = parser.parse_args()
clock_ticks = os.sysconf('SC_CLK_TCK')


def stat(pid):
    text = open(f'/proc/{pid}/stat').read().rsplit(')', 1)[1].split()
    return {'ticks': int(text[11]) + int(text[12]), 'start': int(text[19]),
            'rss': int(text[21]) * os.sysconf('SC_PAGE_SIZE'),
            'fds': len(os.listdir(f'/proc/{pid}/fd'))}


class Iovec(ctypes.Structure):
    _fields_ = [('base', ctypes.c_void_p), ('length', ctypes.c_size_t)]


def progress(pid, address):
    value = ctypes.c_ulong()
    local = Iovec(ctypes.addressof(value), ctypes.sizeof(value))
    remote = Iovec(address, ctypes.sizeof(value))
    libc = ctypes.CDLL(None, use_errno=True)
    n = libc.process_vm_readv(pid, ctypes.byref(local), 1, ctypes.byref(remote), 1, 0)
    if n != ctypes.sizeof(value):
        raise OSError(ctypes.get_errno(), 'read workload counter')
    return value.value


def measure(target, address, seconds, server=None):
    before = stat(target.pid)
    server_before = stat(server.pid) if server else None
    count = progress(target.pid, address)
    start = time.monotonic()
    time.sleep(seconds)
    elapsed = time.monotonic() - start
    result = {'target_cpu_pct': (stat(target.pid)['ticks']-before['ticks'])/clock_ticks/elapsed*100,
              'iterations_per_second': (progress(target.pid, address)-count)/elapsed}
    if server:
        after = stat(server.pid)
        result.update(server_cpu_pct=(after['ticks']-server_before['ticks'])/clock_ticks/elapsed*100,
                      server_rss_bytes=after['rss'], server_fds=after['fds'])
    return result


results = []
for count in args.threads:
    target = subprocess.Popen(['tests/targets/bin/perf_workload', '--allow-inspector', str(count)], stdout=subprocess.PIPE, text=True)
    server = stream = reader = None
    try:
        address = int(target.stdout.readline().split()[1], 16)
        baseline = measure(target, address, args.seconds)
        for hz in args.hz:
            for callchain in [False, True]:
                command = ['target/debug/procinsh', '--listen', '127.0.0.1:0', '--sample-hz', str(hz)]
                if not callchain:
                    command.append('--no-callchain')
                server = subprocess.Popen(command, stderr=subprocess.PIPE, stdout=subprocess.DEVNULL, text=True)
                while True:
                    line = server.stderr.readline()
                    match = re.search(r'http://127.0.0.1:\d+', line)
                    if match:
                        url = match[0]
                        break
                    if server.poll() is not None:
                        raise RuntimeError('server startup failed')
                identity = stat(target.pid)
                stream = Stream(f'{url}/api/processes/events?pid={target.pid}&start_time_ticks={identity["start"]}')
                received = {'bytes': 0, 'samples': None, 'error': None, 'closing': False, 'sample_received_at': 0}
                def consume():
                    event = ''
                    try:
                        for line in stream:
                            received['bytes'] += len(line)
                            if line.startswith(b'event: '):
                                event = line[7:].strip()
                            elif line.startswith(b'data: ') and event == b'samples':
                                received['samples'] = json.loads(line[6:])
                                received['sample_received_at'] = time.monotonic()
                    except (OSError, ValueError, http.client.HTTPException) as error:
                        if not received['closing']:
                            received['error'] = str(error)
                reader = threading.Thread(target=consume, daemon=True)
                reader.start()
                time.sleep(1.5)
                start_bytes = received['bytes']
                row = measure(target, address, args.seconds, server)
                data = received['samples']
                if received['error'] or time.monotonic()-received['sample_received_at'] > 3 or not data or data['status'] != 'active' or not data['threads']:
                    raise RuntimeError(f'perf unavailable or partial: {data}')
                row.update(threads=count, hz=hz, callchain=callchain, baseline=baseline,
                           lost_total=data['lost_total'], malformed_total=data['malformed_total'],
                           sampled_threads=len(data['threads']), sse_bytes_per_second=(received['bytes']-start_bytes)/args.seconds)
                results.append(row)
                print(json.dumps(row), flush=True)
                received['closing'] = True
                stream.close(reader); stream = reader = None
                server.terminate(); server.wait(timeout=10); server = None
    finally:
        if stream:
            received['closing'] = True
            stream.close(reader)
        if server:
            server.terminate(); server.wait(timeout=10)
        target.terminate(); target.wait(timeout=5)
