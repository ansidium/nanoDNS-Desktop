"""Exercise the release EXE over real local UDP sockets; never changes Windows settings."""
import json
from pathlib import Path
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time


def query(name, kind=1):
    labels = b"".join(bytes([len(part)]) + part.encode("ascii") for part in name.split("."))
    return struct.pack("!6H", 0x1234, 0x100, 1, 0, 0, 0) + labels + b"\0" + struct.pack("!2H", kind, 1)


def verify(exe):
    upstream = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    upstream.bind(("127.0.0.1", 0))
    upstream.settimeout(0.1)
    stop = threading.Event()

    def resolver():
        while not stop.is_set():
            try:
                request, peer = upstream.recvfrom(4096)
            except socket.timeout:
                continue
            answer = request[:2] + struct.pack("!5H", 0x8180, 1, 1, 0, 0) + request[12:]
            answer += b"\xc0\x0c" + struct.pack("!2HIH", 1, 1, 60, 4) + bytes([203, 0, 113, 7])
            upstream.sendto(answer, peer)

    worker = threading.Thread(target=resolver)
    worker.start()
    with tempfile.TemporaryDirectory(prefix="nanodns-smoke-") as directory:
        profile = Path(directory)
        reservation = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        reservation.bind(("127.0.0.1", 0))
        address = reservation.getsockname()
        reservation.close()
        settings = {"bind": "127.0.0.1", "port": address[1], "upstreams": ["%s:%d" % upstream.getsockname()]}
        (profile / "settings.json").write_text(json.dumps(settings), encoding="utf-8")
        process = subprocess.Popen([str(exe), "--headless", "--data-dir", str(profile), "--run-for-ms", "2500"], creationflags=subprocess.CREATE_NO_WINDOW)
        client = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        client.settimeout(0.15)
        try:
            for attempt in range(20):
                try:
                    client.sendto(query("UPDATE.PlayStation.NET"), address)
                    blocked, _ = client.recvfrom(4096)
                    break
                except (socket.timeout, ConnectionResetError):
                    time.sleep(0.05)
            else:
                raise AssertionError("EXE listener did not become ready")
            assert blocked[-4:] == bytes(4)
            assert struct.unpack("!6H", blocked[:12]) == (0x1234, 0x8180, 1, 1, 0, 0)
            assert b"UPDATE" in blocked, "DNS question case must survive local response"
            client.sendto(query("update.playstation.net", 28), address)
            nodata, _ = client.recvfrom(4096)
            assert struct.unpack("!H", nodata[6:8])[0] == 0
            client.sendto(query("update.playstation.net", 255), address)
            any_answer, _ = client.recvfrom(4096)
            assert any_answer[-4:] == bytes(4)
            for domain in ["feature.api.playstation.com", "post.net.playstation.net", "allowed.example"]:
                client.sendto(query(domain), address)
                forwarded, _ = client.recvfrom(4096)
                assert forwarded[-4:] == bytes([203, 0, 113, 7]), domain
            assert process.wait(timeout=8) == 0
            stats = json.loads((profile / "smoke-stats.json").read_text())
            assert stats["blocked"] >= 3 and stats["forwarded"] == 3 and stats["failed"] == 0, stats
            reuse = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            reuse.bind(address)
            reuse.close()
            report = {"release_exe_udp": "PASS", "override_A_ANY": "PASS", "AAAA_NODATA": "PASS", "exceptions_and_forwarding": "PASS", "case_preserved": "PASS", "stop_releases_port": "PASS", "stats": stats}
        finally:
            client.close()
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
            stop.set()
            worker.join(timeout=2)
            upstream.close()
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    verify(Path(sys.argv[1]).resolve())
