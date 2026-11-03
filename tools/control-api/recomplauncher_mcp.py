#!/usr/bin/env python3
"""RecompLauncher control API: MCP stdio adapter and command-line client.

Talks to the local control API inside a running RecompLauncher (see docs/control-api.md).
Standard library only; Python 3.8+.

    recomplauncher_mcp.py mcp                  # MCP server on stdin/stdout (for MCP clients)
    recomplauncher_mcp.py status               # app, window, running games, jobs
    recomplauncher_mcp.py commands [--query Q]
    recomplauncher_mcp.py describe install
    recomplauncher_mcp.py call get_state
    recomplauncher_mcp.py call install '{"id": "zelda64recomp"}' --async
    recomplauncher_mcp.py job job-3 --wait-ms 60000
    recomplauncher_mcp.py events --follow --names install,setup
    recomplauncher_mcp.py enable | disable | launch | endpoint

The endpoint file (URL and bearer token) is read on every request, so a restarted launcher is
picked up. The token is never printed. In `mcp` mode stdout carries only JSON-RPC messages;
everything else goes to stderr.
"""

import argparse
import json
import os
import socket
import stat
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

DEFAULT_IDENTIFIER = "com.recomplauncher.app"
ENDPOINT_SCHEMA = "recomplauncher-control-api/1"
EXIT_OK, EXIT_FAILED, EXIT_USAGE, EXIT_UNREACHABLE = 0, 1, 2, 3


class Unreachable(Exception):
    """The launcher's control API isn't there (not running, or the API is off)."""


def log(msg):
    sys.stderr.write("recomplauncher-mcp: %s\n" % msg)
    sys.stderr.flush()


# ---------- finding the endpoint ----------

def default_data_dir(identifier):
    home = os.path.expanduser("~")
    if sys.platform == "darwin":
        return os.path.join(home, "Library", "Application Support", identifier)
    if os.name == "nt":
        return os.path.join(os.environ.get("APPDATA") or os.path.join(home, "AppData", "Roaming"), identifier)
    return os.path.join(os.environ.get("XDG_DATA_HOME") or os.path.join(home, ".local", "share"), identifier)


def endpoint_path(opts):
    if opts.endpoint:
        return opts.endpoint
    if os.environ.get("RECOMPLAUNCHER_CONTROL_ENDPOINT"):
        return os.environ["RECOMPLAUNCHER_CONTROL_ENDPOINT"]
    return os.path.join(data_dir(opts), "control-api", "endpoint.json")


def data_dir(opts):
    return opts.data_dir or os.environ.get("RECOMPLAUNCHER_DATA_DIR") or default_data_dir(opts.identifier)


def read_endpoint(path):
    try:
        st = os.stat(path)
    except OSError:
        raise Unreachable("no endpoint file at %s: is RecompLauncher running with the control API on? (`enable`, then start it, or `launch`)" % path)
    if os.name != "posix":
        # The launcher doesn't run its control API here (it can't make the endpoint file private
        # to one user), so a file found here isn't one to trust with a token.
        raise Unreachable("the RecompLauncher control API is available on macOS and Linux only; not reading %s" % path)
    if os.name == "posix":
        if st.st_mode & 0o077:
            raise Unreachable("refusing %s: other users can read it (mode %o); it should be 0600" % (path, stat.S_IMODE(st.st_mode)))
        if hasattr(os, "getuid") and st.st_uid != os.getuid():
            raise Unreachable("refusing %s: it belongs to another user" % path)
    with open(path, "r", encoding="utf-8") as f:
        info = json.load(f)
    if info.get("schema") != ENDPOINT_SCHEMA or not info.get("token") or not info.get("port"):
        raise Unreachable("%s isn't a RecompLauncher control API endpoint file" % path)
    if not pid_alive(info.get("pid")):
        raise Unreachable("the launcher that wrote %s (pid %s) isn't running" % (path, info.get("pid")))
    return info


def pid_alive(pid):
    if not isinstance(pid, int) or pid <= 0:
        return True
    if os.name == "nt":
        return True
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True


# ---------- HTTP ----------

# No proxies, ever: the token must only go to 127.0.0.1.
_OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def request(opts, method, path, body=None, timeout=700):
    info = read_endpoint(endpoint_path(opts))
    url = "http://127.0.0.1:%d%s" % (int(info["port"]), path)
    data = None
    headers = {"Authorization": "Bearer " + info["token"], "Host": "127.0.0.1:%d" % int(info["port"])}
    if body is not None:
        data = body if isinstance(body, bytes) else json.dumps(body).encode("utf-8")
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with _OPENER.open(req, timeout=timeout) as r:
            return r.status, r.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read()
    except (urllib.error.URLError, ConnectionError, socket.timeout, OSError) as e:
        raise Unreachable("can't reach the control API at %s: %s" % (url, getattr(e, "reason", e)))


