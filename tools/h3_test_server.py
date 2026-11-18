#!/usr/bin/env python3
"""minOs canli HTTP/3 (QUIC) test sunucusu — aioquic tabanli.

Kullanim (QEMU user-net altinda misafir 10.0.2.2 uzerinden erisir):

    python -m pip install aioquic
    python tools/h3_test_server.py [--port 4433]

Ardindan ISO'yu su cmdline ile acin:

    livetest=http://10.0.2.2:8000/ livetest_h3=https://10.0.2.2:4433/ livetest_insecure

`livetest_insecure` yerel oz-imzali sertifikayi kabul ettirmek icin
gerekir; gercek bir H3 sunucusuna karsi test ederken verilmez.

Uclar:
  /          -> 200 text/plain govde
  /trailers  -> 200 + DATA sonrasi trailer HEADERS (orn. x-checksum)
  /big       -> ~64 KiB govde (stream/akis-kontrol pencerelerini zorlar)
  /echo      -> istek govdesini oldugu gibi dondurur (DATA govdesi)

Sertifika ilk calistirmada `tools/h3_test_cert.pem` /
`tools/h3_test_key.pem` olarak uretilir (oz-imzali, SAN: localhost,
10.0.2.2).
"""

import argparse
import asyncio
import datetime
import ipaddress
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
CERT_PATH = os.path.join(HERE, "h3_test_cert.pem")
KEY_PATH = os.path.join(HERE, "h3_test_key.pem")

PAYLOAD = (
    "minOs HTTP/3 live test payload. "
    "The quick brown fox jumps over the lazy dog. " * 16
).encode()

BIG_PAYLOAD = (PAYLOAD * 700)[:65536]


def ensure_cert() -> None:
    """Test sertifikasi yoksa oz-imzali uret (cryptography ile)."""
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

    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "minos-local-h3-test")])
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
    print(f"[h3-test] sertifika uretildi: {CERT_PATH}")


def make_handler():
    from aioquic.asyncio import QuicConnectionProtocol
    from aioquic.h3.connection import H3Connection
    from aioquic.h3.events import DataReceived, HeadersReceived
    from aioquic.quic.events import QuicEvent

    class HttpRequestHandler(QuicConnectionProtocol):
        def __init__(self, *args, **kwargs):
            super().__init__(*args, **kwargs)
            self._http = None
            self._bodies = {}  # stream_id -> bytearray (echo icin)

        def quic_event_received(self, event: QuicEvent) -> None:
            if self._http is None:
                self._http = H3Connection(self._quic)
            for http_event in self._http.handle_event(event):
                self.http_event_received(http_event)

        def _path_of(self, headers) -> str:
            for name, value in headers:
                if name == b":path":
                    return value.decode("ascii", "replace")
            return "/"

        def http_event_received(self, event) -> None:
            if isinstance(event, HeadersReceived):
                stream_id = event.stream_id
                path = self._path_of(event.headers)
                if path.startswith("/echo"):
                    # Govdeyi bekle; DataReceived + stream sonu yaniti tetikler.
                    self._bodies.setdefault(stream_id, bytearray())
                    if event.stream_ended:
                        self._respond_echo(stream_id)
                elif path.startswith("/trailers"):
                    self._respond_trailers(stream_id)
                elif path.startswith("/big"):
                    self._respond(stream_id, BIG_PAYLOAD)
                else:
                    self._respond(stream_id, PAYLOAD)
            elif isinstance(event, DataReceived):
                stream_id = event.stream_id
                if stream_id in self._bodies:
                    self._bodies[stream_id].extend(event.data)
                    if event.stream_ended:
                        self._respond_echo(stream_id)

        def _respond(self, stream_id: int, body: bytes) -> None:
            self._http.send_headers(
                stream_id=stream_id,
                headers=[
                    (b":status", b"200"),
                    (b"content-type", b"text/plain; charset=utf-8"),
                    (b"content-length", str(len(body)).encode()),
                    (b"x-h3-test", b"minos"),
                ],
                end_stream=False,
            )
            self._http.send_data(stream_id=stream_id, data=body, end_stream=True)

        def _respond_trailers(self, stream_id: int) -> None:
            body = PAYLOAD + b" [trailers]"
            self._http.send_headers(
                stream_id=stream_id,
                headers=[
                    (b":status", b"200"),
                    (b"content-type", b"text/plain; charset=utf-8"),
                    (b"content-length", str(len(body)).encode()),
                ],
                end_stream=False,
            )
            self._http.send_data(stream_id=stream_id, data=body, end_stream=False)
            # Trailer HEADERS: pseudo-header ve content-length icermez.
            self._http.send_headers(
                stream_id=stream_id,
                headers=[
                    (b"x-checksum", b"ok"),
                    (b"x-trailer-test", b"1"),
                ],
                end_stream=True,
            )

        def _respond_echo(self, stream_id: int) -> None:
            body = bytes(self._bodies.pop(stream_id, b""))
            self._http.send_headers(
                stream_id=stream_id,
                headers=[
                    (b":status", b"200"),
                    (b"content-type", b"application/octet-stream"),
                    (b"content-length", str(len(body)).encode()),
                ],
                end_stream=False,
            )
            self._http.send_data(stream_id=stream_id, data=body, end_stream=True)

    return HttpRequestHandler


async def main() -> None:
    from aioquic.asyncio import serve
    from aioquic.quic.configuration import QuicConfiguration

    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=4433)
    args = parser.parse_args()

    ensure_cert()

    config = QuicConfiguration(is_client=False, alpn_protocols=["h3"])
    config.load_cert_chain(CERT_PATH, KEY_PATH)

    server = await serve(
        "0.0.0.0",
        args.port,
        configuration=config,
        create_protocol=make_handler(),
    )
    print(f"[h3-test] HTTP/3 sunucusu UDP {args.port} uzerinde dinliyor.")
    print("[h3-test] QEMU misafiri icin: https://10.0.2.2:%d/" % args.port)
    print("[h3-test] cmdline: livetest_h3=https://10.0.2.2:%d/ livetest_insecure" % args.port)
    await asyncio.Future()  # sonsuza kadar calis


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        pass
