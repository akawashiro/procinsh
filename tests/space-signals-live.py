"""Verify signal_generate against a running dev_run server; unavailable sensors fail."""
import ctypes
import json
import os
import signal
import subprocess
import sys
import threading
import time

from sse import Stream

base = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:9092"
target = subprocess.Popen(
    [sys.executable, "-u", "-c", """
import signal, threading, time
signal.signal(signal.SIGUSR1, signal.SIG_IGN)
signal.signal(signal.SIGUSR2, signal.SIG_IGN)
def worker():
    print(threading.get_native_id(), flush=True)
    while True: time.sleep(1)
threading.Thread(target=worker, daemon=True).start()
while True: time.sleep(1)
"""], stdout=subprocess.PIPE, text=True,
)
response = None
reader = None
frames = []
identities = {}
failures = []


def wait_for(predicate, label, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        assert not failures, failures
        if predicate():
            return
        time.sleep(0.05)
    raise AssertionError((label, frames[-1:] or identities))


try:
    tid = int(target.stdout.readline())
    response = Stream(base + "/api/system/events")

    def consume():
        try:
            for line in response:
                if not line.startswith(b"data: "):
                    continue
                value = json.loads(line[6:])
                if "processes" in value:
                    identities.clear()
                    identities.update({n["identity"]["pid"]: n["identity"] for n in value["processes"]})
                if "signals" in value:
                    frames.append(value)
                    state = value["status"]["signals"]
                    if state["state"] in ("unavailable", "error"):
                        failures.append(state)
        except (OSError, ValueError):
            pass

    reader = threading.Thread(target=consume, daemon=True)
    reader.start()
    wait_for(lambda: frames and os.getpid() in identities and target.pid in identities,
             "sensor and process snapshot readiness")
    assert frames[-1]["status"]["signals"]["state"] == "observing"
    source_id = identities[os.getpid()]
    destination_id = identities[target.pid]

    def send():
        os.kill(target.pid, signal.SIGUSR1)
        os.kill(target.pid, signal.SIGUSR2)
        # x86-64 tgkill targets a non-leader thread; the event must still use TGID.
        assert ctypes.CDLL(None).syscall(234, target.pid, tid, signal.SIGUSR1) == 0

    sender = threading.Thread(target=send)
    sender.start()
    sender.join()

    def matching():
        return [event for frame in frames for event in frame["signals"]
                if event["src_pid"] == os.getpid() and event["dst_pid"] == target.pid]

    wait_for(lambda: len(matching()) >= 3, "process and thread signal observations")
    events = matching()
    assert [e["signal"] for e in events] == [signal.SIGUSR1, signal.SIGUSR2, signal.SIGUSR1], events
    assert all(e["source_id"] == source_id and e["destination_id"] == destination_id for e in events)
    assert all(e["timestamp_ns"] > 0 for e in events)
    assert events == sorted(events, key=lambda e: e["timestamp_ns"])
    for _ in range(5000):
        os.kill(target.pid, signal.SIGUSR1)
    wait_for(lambda: frames[-1]["status"]["signals_lost"] > 0, "bounded queue reports burst losses")
    assert all(len(frame["signals"]) <= 1024 for frame in frames)
    print("Live signals passed: SIGUSR1/SIGUSR2, sender/target thread normalization, endpoint identities, timestamps, bounded burst collection")
finally:
    if response:
        response.close(reader)
    target.terminate()
    target.wait(timeout=5)