def call_json(opts, method, path, body=None, timeout=700):
    status, raw = request(opts, method, path, body, timeout)
    try:
        return status, json.loads(raw.decode("utf-8")) if raw else {}
    except ValueError:
        return status, {"ok": False, "error": {"code": "bad_response", "message": raw[:200].decode("utf-8", "replace")}}


# ---------- starting the launcher ----------

def socket_path(identifier):
    return "/tmp/%s_si.sock" % identifier.replace(".", "_").replace("-", "_")


def is_running(identifier):
    """Whether a launcher with this identifier runs (its single-instance socket answers)."""
    if os.name != "posix":
        return None
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        s.settimeout(1)
        s.connect(socket_path(identifier))
        return True
    except OSError:
        return False
    finally:
        s.close()


def find_app(opts):
    if opts.app:
        return opts.app
    if os.environ.get("RECOMPLAUNCHER_APP"):
        return os.environ["RECOMPLAUNCHER_APP"]
    if sys.platform == "darwin":
        for p in (os.path.expanduser("~/Applications/RecompLauncher.app"), "/Applications/RecompLauncher.app"):
            if os.path.isdir(p):
                return p
    elif os.name == "nt":
        p = os.path.join(os.environ.get("LOCALAPPDATA", ""), "RecompLauncher", "recomplauncher.exe")
        if os.path.isfile(p):
            return p
    else:
        for d in os.environ.get("PATH", "").split(os.pathsep):
            p = os.path.join(d, "recomplauncher")
            if os.path.isfile(p):
                return p
    return None


def app_binary(app):
    if app.endswith(".app") and os.path.isdir(app):
        return os.path.join(app, "Contents", "MacOS", "recomplauncher")
    return app


def detached(argv):
    kw = {"stdin": subprocess.DEVNULL, "stdout": subprocess.DEVNULL, "stderr": subprocess.DEVNULL, "close_fds": True}
    if os.name == "nt":
        kw["creationflags"] = 0x00000008 | 0x00000200  # DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    else:
        kw["start_new_session"] = True
    subprocess.Popen(argv, **kw)


