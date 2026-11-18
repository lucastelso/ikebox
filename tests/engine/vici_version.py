#!/usr/bin/env python3
"""Pede `version` ao charon pelo socket VICI e imprime a resposta.

Usado pelo e2e.sh para provar duas coisas:
  1. o usuário comum (não root) consegue abrir o socket (dono/permissão);
  2. o protocolo VICI funciona pelo bind mount, como o app vai usar.

Uso: vici_version.py /caminho/charon.vici
Só biblioteca padrão; rode com `python3 -I`.
"""

import socket
import struct
import sys

# VICI protocol packet and element types.
CMD_REQUEST, CMD_RESPONSE, CMD_UNKNOWN = 0, 1, 2
SECTION_START, SECTION_END, KEY_VALUE = 1, 2, 3
LIST_START, LIST_ITEM, LIST_END = 4, 5, 6


def recv_exact(sock: socket.socket, n: int) -> bytes:
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("conexão VICI fechada no meio do pacote")
        buf += chunk
    return buf


def parse_message(data: bytes) -> dict[str, str]:
    """Extrai os pares chave/valor de primeiro nível (suficiente para `version`)."""
    out: dict[str, str] = {}
    i = 0
    while i < len(data):
        kind = data[i]
        i += 1
        if kind == KEY_VALUE:
            klen = data[i]
            key = data[i + 1 : i + 1 + klen].decode()
            i += 1 + klen
            (vlen,) = struct.unpack(">H", data[i : i + 2])
            out[key] = data[i + 2 : i + 2 + vlen].decode(errors="replace")
            i += 2 + vlen
        elif kind in (SECTION_START, LIST_START):
            i += 1 + data[i]
        elif kind == LIST_ITEM:
            (vlen,) = struct.unpack(">H", data[i : i + 2])
            i += 2 + vlen
        elif kind in (SECTION_END, LIST_END):
            pass
        else:
            raise ValueError(f"elemento VICI desconhecido: {kind}")
    return out


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
        sock.settimeout(5)
        sock.connect(sys.argv[1])
        name = b"version"
        payload = bytes([CMD_REQUEST, len(name)]) + name
        sock.sendall(struct.pack(">I", len(payload)) + payload)
        (length,) = struct.unpack(">I", recv_exact(sock, 4))
        packet = recv_exact(sock, length)
    if packet[0] != CMD_RESPONSE:
        print(f"resposta VICI inesperada: tipo {packet[0]}", file=sys.stderr)
        return 1
    for key, value in parse_message(packet[1:]).items():
        print(f"{key}={value}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
