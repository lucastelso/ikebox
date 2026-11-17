"""Writes a schema 1 profile for tests/modes (test gateways only, RFC 5737).

Usage: make_profile.py OUT key=value...

Keys: id, gateway, version (1|2), aggressive (yes|no), encap (yes|no),
method (psk|psk-xauth|eap|cert|cert-xauth), local_id, remote_id, ca (CA
certificate files, comma separated: eap and cert only), ike (proposals, comma
separated), esp (proposals), remote_ts (comma separated), vip (yes|no),
schema (1|2, default 1: the file format), otp (yes|no: the XAuth password
is a one-time code; schema 2 only).
"""

import json
import os
import sys


def main() -> None:
    out = sys.argv[1]
    args = dict(arg.split("=", 1) for arg in sys.argv[2:])
    yes = {"yes": True, "no": False}
    schema = int(args.get("schema", "1"))
    if args["method"] in ("eap", "cert", "cert-xauth"):
        schema = 2
        method = "eap-mschapv2" if args["method"] == "eap" else "certificate"
        first = {"method": method}
        if args.get("local_id"):
            first["id"] = args["local_id"]
        local = [first]
        if args["method"] == "cert-xauth":
            local.append({"method": "xauth"})
        cas = []
        for path in filter(None, args.get("ca", "").split(",")):
            with open(path, encoding="utf-8") as f:
                cas.append(f.read())
        remote = {"method": "certificate", "ca": cas}
        if args.get("remote_id"):
            remote["id"] = args["remote_id"]
        auth = {"local": local, "remote": remote}
    elif schema == 1:
        auth = {"method": args["method"]}
        for key in ("local_id", "remote_id"):
            if args.get(key):
                auth[key] = args[key]
    else:
        psk = {"method": "psk"}
        if args.get("local_id"):
            psk["id"] = args["local_id"]
        local = [psk]
        if args["method"] == "psk-xauth":
            xauth = {"method": "xauth"}
            if yes[args.get("otp", "no")]:
                xauth["otp"] = True
            local.append(xauth)
        remote = {"method": "psk"}
        if args.get("remote_id"):
            remote["id"] = args["remote_id"]
        auth = {"local": local, "remote": remote}
    profile = {
        "schema_version": schema,
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