def launch(opts, wait_s):
    """Starts the launcher with the control API, or turns the API on in the one running."""
    try:
        call_json(opts, "GET", "/v1/status", timeout=10)
        return "already-on"
    except Unreachable:
        pass
    app = find_app(opts)
    if not app:
        raise Unreachable("can't find RecompLauncher; pass --app or set RECOMPLAUNCHER_APP")
    running = is_running(opts.identifier)
    if sys.platform == "darwin" and app.endswith(".app") and not running:
        # A fresh start through LaunchServices, in the background.
        subprocess.run(["/usr/bin/open", "-g", "-n", app, "--args", "--control-api"], check=True, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        how = "started"
    else:
        # The running one is handed --control-api through its single-instance hand-off (this
        # second process then exits); if none runs, this one becomes the launcher.
        detached([app_binary(app), "--control-api"])
        how = "asked-running" if running else "started"
    deadline = time.time() + wait_s
    last = None
    while time.time() < deadline:
        try:
            call_json(opts, "GET", "/v1/status", timeout=5)
            return how
        except Unreachable as e:
            last = e
            time.sleep(0.5)
    raise Unreachable("started RecompLauncher but its control API didn't answer within %ss (%s)" % (wait_s, last))


# ---------- MCP over stdio ----------

def write_msg(obj):
    sys.stdout.write(json.dumps(obj, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def rpc_error(id_, code, message, data=None):
    e = {"code": code, "message": message}
    if data is not None:
        e["data"] = data
    return {"jsonrpc": "2.0", "id": id_, "error": e}


def run_mcp(opts):
    path = "/mcp" + ("?tools=meta" if opts.tools == "meta" else "")
    tried_launch = False
    for raw in sys.stdin.buffer:
        line = raw.strip()
        if not line:
            continue
        try:
            msg = json.loads(line.decode("utf-8"))
        except ValueError as e:
            write_msg(rpc_error(None, -32700, "Parse error", {"detail": str(e)}))
            continue
        ids = [m.get("id") for m in (msg if isinstance(msg, list) else [msg]) if isinstance(m, dict) and "id" in m and "method" in m]
        while True:
            try:
                status, body = request(opts, "POST", path, line)
                break
            except Unreachable as e:
                if opts.autolaunch and not tried_launch:
                    tried_launch = True
                    try:
                        log("launching RecompLauncher: %s" % launch(opts, opts.launch_wait))
                        continue
                    except (Unreachable, OSError, subprocess.SubprocessError) as le:
                        e = le
                log(str(e))
                for i in ids:
                    write_msg(rpc_error(i, -32000, "RecompLauncher control API unavailable: %s" % e))
                status, body = None, None
                break
        if status is None or status == 202 or not body:
            continue
        try:
            reply = json.loads(body.decode("utf-8"))
        except ValueError:
            for i in ids:
                write_msg(rpc_error(i, -32603, "Bad answer from the launcher (HTTP %s)" % status))
            continue
        if status >= 400 and isinstance(reply, dict) and "jsonrpc" not in reply:
            # The transport refused it (auth, size...): say so per request.
            for i in ids:
                write_msg(rpc_error(i, -32000, "Control API refused the request (HTTP %s)" % status, reply.get("error")))
            continue
        write_msg(reply)
    return EXIT_OK


# ---------- command line ----------

def parse_value(v):
    try:
        return json.loads(v)
    except ValueError:
        return v


def show(obj):
    print(json.dumps(obj, indent=2, ensure_ascii=False))


def finish(status, body):
    show(body)
    if isinstance(body, dict) and body.get("ok") is False:
        return EXIT_FAILED
    return EXIT_OK if status < 400 else EXIT_FAILED


def marker(opts):
    return os.path.join(data_dir(opts), "control-api", "enabled")


def main(argv=None):
    ap = argparse.ArgumentParser(prog="recomplauncher_mcp.py", description="RecompLauncher control API client and MCP stdio adapter.")
    ap.add_argument("--endpoint", help="endpoint.json path (default: <data dir>/control-api/endpoint.json)")
    ap.add_argument("--data-dir", help="the launcher's data folder (portable installs, test instances)")
    ap.add_argument("--identifier", default=os.environ.get("RECOMPLAUNCHER_IDENTIFIER", DEFAULT_IDENTIFIER), help="app identifier (default %(default)s)")
    ap.add_argument("--app", help="RecompLauncher.app / executable, for launch and --autolaunch")
    sub = ap.add_subparsers(dest="cmd")
    m = sub.add_parser("mcp", help="MCP server on stdio")
    m.add_argument("--tools", choices=["all", "meta"], default=os.environ.get("RECOMPLAUNCHER_MCP_TOOLS", "all"), help="all: one tool per command plus rlapi_*; meta: only rlapi_* (call commands with rlapi_invoke)")
    m.add_argument("--autolaunch", action="store_true", help="start the launcher (or turn its API on) if it isn't reachable")
    m.add_argument("--launch-wait", type=float, default=45.0)
    sub.add_parser("status")
    c = sub.add_parser("commands")
    c.add_argument("--query", default="")
    c.add_argument("--module", default="")
    d = sub.add_parser("describe")
    d.add_argument("name")
    k = sub.add_parser("call", help="invoke a command")
    k.add_argument("name")
    k.add_argument("args", nargs="?", default=None, help="JSON object of arguments")
    k.add_argument("--arg", action="append", default=[], metavar="KEY=VALUE", help="one argument (VALUE parsed as JSON when it can be)")
    k.add_argument("--async", dest="is_async", action="store_true", help="answer at once with the job")
    k.add_argument("--timeout-ms", type=int, default=30000)
    k.add_argument("--no-validate", action="store_true")
    sub.add_parser("jobs")
    j = sub.add_parser("job")
    j.add_argument("id")
    j.add_argument("--wait-ms", type=int, default=0)
    x = sub.add_parser("cancel")
    x.add_argument("id")
    e = sub.add_parser("events")
    e.add_argument("--after", type=int, default=0)
    e.add_argument("--names", default="")
    e.add_argument("--limit", type=int, default=200)
    e.add_argument("--wait-ms", type=int, default=0)
    e.add_argument("--follow", action="store_true", help="keep printing events as JSON lines")
    a = sub.add_parser("app")
    a.add_argument("action", choices=["status", "show", "hide", "focus", "quit"])
    s = sub.add_parser("settings", help="change some settings (JSON merge patch)")
    s.add_argument("patch")
    sub.add_parser("limitations")
    sub.add_parser("endpoint", help="where the endpoint file is and whether it answers (no token)")
    sub.add_parser("enable", help="turn the API on for every start (creates control-api/enabled)")
    sub.add_parser("disable", help="stop turning it on at start")
    la = sub.add_parser("launch", help="start the launcher with the API, or turn it on in the one running")
    la.add_argument("--wait", type=float, default=45.0)
    opts = ap.parse_args(argv)
    if not opts.cmd:
        ap.print_help(sys.stderr)
        return EXIT_USAGE
    try:
        return run(opts)
    except Unreachable as err:
        log(str(err))
        return EXIT_UNREACHABLE


def run(opts):
    q = urllib.parse.quote
    if opts.cmd == "mcp":
        return run_mcp(opts)
    if opts.cmd == "status":
        return finish(*call_json(opts, "GET", "/v1/status"))
    if opts.cmd == "commands":
        return finish(*call_json(opts, "GET", "/v1/commands?q=%s&module=%s" % (q(opts.query), q(opts.module))))
    if opts.cmd == "describe":
        return finish(*call_json(opts, "GET", "/v1/commands/" + q(opts.name)))
    if opts.cmd == "call":
        args = parse_value(opts.args) if opts.args else {}
        if not isinstance(args, dict):
            log("arguments must be a JSON object")
            return EXIT_USAGE
        for kv in opts.arg:
            key, _, val = kv.partition("=")
            args[key] = parse_value(val)
        body = {"command": opts.name, "args": args, "async": opts.is_async, "timeout_ms": opts.timeout_ms, "validate": not opts.no_validate}
        return finish(*call_json(opts, "POST", "/v1/invoke", body))
    if opts.cmd == "jobs":
        return finish(*call_json(opts, "GET", "/v1/jobs"))
    if opts.cmd == "job":
        return finish(*call_json(opts, "GET", "/v1/jobs/%s?wait_ms=%d" % (q(opts.id), opts.wait_ms)))
    if opts.cmd == "cancel":
        return finish(*call_json(opts, "POST", "/v1/jobs/%s/cancel" % q(opts.id), {}))
    if opts.cmd == "events":
        after = opts.after
        while True:
            wait = opts.wait_ms if not opts.follow else max(opts.wait_ms, 25000)
            status, body = call_json(opts, "GET", "/v1/events?after=%d&names=%s&limit=%d&wait_ms=%d" % (after, q(opts.names), opts.limit, wait))
            if not opts.follow:
                return finish(status, body)
            for ev in body.get("events", []):
                print(json.dumps(ev, ensure_ascii=False), flush=True)
            after = body.get("next_after", after)
    if opts.cmd == "app":
        return finish(*call_json(opts, "POST", "/v1/app", {"action": opts.action}))
    if opts.cmd == "settings":
        return finish(*call_json(opts, "POST", "/v1/settings", {"patch": parse_value(opts.patch)}))
    if opts.cmd == "limitations":
        return finish(*call_json(opts, "GET", "/v1/limitations"))
    if opts.cmd == "endpoint":
        path = endpoint_path(opts)
        out = {"endpoint_file": path, "enabled_marker": os.path.exists(marker(opts))}
        try:
            info = read_endpoint(path)
            out.update({"url": "http://127.0.0.1:%d" % int(info["port"]), "pid": info.get("pid"), "app_version": info.get("app_version"), "identifier": info.get("identifier")})
            status, body = call_json(opts, "GET", "/v1/status", timeout=10)
            out["answers"] = status == 200
        except Unreachable as err:
            out["answers"] = False
            out["problem"] = str(err)
        show(out)
        return EXIT_OK if out["answers"] else EXIT_UNREACHABLE
    if opts.cmd == "enable":
        p = marker(opts)
        os.makedirs(os.path.dirname(p), mode=0o700, exist_ok=True)
        if os.name == "posix":
            os.chmod(os.path.dirname(p), 0o700)
        open(p, "a").close()
        show({"enabled": True, "marker": p, "note": "Takes effect when RecompLauncher next starts (or use `launch`)."})
        return EXIT_OK
    if opts.cmd == "disable":
        p = marker(opts)
        if os.path.exists(p):
            os.remove(p)
        show({"enabled": False, "marker": p, "note": "A running launcher keeps its API until it closes."})
        return EXIT_OK
    if opts.cmd == "launch":
        how = launch(opts, opts.wait)
        show({"ok": True, "how": how, "endpoint_file": endpoint_path(opts)})
        return EXIT_OK
    return EXIT_USAGE


if __name__ == "__main__":
    sys.exit(main())
