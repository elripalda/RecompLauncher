"""Run the pinned builder in its owned group; do not leave compilers behind after a crash."""
import os
import runpy
import signal
import sys
import threading
import time

owner = os.getppid()
if owner <= 1:
    raise SystemExit(130)


def watch_owner():
    while True:
        time.sleep(0.2)
        if os.getppid() != owner:
            os.killpg(os.getpgrp(), signal.SIGKILL)


threading.Thread(target=watch_owner, daemon=True).start()
sys.argv = sys.argv[1:]
sys.path.insert(0, os.path.dirname(sys.argv[0]))
runpy.run_path(sys.argv[0], run_name="__main__")
