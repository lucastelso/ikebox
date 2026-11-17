"""Writes a schema 1 profile for tests/modes (test gateways only, RFC 5737).

Usage: make_profile.py OUT key=value...

Keys: id, gateway, version (1|2), aggressive (yes|no), encap (yes|no),
method (psk|psk-xauth), local_id, remote_id, ike (proposals, comma
separated), esp (proposals), remote_ts (comma separated), vip (yes|no).
"""

import json
import os
import sys


def main() -> None:
    out = sys.argv[1]
    args = dict(arg.split("=", 1) for arg in sys.argv[2:])
    yes = {"yes": True, "no": False}
    auth = {"method": args["method"]}
    for key in ("local_id", "remote_id"):
        if args.get(key):
            auth[key] = args[key]
    profile = {
        "schema_version": 1,
        "id": args["id"],
        "name": f"Mode test {args['id']}",
        "gateway": args["gateway"],
        "ike": {
            "version": int(args["version"]),
            "aggressive": yes[args.get("aggressive", "no")],
            "encap": yes[args.get("encap", "no")],
            "proposals": args["ike"].split(","),
        },
        "auth": auth,
        "child": {
            "name": "rede",
            "esp_proposals": args["esp"].split(","),
            "remote_ts": args["remote_ts"].split(","),
        },
        "network": {
            "request_vip": yes[args.get("vip", "yes")],
            "dns": {"use_received": False},
        },
    }
    with open(out, "w", encoding="utf-8") as f:
        json.dump(profile, f, indent=2)
    os.chmod(out, 0o644)


if __name__ == "__main__":
    main()
