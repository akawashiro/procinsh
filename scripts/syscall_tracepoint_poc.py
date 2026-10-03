#!/usr/bin/env python3
"""Run #109 workloads through dev_run.sh (with a test harness installed there)."""
import argparse
import json
import os
import re
import subprocess
import tempfile
import time

configs = ["baseline", "light", "all-2048", "all-4096", "all-8192", "blocking-2048", "blocking-4096", "blocking-8192", "exit-stack-8192", "with-context-2048", "context-2048"]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--launcher", default="./scripts/dev_run.sh")
parser.add_argument("--mode", choices=["sleep", "futex", "epoll", "busy", "initial"], default="sleep")
parser.add_argument("--configs", nargs="+", choices=configs, default=configs)
parser.add_argument("--repeats", type=int, default=3)
parser.add_argument("--seconds", type=int, default=5)
parser.add_argument("--wait-ms", type=int, default=1000)
parser.add_argument("--enter-id", type=int)
parser.add_argument("--exit-id", type=int)
parser.add_argument("--omit-frame-pointer", action="store_true")
args = parser.parse_args()
if not 1 <= args.wait_ms <= 8000:
    parser.error("wait-ms must be 1..8000")
if args.repeats < 1 or not 1 <= args.seconds <= 6:
    parser.error("repeats must be positive and seconds must be 1..6")
if (args.enter_id is None) != (args.exit_id is None):
    parser.error("provide both enter-id and exit-id or neither")
if args.enter_id is None:
    probe = subprocess.run([args.launcher, "syscall_tracepoint_ids", "--ignored", "--nocapture"], capture_output=True, text=True, check=True)
    match = re.search(r"SYSCALL_TRACEPOINT_IDS enter=(\d+) exit=(\d+)", probe.stdout + probe.stderr)
    if not match:
        raise RuntimeError("no verified syscall tracepoint IDs")
    args.enter_id, args.exit_id = map(int, match.groups())
print(f"tracepoint_ids enter={args.enter_id} exit={args.exit_id}", flush=True)
with tempfile.TemporaryDirectory(prefix="procinsh-syscall-") as directory:
    fixture = os.path.join(directory, "target")
    subprocess.run(["cc", "-O2", "-g", "-pthread", "-fomit-frame-pointer" if args.omit_frame_pointer else "-fno-omit-frame-pointer", "-fno-optimize-sibling-calls", "tests/targets/syscall_wait.c", "-o", fixture], check=True)
    for repeat in range(args.repeats):
        for config in args.configs:
            print(f"repeat={repeat} mode={args.mode} config={config}", flush=True)
            with subprocess.Popen([fixture, args.mode], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=dict(os.environ, PROCINSH_WAIT_MS=str(args.wait_ms))) as target:
                tid = target.stdout.readline().strip()
                time.sleep(0.1)
                result = {"repeat": repeat, "mode": args.mode, "config": config, "frame_pointer_omitted": args.omit_frame_pointer, "wait_ms": args.wait_ms}
                if config != "baseline":
                    env = {key: value for key, value in os.environ.items() if not key.startswith("PROCINSH_SYSCALL_")}
                    env.update(PROCINSH_SYSCALL_TID=tid, PROCINSH_SYS_ENTER_ID=str(args.enter_id), PROCINSH_SYS_EXIT_ID=str(args.exit_id), PROCINSH_SYSCALL_SECONDS=str(args.seconds), PROCINSH_SYSCALL_STACK="0" if config == "light" else config.split("-")[-1])
                    if config.startswith("blocking"):
                        env["PROCINSH_SYSCALL_FILTER"] = "1"
                    if config.startswith("exit-stack"):
                        env["PROCINSH_SYSCALL_EXIT_STACK"] = "1"
                    if config.startswith("with-context"):
                        env["PROCINSH_SYSCALL_CONTEXT"] = "1"
                    test = "syscall_context_baseline" if config.startswith("context-") else "syscall_tracepoint_poc"
                    observation = subprocess.run([args.launcher, test, "--ignored", "--nocapture"], env=env, capture_output=True, text=True)
                    output = observation.stdout + observation.stderr
                    print(output, flush=True)
                    if observation.returncode:
                        target.kill()
                        raise RuntimeError(f"observation failed ({observation.returncode})")
                    line = next((line for line in output.splitlines() if line.startswith("SYSCALL_RESULT ")), None)
                    if not line:
                        target.kill()
                        raise RuntimeError("missing syscall result")
                    result["observation"] = json.loads(line.split(" ", 1)[1])
                stdout, stderr = target.communicate(timeout=20)
                if target.returncode:
                    raise RuntimeError(f"target failed ({target.returncode}): {stderr}")
                match = re.search(r"iterations=(\d+) wall=([\d.]+) cpu=([\d.]+)", stdout)
                if not match:
                    raise RuntimeError("missing workload metrics")
                result["target"] = {"iterations": int(match[1]), "wall_s": float(match[2]), "cpu_s": float(match[3])}
                print("RUN_RESULT " + json.dumps(result, separators=(",", ":")), flush=True)
