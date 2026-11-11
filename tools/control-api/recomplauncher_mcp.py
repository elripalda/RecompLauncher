#!/usr/bin/env python3
"""MCP over stdio for RecompLauncher's local control API.

MCP clients that start a program and talk to it over stdin/stdout run this. Each JSON-RPC line it
reads goes to the running launcher's POST /mcp, and the answer comes back as one line. The
launcher decides everything: this only finds it and carries the messages, so it can't reach
anything the launcher doesn't offer (the launcher_status and play_game tools).

The launcher's address and token are read from its endpoint file each time, so a launcher that
was restarted is found again. Turn the API on in RecompLauncher: Settings > General > Local
control. Python 3.9 or newer, no packages needed.

  python3 recomplauncher_mcp.py [--endpoint PATH]

PATH (or RECOMPLAUNCHER_ENDPOINT) is only needed for a portable or Flatpak launcher, whose data
folder isn't the usual one.
"""
import argparse
import json
import os
import stat
import sys
import urllib.error
import urllib.request

APP_ID = 'com.recomplauncher.app'
TIMEOUT_S = 60
NOT_RUNNING = 'RecompLauncher isn\'t running with its local control turned on (Settings > General > Local control).'


def default_endpoint():
    """The endpoint file in the launcher's usual data folder on this system."""
    home = os.path.expanduser('~')
    if sys.platform == 'win32':
        base = os.environ.get('APPDATA') or os.path.join(home, 'AppData', 'Roaming')
    elif sys.platform == 'darwin':
        base = os.path.join(home, 'Library', 'Application Support')
    else:
        base = os.environ.get('XDG_DATA_HOME') or os.path.join(home, '.local', 'share')
    return os.path.join(base, APP_ID, 'control-api', 'endpoint.json')


def open_endpoint(path):
    """The endpoint file, opened to read. On macOS and Linux it must be a plain file of this
    user's that nobody else can open, as the launcher writes it: another user who could change a
    folder on the way could put their own there, but not one owned by this user."""
    if os.name != 'posix':
        return open(path, 'r', encoding='utf-8')
    # O_NONBLOCK: a pipe put in its place can't keep this waiting.
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | getattr(os, 'O_NOFOLLOW', 0))
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_uid != os.geteuid() or st.st_mode & 0o077:
            raise OSError('the endpoint file isn\'t private to this user')
        return os.fdopen(fd, 'r', encoding='utf-8')
    except BaseException:
        os.close(fd)
        raise


def read_endpoint(path):
    """(url, token) from the endpoint file, or None when there's no usable one."""
    try:
        with open_endpoint(path) as f:
            info = json.load(f)
    except (OSError, ValueError):
        return None
    url = info.get('mcp_url') if isinstance(info, dict) else None
    token = info.get('token') if isinstance(info, dict) else None
    # The token only ever goes to the launcher on this computer.
    if not isinstance(url, str) or not url.startswith('http://127.0.0.1:'):
        return None
    if not isinstance(token, str) or not token:
        return None
    return url, token


# No proxies: HTTP_PROXY and friends would otherwise see the token.
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def post(url, token, body):
    """(status, text) of one POST to the launcher."""
    req = urllib.request.Request(url, data=body, method='POST', headers={
        'Authorization': 'Bearer ' + token,
        'Content-Type': 'application/json',
        'Accept': 'application/json',
    })
    try:
        with OPENER.open(req, timeout=TIMEOUT_S) as r:
            return r.status, r.read().decode('utf-8', 'replace')
    except urllib.error.HTTPError as e:
        try:
            return e.code, e.read().decode('utf-8', 'replace')
        finally:
            e.close()


def error_line(msg_id, message):
    return json.dumps({'jsonrpc': '2.0', 'id': msg_id, 'error': {'code': -32000, 'message': message}})


def answer(line, endpoint_path):
    """The line to print for one message, or None (notifications and responses get none)."""
    try:
        msg = json.loads(line)
    except ValueError:
        return json.dumps({'jsonrpc': '2.0', 'id': None, 'error': {'code': -32700, 'message': 'Parse error'}})
    msg_id = msg.get('id') if isinstance(msg, dict) else None
    wants_answer = isinstance(msg, dict) and 'id' in msg and 'method' in msg
    found = read_endpoint(endpoint_path)
    if found is None:
        return error_line(msg_id, NOT_RUNNING) if wants_answer else None
    url, token = found
    try:
        status, text = post(url, token, line.encode('utf-8'))
    except (OSError, urllib.error.URLError):
        return error_line(msg_id, NOT_RUNNING) if wants_answer else None
    if status == 202 or not text.strip():
        return None
    if status != 200:
        if not wants_answer:
            return None
        try:
            why = json.loads(text)['error']['message']
        except (ValueError, KeyError, TypeError):
            why = 'HTTP %d' % status
        return error_line(msg_id, 'RecompLauncher refused the request: ' + str(why))
    # One line per answer, whatever spacing the launcher used.
    return json.dumps(json.loads(text), separators=(',', ':'))


def main(argv=None):
    p = argparse.ArgumentParser(description='MCP over stdio for RecompLauncher\'s local control API.')
    p.add_argument('--endpoint', default=os.environ.get('RECOMPLAUNCHER_ENDPOINT') or default_endpoint(), help='the launcher\'s control-api/endpoint.json')
    args = p.parse_args(argv)
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        out = answer(line, args.endpoint)
        if out is not None:
            sys.stdout.write(out + '\n')
            sys.stdout.flush()
    return 0


if __name__ == '__main__':
    sys.exit(main())
