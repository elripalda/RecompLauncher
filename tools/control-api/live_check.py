#!/usr/bin/env python3
"""Live check of the control API against a running RecompLauncher.

Reads the endpoint file like the adapter does, then checks the transport and auth, every
registered command's dispatch, MCP (over HTTP and through the stdio adapter), jobs, events, the
window, and (with --writes, only on a test instance) settings, ROM scanning, import, play/stop
and removal using a throwaway fixture folder. Writes a JSON report.

    python3 tools/control-api/live_check.py --identifier com.recomplauncher.apitest \\
        --writes --fixture-dir /tmp/rl-fixture --report report.json \\
        --adopt-path "/path/to/Some Game/Game.app" ...

Commands with required parameters are probed with no arguments and the API's own checks off:
Tauri's argument reading then refuses them inside the command's generated wrapper, before the
command body runs, which shows the call reached that command with its context. Read-only
commands without required parameters are run for real. The rest (they would act) are listed,
not run.
"""

import argparse
import http.client
import json
import os
import stat
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import recomplauncher_mcp as rl  # noqa: E402

REPORT = {"checks": [], "failures": 0}


def check(name, ok, detail=None):
    REPORT["checks"].append({"name": name, "ok": bool(ok), "detail": detail})
    if not ok:
        REPORT["failures"] += 1
    sys.stderr.write("%s %s%s\n" % ("PASS" if ok else "FAIL", name, "" if ok or detail is None else "  -> %s" % json.dumps(detail)[:400]))
    return ok


def raw_http(port, method, path, headers, body=b""):
    c = http.client.HTTPConnection("127.0.0.1", port, timeout=30)
    c.putrequest(method, path, skip_host=True, skip_accept_encoding=True)
    for k, v in headers.items():
        c.putheader(k, v)
    c.putheader("Content-Length", str(len(body)))
    try:
        c.endheaders(body)
    except (ConnectionResetError, BrokenPipeError):
        pass  # refused before the whole body went: the answer may still be there to read
    try:
        r = c.getresponse()
    except (ConnectionResetError, BrokenPipeError, http.client.RemoteDisconnected) as e:
        return -1, {}, {"transport_error": str(e)}
    data = r.read()
    hdrs = {k.lower(): v for k, v in r.getheaders()}
    c.close()
    try:
        return r.status, hdrs, json.loads(data or b"null")
    except ValueError:
        return r.status, hdrs, None


