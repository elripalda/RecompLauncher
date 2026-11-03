#!/usr/bin/env python3
"""Tests for recomplauncher_mcp.py against a stand-in control API on loopback.

    python3 -m unittest discover -s tools/control-api -p 'test_*.py'
"""

import http.server
import json
import os
import subprocess
import sys
import tempfile
import threading
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ADAPTER = os.path.join(HERE, "recomplauncher_mcp.py")
TOKEN = "t" * 64


class FakeApi(http.server.BaseHTTPRequestHandler):
    seen = []

    def log_message(self, *a):
        pass

    def reply(self, status, obj=None):
        body = b"" if obj is None else json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Length", str(len(body)))
        if obj is not None:
            self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(body)

    def authorized(self):
        FakeApi.seen.append({"path": self.path, "auth": self.headers.get("Authorization"), "host": self.headers.get("Host"), "origin": self.headers.get("Origin")})
        if self.headers.get("Authorization") != "Bearer " + TOKEN:
            self.reply(401, {"ok": False, "error": {"code": "unauthorized"}})
            return False
        return True

    def do_GET(self):
        if not self.authorized():
            return
        if self.path.startswith("/v1/status"):
            return self.reply(200, {"ok": True, "app": {"name": "fake"}})
        self.reply(404, {"ok": False, "error": {"code": "not_found"}})

    def do_POST(self):
        if not self.authorized():
            return
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])) or b"{}")
        if self.path.startswith("/mcp"):
            if "id" not in body:
                return self.reply(202)
            if body["method"] == "tools/list":
                meta = self.path.endswith("tools=meta")
                return self.reply(200, {"jsonrpc": "2.0", "id": body["id"], "result": {"tools": [{"name": "rlapi_status"}] + ([] if meta else [{"name": "rl_get_state"}])}})
            return self.reply(200, {"jsonrpc": "2.0", "id": body["id"], "result": {"echo": body["method"]}})
        if self.path == "/v1/invoke":
            if body["command"] == "fail":
                return self.reply(422, {"ok": False, "error": {"code": "command_failed", "message": "nope"}})
            return self.reply(200, {"ok": True, "result": {"command": body["command"], "args": body["args"], "async": body["async"]}})
        self.reply(404, {"ok": False})


class AdapterTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), FakeApi)
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()
        cls.port = cls.server.server_address[1]

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()

    def setUp(self):
        FakeApi.seen = []
        self.tmp = tempfile.TemporaryDirectory()
        d = os.path.join(self.tmp.name, "control-api")
        os.makedirs(d, mode=0o700)
        self.endpoint = os.path.join(d, "endpoint.json")
        self.write_endpoint(TOKEN, os.getpid())

    def tearDown(self):
        self.tmp.cleanup()

    def write_endpoint(self, token, pid, mode=0o600):
        with open(self.endpoint, "w") as f:
            json.dump({"schema": "recomplauncher-control-api/1", "port": self.port, "token": token, "pid": pid}, f)
        os.chmod(self.endpoint, mode)

    def run_adapter(self, args, stdin=b"", env_extra=None):
        env = dict(os.environ)
        # A proxy that doesn't exist: the adapter must never use it for 127.0.0.1.
        env.update({"HTTP_PROXY": "http://127.0.0.1:9", "http_proxy": "http://127.0.0.1:9", "NO_PROXY": "", "no_proxy": ""})
        env.update(env_extra or {})
        return subprocess.run([sys.executable, ADAPTER, "--endpoint", self.endpoint] + args, input=stdin, capture_output=True, env=env, timeout=60)

    def test_mcp_stdio_carries_only_protocol_messages(self):
        lines = [
            {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
        ]
        stdin = ("\n".join(json.dumps(m) for m in lines) + "\n\nnot json\n").encode()
        p = self.run_adapter(["mcp"], stdin)
        self.assertEqual(p.returncode, 0, p.stderr)
        out = [json.loads(l) for l in p.stdout.decode().splitlines()]
        self.assertEqual([o.get("id") for o in out], [1, 2, None], "notification answered with nothing; parse error has null id")
        self.assertEqual(out[0]["result"]["echo"], "initialize")
        self.assertEqual([t["name"] for t in out[1]["result"]["tools"]], ["rlapi_status", "rl_get_state"])
        self.assertEqual(out[2]["error"]["code"], -32700)
        self.assertTrue(all(s["auth"] == "Bearer " + TOKEN and s["origin"] is None for s in FakeApi.seen))
        self.assertTrue(all(s["host"] == "127.0.0.1:%d" % self.port for s in FakeApi.seen))
        self.assertNotIn(TOKEN, p.stderr.decode() + p.stdout.decode())

    def test_meta_only_tools(self):
        p = self.run_adapter(["mcp", "--tools", "meta"], (json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}) + "\n").encode())
        self.assertEqual([t["name"] for t in json.loads(p.stdout)["result"]["tools"]], ["rlapi_status"])

    def test_unreachable_launcher_gives_errors_per_request(self):
        self.write_endpoint(TOKEN, 2 ** 22 + 12345)  # a pid that isn't running
        stdin = (json.dumps({"jsonrpc": "2.0", "id": 7, "method": "tools/list"}) + "\n" + json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n").encode()
        p = self.run_adapter(["mcp"], stdin)
        out = [json.loads(l) for l in p.stdout.decode().splitlines()]
        self.assertEqual(len(out), 1)
        self.assertEqual((out[0]["id"], out[0]["error"]["code"]), (7, -32000))

    def test_wrong_token_is_reported_not_crashed(self):
        self.write_endpoint("x" * 64, os.getpid())
        p = self.run_adapter(["mcp"], (json.dumps({"jsonrpc": "2.0", "id": 3, "method": "ping"}) + "\n").encode())
        out = json.loads(p.stdout)
        self.assertEqual((out["id"], out["error"]["code"]), (3, -32000))
        self.assertEqual(out["error"]["data"]["code"], "unauthorized")

    @unittest.skipUnless(os.name == "posix", "file modes")
    def test_endpoint_readable_by_others_is_refused(self):
        self.write_endpoint(TOKEN, os.getpid(), mode=0o644)
        p = self.run_adapter(["status"])
        self.assertEqual(p.returncode, 3)
        self.assertIn("other users can read it", p.stderr.decode())
        self.assertEqual(FakeApi.seen, [])

    def test_endpoint_is_refused_where_the_launcher_cant_make_it_private(self):
        import importlib.util
        from unittest import mock
        spec = importlib.util.spec_from_file_location("rl_adapter_under_test", ADAPTER)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        self.assertEqual(mod.read_endpoint(self.endpoint)["token"], TOKEN)
        with mock.patch.object(mod.os, "name", "nt"):
            with self.assertRaises(mod.Unreachable) as e:
                mod.read_endpoint(self.endpoint)
        self.assertIn("macOS and Linux only", str(e.exception))
        self.assertNotIn(TOKEN, str(e.exception))

    def test_cli_call_and_exit_codes(self):
        p = self.run_adapter(["call", "install", '{"id": "zelda64recomp"}', "--arg", "tag=\"v1\"", "--async"])
        self.assertEqual(p.returncode, 0, p.stderr)
        r = json.loads(p.stdout)["result"]
        self.assertEqual(r, {"command": "install", "args": {"id": "zelda64recomp", "tag": "v1"}, "async": True})
        self.assertEqual(self.run_adapter(["call", "fail"]).returncode, 1)
        self.assertEqual(self.run_adapter(["status"]).returncode, 0)

    def test_enable_marker_is_private(self):
        data = os.path.join(self.tmp.name, "data")
        p = subprocess.run([sys.executable, ADAPTER, "--data-dir", data, "enable"], capture_output=True, timeout=30)
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertTrue(os.path.isfile(os.path.join(data, "control-api", "enabled")))
        if os.name == "posix":
            self.assertEqual(os.stat(os.path.join(data, "control-api")).st_mode & 0o777, 0o700)
        subprocess.run([sys.executable, ADAPTER, "--data-dir", data, "disable"], capture_output=True, timeout=30)
        self.assertFalse(os.path.exists(os.path.join(data, "control-api", "enabled")))


if __name__ == "__main__":
    unittest.main()
