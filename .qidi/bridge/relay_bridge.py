# -*- coding: utf-8 -*-
"""Local HTTP->HTTPS bridge: qidicode -> cloud relay (111.231.166.204).

Why: qidi.exe uses rustls with the embedded webpki-roots (Mozilla CA bundle)
and ignores the Windows certificate stores entirely, so the relay's
self-signed cert can never be trusted in-process. This bridge listens on
plain HTTP 127.0.0.1:8790 (qidi config points here) and forwards everything
to https://111.231.166.204 with an SSL context that trusts .qidi/qidi-relay.crt.

Streaming pass-through: response body is relayed chunk-by-chunk so SSE
streams reach qidi as they arrive.

Usage: py -3 relay_bridge.py [port]
Auto-started by ensure_bridge.ps1 from the .bat launchers.
"""
import http.client
import http.server
import io
import os
import socketserver
import ssl
import sys
import threading
import time
import urllib.parse

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding="utf-8")

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CERT = os.path.join(ROOT, ".qidi", "qidi-relay.crt")
UPSTREAM_HOST = "111.231.166.204"
UPSTREAM_PORT = 443
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8790

ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
ctx.load_verify_locations(CERT)
ctx.check_hostname = True

HOP_HEADERS = {"connection", "transfer-encoding", "keep-alive", "proxy-authenticate",
               "proxy-authorization", "te", "trailers", "upgrade"}
FWD_REQ_HEADERS = ("authorization", "content-type", "accept", "user-agent",
                   "x-api-key", "anthropic-version", "accept-encoding", "x-request-id")

_loglock = threading.Lock()


def log(msg):
    with _loglock:
        print("[%s] %s" % (time.strftime("%H:%M:%S"), msg), flush=True)


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    timeout = 600

    def _read_body(self):
        if self.headers.get("Content-Length"):
            return self.rfile.read(int(self.headers["Content-Length"]))
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            body = b""
            while True:
                line = self.rfile.readline().strip()
                sz = int(line.split(b";")[0], 16)
                if sz == 0:
                    self.rfile.readline()
                    return body
                body += self.rfile.read(sz)
                self.rfile.readline()
        return None

    def _proxy(self, method):
        body = self._read_body()
        headers = {}
        for h in FWD_REQ_HEADERS:
            if self.headers.get(h):
                headers[h] = self.headers[h]

        conn = http.client.HTTPSConnection(UPSTREAM_HOST, UPSTREAM_PORT,
                                           context=ctx, timeout=600)
        sent_headers = False
        t0 = time.time()
        try:
            conn.request(method, self.path, body=body, headers=headers)
            resp = conn.getresponse()

            self.send_response(resp.status)
            sent_headers = True
            clen = resp.getheader("Content-Length")
            for k, v in resp.getheaders():
                lk = k.lower()
                if lk in HOP_HEADERS or lk == "content-length":
                    continue
                self.send_header(k, v)
            if clen is not None:
                self.send_header("Content-Length", clen)
            else:
                # no length (SSE/chunked): stream until EOF, then close
                self.send_header("Connection", "close")
                self.close_connection = True
            self.end_headers()

            total = 0
            while True:
                chunk = resp.read(16384)
                if not chunk:
                    break
                self.wfile.write(chunk)
                total += len(chunk)
            log("%s %s -> %d (%dB, %.1fs)" % (method, self.path, resp.status,
                                              total, time.time() - t0))
        except (BrokenPipeError, ConnectionResetError):
            log("%s %s -> client disconnected" % (method, self.path))
            self.close_connection = True
        except Exception as e:
            log("%s %s -> ERROR %s" % (method, self.path, e))
            if not sent_headers:
                try:
                    msg = ('{"error": {"message": "bridge upstream error: %s", '
                           '"type": "bridge_error"}}' % str(e).replace('"', "'")).encode()
                    self.send_response(502)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(msg)))
                    self.end_headers()
                    self.wfile.write(msg)
                except OSError:
                    pass
            self.close_connection = True
        finally:
            conn.close()

    def do_GET(self):
        self._proxy("GET")

    def do_POST(self):
        self._proxy("POST")

    def log_message(self, *a):
        pass


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


if __name__ == "__main__":
    httpd = Server(("127.0.0.1", PORT), Handler)
    log("bridge listening on http://127.0.0.1:%d -> https://%s" % (PORT, UPSTREAM_HOST))
    log("trust cert: %s" % CERT)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
