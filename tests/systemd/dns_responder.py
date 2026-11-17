"""Minimal DNS server for tests/systemd: answers one name with one IPv4 address.

Usage: python3 -I dns_responder.py <listen address> <name> <IPv4>

Every query for <name> of type A gets the address; any other query gets an
empty answer (NOERROR, no records) for known names and NXDOMAIN for unknown
ones. Enough for systemd-resolved to forward the test gateway's name to it,
so that charon resolves the gateway the way it does on a user's computer.
"""

import socket
import struct
import sys


def question(packet: bytes) -> tuple[str, int, int]:
    """Name, type and end offset of the first question."""
    labels, i = [], 12
    while packet[i]:
        size = packet[i]
        labels.append(packet[i + 1 : i + 1 + size].decode("ascii").lower())
        i += 1 + size
    qtype = struct.unpack("!H", packet[i + 1 : i + 3])[0]
    return ".".join(labels), qtype, i + 5


def main() -> None:
    listen, name, address = sys.argv[1], sys.argv[2].lower().rstrip("."), sys.argv[3]
    rdata = socket.inet_aton(address)
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind((listen, 53))
    while True:
        packet, peer = sock.recvfrom(4096)
        try:
            qname, qtype, end = question(packet)
        except (IndexError, UnicodeDecodeError, struct.error):
            continue
        known = qname == name
        answers = b""
        if known and qtype == 1:
            answers = struct.pack("!HHHIH", 0xC00C, 1, 1, 60, 4) + rdata
        flags = 0x8180 if known else 0x8183  # response, RD, RA; NXDOMAIN if unknown
        header = packet[:2] + struct.pack("!HHHHH", flags, 1, 1 if answers else 0, 0, 0)
        sock.sendto(header + packet[12:end] + answers, peer)


if __name__ == "__main__":
    main()
