"""Manda um pedido (JSON) ao ipsec-client-helper e imprime a resposta.

Uso: python3 -I helper_client.py '<json>' [--hold SEGUNDOS]
Com --hold, só abre a conexão e fica parado (cliente lento/malicioso).
"""

import json
import socket
import sys
import time

SOCKET = "/run/ipsec-client-helper.sock"


def main() -> None:
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(SOCKET)
    if len(sys.argv) > 2 and sys.argv[2] == "--hold":
        time.sleep(float(sys.argv[3]))
        return
    request = json.loads(sys.argv[1])
    sock.sendall((json.dumps(request) + "\n").encode())
    print(sock.makefile().readline().strip())


if __name__ == "__main__":
    main()
