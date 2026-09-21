"""File I/O integration test. Requires CAP_BPF/CAP_PERFMON (or root).

Run: python3 tests/space-files-live.py [http://127.0.0.1:PORT]
Without a URL, starts and stops a private test server on an ephemeral port.
"""
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request


def fixture(directory):
    path = os.path.join(directory, 'short-lived-file.txt')
    fd = os.open(path, os.O_CREAT | os.O_RDWR, 0o600)
    print(json.dumps({'pid': os.getpid(), 'path': path}), flush=True)
    sys.stdin.readline()
    expected = {'read_bytes': 0, 'write_bytes': 0, 'read_count': 0, 'write_count': 0}

    def record(write, result):
        n = result if isinstance(result, int) else len(result)
        if n > 0:
            mode = 'write' if write else 'read'
            expected[mode + '_bytes'] += n
            expected[mode + '_count'] += 1

    record(True, os.write(fd, b'abcdefgh'))
    record(True, os.pwrite(fd, b'XYZ', 0))
    record(False, os.pread(fd, 100, 0))  # Short successful read, not requested length.
    os.lseek(fd, 0, os.SEEK_SET)
    record(False, os.read(fd, 100))
    record(False, os.read(fd, 100))  # EOF must not create an event.
    record(True, os.writev(fd, [b'12', b'345']))
    record(True, os.pwritev(fd, [b'67', b'890'], 0))
    os.lseek(fd, 0, os.SEEK_SET)
    record(False, os.readv(fd, [bytearray(4), bytearray(100)]))
    record(False, os.preadv(fd, [bytearray(4), bytearray(100)], 0))
    readonly = os.open(path, os.O_RDONLY)
    try:
        os.write(readonly, b'failure')
        raise AssertionError('write to readonly fd succeeded')
    except OSError:
        pass
    os.close(readonly)
    os.close(fd)
    os.unlink(path)  # The path must survive close/unlink before topology refresh.
    print(json.dumps(expected), flush=True)
    sys.stdin.readline()


if len(sys.argv) > 1 and sys.argv[1] == '--fixture':
    fixture(sys.argv[2])
    sys.exit(0)

server = None
child = None
lease = None
response = None
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
logs = tempfile.TemporaryFile(mode='w+')


def api(path, body=None, method=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(base + path, data=data, method=method,
                                 headers={'Content-Type': 'application/json'})
    with opener.open(req, timeout=20) as r:
        return json.load(r)


try:
    if len(sys.argv) > 1:
        base = sys.argv[1]
    else:
        server = subprocess.Popen(['target/debug/procinsh', '--listen', '127.0.0.1:0'],
                                  stdout=subprocess.PIPE, stderr=logs, text=True)
        lines = queue.Queue()
        threading.Thread(target=lambda: [lines.put(line) for line in server.stdout], daemon=True).start()
        deadline = time.monotonic() + 15
        while True:
            line = lines.get(timeout=max(.01, deadline-time.monotonic()))
            if 'http://127.0.0.1:' in line:
                base = line.strip()
                break
    with tempfile.TemporaryDirectory(prefix='procinsh-file-io-') as directory:
        child = subprocess.Popen([sys.executable, __file__, '--fixture', directory],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        ready = json.loads(child.stdout.readline())
        lease = api('/api/space/leases', {})
        response = opener.open(base + '/api/space/events?token=' + lease['token'], timeout=30)
        frames = []

        def consume():
            try:
                for line in response:
                    if line.startswith(b'data: '):
                        data = json.loads(line[6:])
                        if isinstance(data, dict) and 'files' in data:
                            frames.append(data)
            except (OSError, ValueError):
                pass

        threading.Thread(target=consume, daemon=True).start()
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            status = api('/api/space/status')
            if str(status.get('files', '')).startswith('unavailable'):
                raise AssertionError(status['files'])
            snapshot = api('/api/space/snapshot')
            if status.get('files') == 'observing' and any(n['identity']['pid'] == ready['pid'] for n in snapshot['nodes']):
                break
            time.sleep(.2)
        else:
            raise AssertionError(('file sensor/topology did not become ready', status))
        assert 'memory' not in status, status
        child.stdin.write('go\n')
        child.stdin.flush()
        expected = json.loads(child.stdout.readline())
        deadline = time.monotonic() + 5
        actual = {}
        while time.monotonic() < deadline:
            actual = dict.fromkeys(expected, 0)
            events = [e for f in list(frames) for e in f['files']
                      if e['process_id']['pid'] == ready['pid'] and e['path'] == ready['path']]
            for event in events:
                mode = 'write' if event['write'] else 'read'
                actual[mode + '_bytes'] += event['bytes']
                actual[mode + '_count'] += event['count']
            if actual == expected:
                break
            time.sleep(.1)
        assert actual == expected, (actual, expected, [f['status'] for f in frames[-1:]])
        assert len({e['resource'] for e in events}) == 1
        assert not Path(ready['path']).exists()
        api('/api/space/leases', {'token': lease['token']}, 'DELETE')
        lease = None
        if server:
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline and api('/api/space/status')['active']:
                time.sleep(.1)
            assert api('/api/space/status')['files'] == 'idle'
        print('File I/O live checks passed:', actual,
              '(scalar/positioned/vectored I/O, short reads, EOF/errors, immediate close/unlink, no memory sampling, shutdown)')
finally:
    if lease:
        api('/api/space/leases', {'token': lease['token']}, 'DELETE')
    # Closing the streaming socket can wait for a blocked reader; stop the
    # owned server first so the daemon reader naturally finishes.
    for process in (child, server):
        if process and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
    if server and server.returncode not in (0, -15):
        logs.seek(0)
        print(logs.read()[-8000:], file=sys.stderr)
    logs.close()
