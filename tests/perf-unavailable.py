"""Deny only perf_event_open in a child server; verify /proc and memory survive."""
import ctypes
import errno
import json
import re
import subprocess
import urllib.request
from sse import Stream


def deny_perf():
    class Filter(ctypes.Structure):
        _fields_ = [('code', ctypes.c_ushort), ('jt', ctypes.c_ubyte), ('jf', ctypes.c_ubyte), ('k', ctypes.c_uint)]
    class Program(ctypes.Structure):
        _fields_ = [('length', ctypes.c_ushort), ('filters', ctypes.POINTER(Filter))]
    # x86-64: load syscall nr; if perf_event_open return EPERM, otherwise allow.
    filters = (Filter * 4)(Filter(0x20, 0, 0, 0), Filter(0x15, 0, 1, 298),
                           Filter(0x06, 0, 0, 0x50000 | errno.EPERM), Filter(0x06, 0, 0, 0x7fff0000))
    program = Program(4, filters)
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(38, 1, 0, 0, 0) or libc.prctl(22, 2, ctypes.byref(program), 0, 0):
        raise OSError(ctypes.get_errno(), 'install test seccomp filter')


target = subprocess.Popen(['tests/targets/bin/busy_loop', '--allow-inspector'], stdout=subprocess.PIPE, text=True)
server = stream = None
try:
    address = target.stdout.readline().split()[1]
    server = subprocess.Popen(['target/debug/procinsh', '--listen', '127.0.0.1:0'],
                              stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                              text=True, preexec_fn=deny_perf)
    while True:
        line = server.stderr.readline()
        match = re.search(r'http://127.0.0.1:\d+', line)
        if match:
            url = match[0]
            break
        if server.poll() is not None:
            raise RuntimeError('server startup failed')
    processes = json.load(urllib.request.urlopen(url + '/api/processes'))
    identity = next(p['identity'] for p in processes if p['identity']['pid'] == target.pid)
    query = f'pid={target.pid}&start_time_ticks={identity["start_time_ticks"]}'
    stream = Stream(url + '/api/processes/events?' + query)
    event = ''
    observations = 0
    for line in stream:
        if line.startswith(b'event: '):
            event = line[7:].strip()
        if line.startswith(b'data: '):
            data = json.loads(line[6:])
            if event == b'observation':
                observations += 1
            if event == b'samples' and data['status'] == 'unavailable':
                assert 'CAP_PERFMON' in ' '.join(data['warnings'])
                break
    assert observations > 0
    for endpoint in ['observation', 'threads', 'maps']:
        assert json.load(urllib.request.urlopen(url + f'/api/processes/{endpoint}?' + query))
    memory = json.load(urllib.request.urlopen(url + '/api/processes/memory?' + query + '&address=' + address + '&length=8'))
    assert len(memory['bytes']) == 8
    print('Perf denied: samples unavailable, observation/threads/maps/memory remain usable')
finally:
    if stream:
        stream.close()
    if server:
        server.terminate(); server.wait(timeout=10)
    target.terminate(); target.wait(timeout=5)
