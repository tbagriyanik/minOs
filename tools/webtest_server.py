#!/usr/bin/env python3
"""minOs webtest sayfasi icin yerel HTTP + HTTPS test sunucusu.

Kullanim (QEMU user-net altinda misafir 10.0.2.2 uzerinden erisir):

    python tools/webtest_server.py [--http-port 8001] [--https-port 8443]

Ardindan tarayicida:

    http://10.0.2.2:8001/      -> index.html + style.css + test.png
    https://10.0.2.2:8443/     -> ayni sayfa, TLS uzerinden

HTTPS icin `tools/h3_test_cert.pem` / `tools/h3_test_key.pem` kullanilir
(oz-imzali; SAN: localhost, 10.0.2.2, 127.0.0.1). Sertifika yoksa
`cryptography` paketiyle otomatik uretilir.

Self-signed sertifika minOs guven deposunda olmadigindan, guest'i
asagidaki cmdline token'i ile baslatin:

    livetest_insecure

Uclar:
  /            -> tools/webtest/index.html
  /style.css   -> harici CSS
  /test.png    -> test gorseli
  /api/ping    -> text/plain "pong"
  /api/info    -> JSON benzeri bilgi (sema, sunucu)
"""

import argparse
import functools
import http.server
import os
import ssl
import sys
import threading

HERE = os.path.dirname(os.path.abspath(__file__))
WEBROOT = os.path.join(HERE, "webtest")
CERT_PATH = os.path.join(HERE, "h3_test_cert.pem")
KEY_PATH = os.path.join(HERE, "h3_test_key.pem")

MIME = {
    ".html": "text/html; charset=utf-8",
    ".htm": "text/html; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".gif": "image/gif",
    ".svg": "image/svg+xml",
    ".json": "application/json",
    ".txt": "text/plain; charset=utf-8",
}


def ensure_cert() -> None:
    """Test sertifikasi yoksa oz-imzali uret (h3_test_server ile ayni)."""
    if os.path.exists(CERT_PATH) and os.path.exists(KEY_PATH):
        return
    try:
        from cryptography import x509
        from cryptography.hazmat.primitives import hashes, serialization
        from cryptography.hazmat.primitives.asymmetric import rsa
        from cryptography.x509.oid import NameOID
    except ImportError:
        sys.exit(
            "[hata] cryptography paketi gerekli: python -m pip install cryptography"
        )

    import datetime
    import ipaddress

    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "minos-local-webtest")])
    now = datetime.datetime.now(datetime.timezone.utc)
    cert = (
        x509.CertificateBuilder()
        .subject_name(name)
        .issuer_name(name)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - datetime.timedelta(days=1))
        .not_valid_after(now + datetime.timedelta(days=3650))
        .add_extension(
            x509.SubjectAlternativeName(
                [
                    x509.DNSName("localhost"),
                    x509.IPAddress(ipaddress.ip_address("10.0.2.2")),
                    x509.IPAddress(ipaddress.ip_address("127.0.0.1")),
                ]
            ),
            critical=False,
        )
        .add_extension(x509.BasicConstraints(ca=True, path_length=None), critical=True)
        .sign(key, hashes.SHA256())
    )
    with open(KEY_PATH, "wb") as f:
        f.write(
            key.private_bytes(
                serialization.Encoding.PEM,
                serialization.PrivateFormat.PKCS8,
                serialization.NoEncryption(),
            )
        )
    with open(CERT_PATH, "wb") as f:
        f.write(cert.public_bytes(serialization.Encoding.PEM))
    print(f"[webtest] sertifika uretildi: {CERT_PATH}")


def make_handler():
    class Handler(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=WEBROOT, **kwargs)

        def do_GET(self):  # noqa: N802
            if self.path.startswith("/api/ping"):
                self._send(b"pong\n", "text/plain; charset=utf-8")
            elif self.path.startswith("/api/info"):
                body = (
                    '{"server":"minos-webtest","tls":"see-port",'
                    '"note":"HTTP/HTTPS test endpoint"}\n'
                ).encode()
                self._send(body, "application/json")
            elif self.path in ("/", "/index.html"):
                self._send_file("index.html")
            else:
                super().do_GET()

        def _send(self, body: bytes, ctype: str):
            self.send_response(200)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def _send_file(self, name: str):
            path = os.path.join(WEBROOT, name)
            with open(path, "rb") as f:
                body = f.read()
            ext = os.path.splitext(name)[1].lower()
            self._send(body, MIME.get(ext, "application/octet-stream"))

        def guess_type(self, path):  # SimpleHTTPRequestHandler icin MIME
            ext = os.path.splitext(path)[1].lower()
            return MIME.get(ext, "application/octet-stream")

        def log_message(self, fmt, *args):  # noqa: A003
            sys.stderr.write("[webtest] " + fmt % args + "\n")

    return Handler


def serve(port: int, use_tls: bool) -> None:
    handler = make_handler()
    scheme = "https" if use_tls else "http"
    try:
        server = http.server.ThreadingHTTPServer(("0.0.0.0", port), handler)
    except OSError as e:
        print(f"[webtest] HATA: {scheme}://{port} baglanamadi: {e}", file=sys.stderr)
        return
    if use_tls:
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.load_cert_chain(CERT_PATH, KEY_PATH)
        server.socket = ctx.wrap_socket(server.socket, server_side=True)
    print(f"[webtest] {scheme}://0.0.0.0:{port}/ dinleniyor (misafir: {scheme}://10.0.2.2:{port}/)")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--http-port", type=int, default=8001)
    ap.add_argument("--https-port", type=int, default=8443)
    ap.add_argument("--http-only", action="store_true")
    args = ap.parse_args()

    threads = [
        threading.Thread(target=serve, args=(args.http_port, False), daemon=True),
    ]
    if not args.http_only:
        ensure_cert()
        threads.append(
            threading.Thread(target=serve, args=(args.https_port, True), daemon=True)
        )
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
