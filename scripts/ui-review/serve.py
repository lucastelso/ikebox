"""Static server for the UI review: /__hold answers only after /__release.

The page holds its load event on an image at /__hold; the scenario script
calls /__release when the screen is ready, and Firefox takes the screenshot
on load. A timeout releases it anyway.
"""

import http.server
import sys
import threading

released = threading.Event()


class Handler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):  # quiet
        pass

    def do_GET(self):  # noqa: N802
        if self.path.startswith("/__release"):
            released.set()
            self.send_response(204)
            self.end_headers()
            return
        if self.path.startswith("/__hold"):
            released.wait(timeout=8)
            released.clear()
            self.send_response(200)
            self.send_header("Content-Type", "image/gif")
            self.end_headers()
            self.wfile.write(b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;")
            return
        super().do_GET()


if __name__ == "__main__":
    directory, port = sys.argv[1], int(sys.argv[2])
    handler = lambda *a, **k: Handler(*a, directory=directory, **k)  # noqa: E731
    http.server.ThreadingHTTPServer(("127.0.0.1", port), handler).serve_forever()
