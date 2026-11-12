#!/usr/bin/env python3
"""Tests for the stdio adapter, against a stand-in launcher on a loopback port.

  python3 tools/control-api/test_recomplauncher_mcp.py

(cargo test also runs the adapter against the launcher's real server.)
"""
import http.server
import json
import os
import sys
import tempfile
import threading
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import recomplauncher_mcp as adapter  # noqa: E402

TOKEN = 'test-token-0123456789'


class Launcher(http.server.BaseHTTPRequestHandler):
    """Answers POST /mcp like the launcher: needs the token, echoes the method back."""
    seen = []

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get('Content-Length', '0')))
        Launcher.seen.append((self.path, self.headers.get('Authorization'), body))
        if self.headers.get('Authorization') != 'Bearer ' + TOKEN:
            return self.reply(401, {'ok': False, 'error': {'code': 'unauthorized', 'message': 'A valid bearer token is required.'}})
        msg = json.loads(body)
        if 'id' not in msg:
            return self.reply(202, None)
        self.reply(200, {'jsonrpc': '2.0', 'id': msg['id'], 'result': {'method': msg['method']}})

    def reply(self, status, body):
        data = b'' if body is None else json.dumps(body, indent=2).encode()
        self.send_response(status)
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *args):
        pass


class AdapterTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = http.server.HTTPServer(('127.0.0.1', 0), Launcher)
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()
        cls.port = cls.server.server_address[1]

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()

    def setUp(self):
        Launcher.seen = []
        self.dir = tempfile.TemporaryDirectory()
        self.endpoint = os.path.join(self.dir.name, 'endpoint.json')

    def tearDown(self):
        self.dir.cleanup()

    def write_endpoint(self, url=None, token=TOKEN, mode=0o600):
        with open(self.endpoint, 'w', encoding='utf-8') as f:
            json.dump({'mcp_url': url or 'http://127.0.0.1:%d/mcp' % self.port, 'token': token}, f)
        # As the launcher writes it: only this user's.
        os.chmod(self.endpoint, mode)

    @unittest.skipUnless(os.name == 'posix', 'macOS and Linux only')
    def test_an_endpoint_file_others_can_open_is_not_used(self):
        self.write_endpoint(mode=0o644)
        self.assertIsNone(adapter.read_endpoint(self.endpoint))
        out = json.loads(adapter.answer('{"jsonrpc":"2.0","id":1,"method":"ping"}', self.endpoint))
        self.assertIn('Local control', out['error']['message'])
        self.assertEqual(Launcher.seen, [])

    def test_a_request_goes_to_the_launcher_with_the_token(self):
        self.write_endpoint()
        out = adapter.answer('{"jsonrpc":"2.0","id":1,"method":"tools/list"}', self.endpoint)
        self.assertEqual(json.loads(out), {'jsonrpc': '2.0', 'id': 1, 'result': {'method': 'tools/list'}})
        self.assertEqual(Launcher.seen[0][:2], ('/mcp', 'Bearer ' + TOKEN))

    def test_each_answer_is_one_line(self):
        self.write_endpoint()
        out = adapter.answer('{"jsonrpc":"2.0","id":1,"method":"ping"}', self.endpoint)
        self.assertNotIn('\n', out)

    def test_a_notification_gets_no_line(self):
        self.write_endpoint()
        self.assertIsNone(adapter.answer('{"jsonrpc":"2.0","method":"notifications/initialized"}', self.endpoint))

    def test_no_endpoint_file_says_how_to_turn_it_on(self):
        out = json.loads(adapter.answer('{"jsonrpc":"2.0","id":7,"method":"ping"}', self.endpoint))
        self.assertEqual(out['id'], 7)
        self.assertIn('Local control', out['error']['message'])
        self.assertEqual(Launcher.seen, [])

    def test_the_token_is_never_sent_off_this_computer(self):
        self.write_endpoint(url='http://192.0.2.1:9/mcp')
        out = json.loads(adapter.answer('{"jsonrpc":"2.0","id":1,"method":"ping"}', self.endpoint))
        self.assertIn('error', out)
        self.assertIsNone(adapter.read_endpoint(self.endpoint))

    def test_proxy_settings_are_ignored(self):
        self.write_endpoint()
        saved = {k: os.environ.get(k) for k in ('HTTP_PROXY', 'http_proxy', 'NO_PROXY', 'no_proxy')}
        os.environ.update({'HTTP_PROXY': 'http://127.0.0.1:1', 'http_proxy': 'http://127.0.0.1:1', 'NO_PROXY': '', 'no_proxy': ''})
        try:
            out = json.loads(adapter.answer('{"jsonrpc":"2.0","id":1,"method":"ping"}', self.endpoint))
        finally:
            for k, v in saved.items():
                if v is None:
                    os.environ.pop(k, None)
                else:
                    os.environ[k] = v
        self.assertEqual(out['result'], {'method': 'ping'})

    def test_a_stale_token_is_reported_without_showing_it(self):
        self.write_endpoint(token='old-token')
        out = adapter.answer('{"jsonrpc":"2.0","id":1,"method":"ping"}', self.endpoint)
        self.assertIn('refused', json.loads(out)['error']['message'])
        self.assertNotIn('old-token', out)

    def test_bad_json_is_a_parse_error(self):
        self.assertEqual(json.loads(adapter.answer('{nope', self.endpoint))['error']['code'], -32700)

    def test_the_usual_data_folder_ends_in_the_app_id(self):
        path = adapter.default_endpoint()
        self.assertTrue(path.endswith(os.path.join('com.recomplauncher.app', 'control-api', 'endpoint.json')))


if __name__ == '__main__':
    unittest.main()
