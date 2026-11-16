#!/usr/bin/env python3
"""minOs canli ag testleri icin yerel HTTP test sunucusu.

Kullanim (QEMU user-net altinda misafir 10.0.2.2 uzerinden erisir):

    python tools/live_net_test_server.py [--port 8000]

Ardindan ISO'yu `livetest=http://10.0.2.2:8000/` cmdline'i ile acin.

Uclar:
  /        -> 200 text/plain govde
  /br      -> Content-Encoding: br ile sikistirilmis govde (brotli
              paketi kuruluysa; degilse identity duser)
  /h2hint  -> ALPN/h2 bilgisi iceren govde
"""

import argparse
import http.server
import sys

PAYLOAD = (
    "minOs live network test payload. "
    "The quick brown fox jumps over the lazy dog. " * 8
).encode()


def make_handler():
    class Handler(http.server.BaseHTTPRequestHandler):
        def _send(self, body: bytes, extra_headers: dict | None = None):
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            for k, v in (extra_headers or {}).items():
                self.send_header(k, v)
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):  # noqa: N802
            if self.path.startswith("/br"):
                try:
                    import brotli  # type: ignore

                    body = brotli.compress(PAYLOAD)
                    self._send(body, {"Content-Encoding": "br"})
                except ImportError:
                    self._send(PAYLOAD)
            elif self.path.startswith("/h2hint"):
                proto = self.headers.get("X-Forwarded-Proto", "http")
                self._send(f"alpn-probe path={self.path}\n".encode())
            else:
                # Istemci `Accept-Encoding: br` gonderdiyse (minOs boyle
                # gonderir) Brotli ile sikistirilmis govde dondur; boylece
                # tek GET istegi HTTP + Brotli yolunu dogrular.
                accept = self.headers.get("Accept-Encoding", "")
                if "br" in accept:
                    try:
                        import brotli  # type: ignore

                        self._send(brotli.compress(PAYLOAD), {"Content-Encoding": "br"})
                        return
                    except ImportError:
                        pass
                self._send(PAYLOAD)

        def log_message(self, fmt, *args):  # noqa: A003
            sys.stderr.write("[live-net-server] " + fmt % args + "\n")

    return Handler


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8000)
    ap.add_argument("--host", default="0.0.0.0")
    args = ap.parse_args()
    server = http.server.ThreadingHTTPServer((args.host, args.port), make_handler())
    print(f"[live-net-server] http://{args.host}:{args.port}/ dinleniyor")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
