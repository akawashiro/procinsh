#!/usr/bin/env python3

"""Connect a terminal to the guest serial console (Ctrl-] disconnects)."""
import os
import select
import socket
import sys
import termios
import tty

path = os.path.join(os.environ.get("VM_DIR", "/tmp/procinsh-recording-vm"), "console.sock")
with socket.socket(socket.AF_UNIX) as console:
    console.connect(path)
    previous = termios.tcgetattr(sys.stdin) if sys.stdin.isatty() else None
    try:
        if previous:
            tty.setraw(sys.stdin)
        while True:
            ready, _, _ = select.select([console, sys.stdin], [], [])
            if console in ready:
                data = console.recv(65536)
                if not data:
                    break
                os.write(sys.stdout.fileno(), data)
            if sys.stdin in ready:
                data = os.read(sys.stdin.fileno(), 65536)
                if not data or b'\x1d' in data:
                    break
                console.sendall(data)
    finally:
        if previous:
            termios.tcsetattr(sys.stdin, termios.TCSADRAIN, previous)