def invoke(opts, command, args=None, **extra):
    body = {"command": command, "args": args or {}}
    body.update(extra)
    return rl.call_json(opts, "POST", "/v1/invoke", body)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--endpoint")
    ap.add_argument("--data-dir")
    ap.add_argument("--identifier", default=rl.DEFAULT_IDENTIFIER)
    ap.add_argument("--app")
    ap.add_argument("--report", required=True)
    ap.add_argument("--writes", action="store_true", help="also run the steps that change the launcher's data (test instance only)")
    ap.add_argument("--real-app-ok", action="store_true", help="allow --writes against com.recomplauncher.app")
    ap.add_argument("--fixture-dir")
    ap.add_argument("--adopt-path", action="append", default=[], help="a folder or program to scan with adopt_pick (read-only)")
    ap.add_argument("--quit", action="store_true", help="quit the launcher through the API at the end")
    opts = ap.parse_args()
    opts.tools = "all"
    if opts.writes and opts.identifier == rl.DEFAULT_IDENTIFIER and not opts.real_app_ok:
        sys.exit("refusing --writes against the real launcher; use a test instance (or --real-app-ok)")

    # ---------- endpoint, auth, browser guards ----------
    ep_path = rl.endpoint_path(opts)
    info = rl.read_endpoint(ep_path)
    port, token = int(info["port"]), info["token"]
    REPORT["endpoint_file"] = ep_path
    REPORT["pid"] = info["pid"]
    REPORT["url"] = "http://127.0.0.1:%d" % port
    if os.name == "posix":
        check("endpoint file is 0600", stat.S_IMODE(os.stat(ep_path).st_mode) == 0o600, oct(os.stat(ep_path).st_mode))
        check("control-api folder is 0700", stat.S_IMODE(os.stat(os.path.dirname(ep_path)).st_mode) == 0o700)
    host = {"Host": "127.0.0.1:%d" % port}
    auth = dict(host, Authorization="Bearer " + token)
    s, h, b = raw_http(port, "GET", "/v1/status", host)
    check("no token -> 401", s == 401 and b["error"]["code"] == "unauthorized", [s, b])
    s, h, b = raw_http(port, "GET", "/v1/status", dict(host, Authorization="Bearer " + "0" * 64))
    check("wrong token -> 401", s == 401, [s, b])
    s, h, b = raw_http(port, "POST", "/v1/invoke", dict(auth, Origin="http://evil.example", **{"Content-Type": "application/json"}), b'{"command":"get_state"}')
    check("browser Origin -> 403 even with token", s == 403 and b["error"]["code"] == "browser_request_forbidden", [s, b])
    s, h, b = raw_http(port, "GET", "/v1/status", {"Host": "evil.example:%d" % port, "Authorization": "Bearer " + token})
    check("DNS-rebinding Host -> 403", s == 403 and b["error"]["code"] == "bad_host", [s, b])
    s, h, b = raw_http(port, "POST", "/v1/invoke", dict(auth, **{"Content-Type": "text/plain"}), b'{"command":"get_state"}')
    check("text/plain POST -> 415", s == 415, [s, b])
    s, h, b = raw_http(port, "OPTIONS", "/v1/invoke", dict(auth, Origin="http://evil.example", **{"Access-Control-Request-Method": "POST"}))
    check("CORS preflight refused, no CORS headers", s in (403, 405) and not any(k.startswith("access-control") for k in h), [s, h])
    s, h, b = raw_http(port, "POST", "/v1/invoke", dict(auth, **{"Content-Type": "application/json"}), b"x" * (1024 * 1024 + 1))
    check("body over 1 MiB -> 413", s == 413, [s, b])

    # ---------- status, discovery ----------
    s, st = rl.call_json(opts, "GET", "/v1/status")
    check("status", s == 200 and st["ok"], st)
    REPORT["status"] = st
    s, cl = rl.call_json(opts, "GET", "/v1/commands")
    commands = cl["commands"]
    REPORT["command_count"] = len(commands)
    check("commands listed", s == 200 and len(commands) == cl["total"] and len(commands) >= 150, len(commands))
    described = {}
    for c in commands:
        s, d = rl.call_json(opts, "GET", "/v1/commands/" + c["name"])
        described[c["name"]] = d.get("command", {})
    check("every command describes itself with a schema", all(d.get("input_schema", {}).get("type") == "object" and d.get("signature_found") for d in described.values()))
    s, lim = rl.call_json(opts, "GET", "/v1/limitations")
    check("limitations listed", s == 200 and "native_dialogs" in lim["limitations"])

    # ---------- every command reaches its dispatch ----------
    coverage = {"probed_invalid_args": [], "ran_read_only": [], "not_run_would_act": [], "problems": []}
    for name, d in sorted(described.items()):
        required = [p for p in d["params"] if p["required"]]
        if required:
            s, r = invoke(opts, name, {}, validate=False)
            err = r.get("error", {})
            if err.get("code") == "invalid_args" and required[0]["key"] in err.get("message", ""):
                coverage["probed_invalid_args"].append(name)
            else:
                coverage["problems"].append({"command": name, "status": s, "answer": r})
        elif d["read_only"]:
            s, r = invoke(opts, name, {}, timeout_ms=60000)
            code = r.get("error", {}).get("code")
            # Reached the command: it answered, or failed in its own words (offline, not signed in).
            if r.get("ok") or code == "command_failed":
                coverage["ran_read_only"].append({"command": name, "ok": bool(r.get("ok")), "state": r.get("state"), "error": r.get("error", {}).get("message") if not r.get("ok") else None})
            else:
                coverage["problems"].append({"command": name, "status": s, "answer": r})
        else:
            coverage["not_run_would_act"].append(name)
    REPORT["coverage"] = coverage
    reached = len(coverage["probed_invalid_args"]) + len(coverage["ran_read_only"])
    check("every probed command reached its own handler (%d probed, %d read-only run, %d not run)" % (len(coverage["probed_invalid_args"]), len(coverage["ran_read_only"]), len(coverage["not_run_would_act"])), not coverage["problems"] and reached + len(coverage["not_run_would_act"]) == len(commands), coverage["problems"])

    # ---------- structured errors ----------
    s, r = invoke(opts, "no_such_command")
    check("unknown command -> 404 unknown_command", s == 404 and r["error"]["code"] == "unknown_command", r)
    s, r = invoke(opts, "install", {})
    check("missing argument -> 400 with the missing names", s == 400 and r["error"]["details"]["missing"] == ["id"], r)
    s, r = invoke(opts, "play", {"id": "no-such-game-api-check"})
    check("command's own failure -> structured command_failed", s == 422 and r["error"]["code"] == "command_failed", r)
    s, r = invoke(opts, "uninstall", {"id": "no-such-game-api-check", "delete_files": False})
    check("snake_case arguments accepted (delete_files -> deleteFiles)", r.get("error", {}).get("code") != "invalid_args", r)

    # ---------- MCP over HTTP ----------
    def mcp(msg, path="/mcp"):
        return rl.call_json(opts, "POST", path, msg)
    s, r = mcp({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "live_check", "version": "1"}}})
    check("MCP initialize", s == 200 and r["result"]["protocolVersion"] == "2025-06-18" and r["result"]["serverInfo"]["name"] == "recomplauncher", r)
    s, r = mcp({"jsonrpc": "2.0", "method": "notifications/initialized"})
    check("MCP notification -> 202, no body", s == 202 and r == {}, [s, r])
    s, r = mcp({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
    tools = r["result"]["tools"]
    REPORT["mcp_tools"] = len(tools)
    check("MCP tools/list: one tool per command plus rlapi_*", len([t for t in tools if t["name"].startswith("rl_")]) == len(commands) and any(t["name"] == "rlapi_status" for t in tools), len(tools))
    s, r = mcp({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "rl_net_state", "arguments": {}}})
    check("MCP tools/call rl_net_state", s == 200 and r["result"]["isError"] is False, r)
    s, r = mcp({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "rlapi_status", "arguments": {}}})
    check("MCP tools/call rlapi_status", r["result"]["structuredContent"]["ok"] is True, r)

    # ---------- MCP through the stdio adapter ----------
    lines = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "live_check", "version": "1"}}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
        {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "rl_get_state", "arguments": {}}},
    ]
    cmd = [sys.executable, os.path.join(HERE, "recomplauncher_mcp.py"), "--endpoint", ep_path, "mcp"]
    p = subprocess.run(cmd, input=("\n".join(json.dumps(m) for m in lines) + "\n").encode(), capture_output=True, timeout=120)
    out = [json.loads(l) for l in p.stdout.decode().splitlines()]
    check("stdio adapter: only JSON-RPC on stdout, one answer per request", p.returncode == 0 and [o.get("id") for o in out] == [1, 2, 3], {"stderr": p.stderr.decode()[-500:], "ids": [o.get("id") for o in out]})
    if len(out) == 3:
        sc = out[2]["result"]["structuredContent"]
        check("stdio adapter: rl_get_state returns live state", sc["ok"] and "library" in sc["result"] and "settings" in sc["result"], list(sc.get("result", {}).keys())[:10])
        REPORT["live_library_ids"] = sorted(sc["result"]["library"].keys())
    check("token never printed by the adapter", token not in p.stdout.decode() + p.stderr.decode())

    # ---------- jobs and events ----------
    s, ev = rl.call_json(opts, "GET", "/v1/events?after=0&limit=500")
    start_seq = ev["last_seq"]
    s, r = invoke(opts, "game_sizes", {}, **{"async": True})
    jid = r["job_id"]
    check("async call answers at once with a job", s == 202 and r["state"] == "running" or r["state"] == "succeeded", r)
    s, j = rl.call_json(opts, "GET", "/v1/jobs/%s?wait_ms=60000" % jid)
    check("job finishes and keeps its result", j["job"]["state"] == "succeeded" and "result" in j["job"], j)
    s, c = rl.call_json(opts, "POST", "/v1/jobs/%s/cancel" % jid, {})
    check("finished job can't be cancelled", s == 409 and c["error"]["code"] == "already_finished", c)
    s, js = rl.call_json(opts, "GET", "/v1/jobs")
    check("jobs listed", s == 200 and any(x["id"] == jid for x in js["jobs"]))
    s, w = rl.call_json(opts, "POST", "/v1/app", {"action": "status"})
    check("window status", s == 200 and w["window"]["label"] == "main", w)
    REPORT["window_before"] = w["window"]

    # ---------- writes: test instance only ----------
    if opts.writes:
        fx = opts.fixture_dir
        os.makedirs(fx, exist_ok=True)
        roms = os.path.join(fx, "roms")
        os.makedirs(roms, exist_ok=True)
        # Not a game: a few bytes with an N64 image's first word, named like a dump.
        with open(os.path.join(roms, "API Check Fixture (USA).z64"), "wb") as f:
            f.write(b"\x80\x37\x12\x40" + b"\x00" * 4092)
        game = os.path.join(fx, "Fixture Game")
        os.makedirs(game, exist_ok=True)
        exe = os.path.join(game, "fixture-game")
        with open(exe, "w") as f:
            f.write("#!/bin/sh\n# Control API live check: stands in for a game, then exits.\nsleep 20\n")
        os.chmod(exe, 0o755)

        s, before = invoke(opts, "get_state")
        old_settings = before["result"]["settings"]
        stamp = int(time.time())
        s, r = rl.call_json(opts, "POST", "/v1/settings", {"patch": {"rom_dirs": [roms], "ui": {"control_api_check": stamp}}})
        check("settings patch saved through patch_settings", s == 200 and r["ok"], r)
        s, after = invoke(opts, "get_state")
        sa = after["result"]["settings"]
        check("settings patch visible in live state, rest kept", sa["rom_dirs"] == [roms] and sa["ui"].get("control_api_check") == stamp and sa["library_dir"] == old_settings["library_dir"], {"rom_dirs": sa.get("rom_dirs")})
        s, r = invoke(opts, "scan_roms", {}, timeout_ms=120000)
        found = r.get("result") or []
        REPORT["scan_roms"] = found
        check("scan_roms over the fixture ROM folder", s == 200 and isinstance(found, list), r)
        s, r = invoke(opts, "fs_list", {"path": roms})
        check("fs_list browses a folder", s == 200 and r["ok"], r)
        s, r = invoke(opts, "add_any", {"path": exe})
        check("import a game folder (add_any)", s == 200 and r["ok"], r)
        gid = (r.get("result") or {}).get("id")
        REPORT["fixture_game_id"] = gid
        if gid:
            s, r = invoke(opts, "play", {"id": gid}, timeout_ms=60000)
            check("play the imported fixture", s == 200 and r["ok"], r)
            running = False
            for _ in range(40):
                s, stt = invoke(opts, "get_state")
                if gid in stt["result"]["running"]:
                    running = True
                    break
                time.sleep(0.25)
            check("running state shows the game", running)
            s, r = invoke(opts, "stop", {"id": gid})
            check("stop", s == 200 and r["ok"], r)
            stopped = False
            for _ in range(60):
                s, stt = invoke(opts, "get_state")
                if gid not in stt["result"]["running"]:
                    stopped = True
                    break
                time.sleep(0.25)
            check("running state clears after stop", stopped)
            s, r = invoke(opts, "recent_log", {"id": gid, "since": 0})
            check("recent_log answers", s == 200, r)
            s, r = invoke(opts, "backups", {"id": gid})
            check("backups answers", s in (200, 422), r)
            s, r = invoke(opts, "uninstall", {"id": gid, "deleteFiles": False})
            check("remove the fixture from the Library (files kept)", s == 200 and r["ok"] and os.path.isfile(exe), r)
        s, ev2 = rl.call_json(opts, "GET", "/v1/events?after=%d&limit=500" % start_seq)
        names = [e["name"] for e in ev2["events"]]
        REPORT["events_seen"] = sorted(set(names))
        check("events captured: game started/stopped and change notices", "game" in names and "control-api-changed" in names, sorted(set(names)))
        game_states = [e["payload"].get("state") for e in ev2["events"] if e["name"] == "game"]
        check("game events say running then stopped", "running" in game_states and "stopped" in game_states, game_states)
        s, r = rl.call_json(opts, "POST", "/v1/settings", {"patch": {"rom_dirs": old_settings.get("rom_dirs", []), "ui": {"control_api_check": None}}})
        check("settings put back", s == 200 and r["ok"], r)
        for action in ("hide", "show"):
            s, w = rl.call_json(opts, "POST", "/v1/app", {"action": action})
            check("window %s" % action, s == 200 and w["ok"], w)

    # ---------- installed-game scan of given paths (read-only) ----------
    adopt = []
    for pth in opts.adopt_path:
        s, r = invoke(opts, "adopt_pick", {"path": pth}, timeout_ms=60000)
        adopt.append({"path": pth, "status": s, "found": r.get("result"), "error": r.get("error")})
    if adopt:
        REPORT["adopt_pick"] = adopt
        check("adopt_pick scans each given path", all(a["status"] == 200 for a in adopt), [a for a in adopt if a["status"] != 200])

    if opts.quit:
        s, r = rl.call_json(opts, "POST", "/v1/app", {"action": "quit"})
        check("quit through the API", s == 200 and r["ok"], r)
        gone = False
        for _ in range(40):
            time.sleep(0.25)
            if not rl.pid_alive(info["pid"]):
                gone = True
                break
        check("process ended", gone)
        check("endpoint file removed at exit", not os.path.exists(ep_path))

    with open(opts.report, "w") as f:
        json.dump(REPORT, f, indent=2)
    sys.stderr.write("\n%d checks, %d failed. Report: %s\n" % (len(REPORT["checks"]), REPORT["failures"], opts.report))
    return 1 if REPORT["failures"] else 0


if __name__ == "__main__":
    sys.exit(main())
