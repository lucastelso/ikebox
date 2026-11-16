"""Sends a request (JSON) to ipsec-client-helper and prints the reply.

Usage: python3 -I helper_client.py '<json>' [--hold SECONDS | --drip SECONDS]
With --hold, it only opens the connection and stays idle (slow/malicious client).
With --drip, it sends one byte per second, for that long or until the helper
closes the connection.
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
    if len(sys.argv) > 2 and sys.argv[2] == "--drip":
        try:
            for _ in range(int(float(sys.argv[3]))):
                sock.sendall(b" ")
                time.sleep(1)
        except OSError:
            pass
        return
    request = json.loads(sys.argv[1])
    sock.sendall((json.dumps(request) + "\n").encode())
    print(sock.makefile().readline().strip())


if __name__ == "__main__":
    main()
