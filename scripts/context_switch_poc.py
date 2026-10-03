#!/usr/bin/env python3
"""Run repeatable #107 experiments with an already-built Rust test executable."""
import argparse
import os
import subprocess
import time
import tempfile
import atexit

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("test_binary")
parser.add_argument("--repeats", type=int, default=3)
parser.add_argument("--seconds", type=int, default=5)
parser.add_argument("--mode", choices=["sleep", "socket", "fsync", "busy", "initial"], default="sleep")
parser.add_argument("--omit-frame-pointer", action="store_true")
args = parser.parse_args()
if args.repeats < 1 or not 1 <= args.seconds <= 6:
    parser.error("repeats must be positive and seconds must be 1..6")
workdir = tempfile.TemporaryDirectory(prefix="procinsh-context-switch-")
atexit.register(workdir.cleanup)
fixture = os.path.join(workdir.name, "target")
subprocess.run(["cc", "-g", "-O2", "-fomit-frame-pointer" if args.omit_frame_pointer else "-fno-omit-frame-pointer", "-fno-optimize-sibling-calls", "tests/targets/context_switch.c", "-o", fixture], check=True)
for repeat in range(args.repeats):
    for config in ["baseline", "clock", "context-2048", "context-4096", "context-8192", "both-8192", "no-switch-8192"]:
        print(f"repeat={repeat} mode={args.mode} config={config}", flush=True)
        with subprocess.Popen([fixture, args.mode], stdout=subprocess.PIPE, text=True) as target:
            tid = target.stdout.readline().strip()
            time.sleep(0.2)  # initial mode is already sleeping before attachment.
            if config not in ("baseline", "clock"):
                env = dict(os.environ, PROCINSH_POC_TID=tid, PROCINSH_POC_SECONDS=str(args.seconds), PROCINSH_POC_STACK=config.split("-")[-1], PROCINSH_POC_SWITCH="0" if config.startswith("no-switch") else "1")
                if config.startswith("both"):
                    env["PROCINSH_POC_CLOCK"] = "1"
                subprocess.run([args.test_binary, "context_switch_poc", "--ignored", "--nocapture"], env=env, check=True)
            elif config == "clock":
                env = dict(os.environ, PROCINSH_POC_TID=tid, PROCINSH_POC_SECONDS=str(args.seconds), PROCINSH_POC_CLOCK_ONLY="1")
                subprocess.run([args.test_binary, "context_switch_poc", "--ignored", "--nocapture"], env=env, check=True)
            print(target.communicate(timeout=12)[0].strip(), flush=True)
