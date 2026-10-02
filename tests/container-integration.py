"""Build/run the static binary in its own Docker PID namespace (no --privileged).

Run after static release build: python3 tests/container-integration.py
Docker daemon access, host BTF, and BPF-capable kernel are required.
"""
import ctypes
from pathlib import Path
import subprocess
import sys
import time


def outside():
    ctypes.CDLL(None).prctl(15, b"outside-pidns", 0, 0, 0)
    with open('/tmp/procinsh-outside-pidns', 'wb', buffering=0) as file:
        while True:
            file.write(b'outside namespace\n')
            time.sleep(.01)


if '--outside' in sys.argv:
    outside()
    sys.exit(0)

binary = Path('target/x86_64-unknown-linux-gnu/release/procinsh')
headers = subprocess.check_output(['readelf', '-l', str(binary)], text=True)
assert 'INTERP' not in headers, 'release binary has a dynamic interpreter'
dynamic = subprocess.check_output(['readelf', '-d', str(binary)], text=True)
assert '(NEEDED)' not in dynamic, 'release binary has shared-library dependencies'
subprocess.run(['docker', 'build', '-f', 'tests/container.Dockerfile', '-t', 'procinsh-container-test', '.'], check=True)
external = subprocess.Popen([sys.executable, __file__, '--outside'])
try:
    # Wait for the outside fixture to start; retain it through all sensor checks.
    for _ in range(100):
        if Path(f'/proc/{external.pid}/comm').read_text().strip() == 'outside-pidns':
            break
        time.sleep(.01)
    else:
        raise AssertionError('outside namespace fixture did not start')
    subprocess.run(['docker', 'run', '--rm', '--cap-add=SYS_PTRACE', '--cap-add=BPF',
                    '--cap-add=PERFMON', '-e',
                    f'HOST_PIDNS={Path("/proc/self/ns/pid").readlink()}', '-v', '/sys/kernel/btf:/sys/kernel/btf:ro',
                    'procinsh-container-test'], check=True, timeout=90)
finally:
    external.terminate()
    external.wait(timeout=5)
