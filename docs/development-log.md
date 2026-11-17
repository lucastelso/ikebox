# Development log

A chronological record of the steps taken, the problems found and how they
were solved. The formal decisions are in the [ADRs](adr/README.md). The plan
is in the [roadmap](roadmap.md).

> This log was written in Portuguese up to step 35 and translated to English
> in 0.2.0 (step 36). Commit messages before 0.2.0 are in Portuguese.

---

## 2026-11-15: starting point and foundation

### Where the project starts

Before this repository existed, the maintainer's work VPN (legacy IPsec on
FortiGate: IKEv1 aggressive, group PSK + XAuth) only worked on Linux through a
manual flow. It consisted of:

1. A Docker `ubuntu:20.04` image with strongSwan 5.8.2.
2. A `swanctl.conf` mounted in the container, **with the secrets in plain
   text**.
3. A bash script, run with `sudo`, that:
   - started the container with `--privileged`, `SYS_MODULE` and
     `/lib/modules`;
   - ran `swanctl --load-all` and `--initiate` via `docker exec`;
   - extracted the virtual IP from the text output with `grep`;
   - added the VIP and the route **to the physical interface** (Wi-Fi), and
     the DNS too;
   - on exit, ran **global** `ip xfrm state flush` and `ip xfrm policy flush`.

The flow works and is validated. The analysis showed what had to improve for
it to become an app other people can use:

| Problem in the manual flow | Solution in the app | ADR |
|---|---|---|
| Secrets in a text file | VICI `load-shared` in memory + optional keyring | 0003, 0007 |
| `--privileged` + `SYS_MODULE` | `NET_ADMIN` only | 0005 |
| VIP/route/DNS on the physical interface | Dedicated XFRM interface `ipsc0` | 0004 |
| Global XFRM flush (kills other IPsec) | Atomic cleanup: delete `ipsc0` | 0004 |
| Parsing `swanctl` text (`grep`) | Structured replies and events via VICI | 0003 |
| Configuration fixed to one VPN | Generic JSON profiles | 0008 |
| Needs `sudo` on every use | Only the `docker` group; root stays in the container | 0005 |

### Step 1: repository and privacy rule

- `git init -b main`.
- The project will be published on GitHub, so it was born with the rule "no
  real VPN data in the repository" (ADR-0010):
  - the maintainer's private material (the original plan with work data and
    the copy of the manual flow) was listed in `.git/info/exclude`, which is
    local and never versioned;
  - `scripts/check-private.sh` was created: it blocks commits that match
    private patterns kept in `.git/info/private-patterns`, also outside git.
    The output shows only the file and line, never the matched text.
- Manual tests of the guard:
  - a file with a private IP and domain was blocked;
  - similar words did not produce false positives, thanks to word boundaries
    (`\b`);
  - the commit message is checked, and git's comment lines are ignored.
- Bug found and fixed before the first commit: with `set -o pipefail`, a
  `git grep` without results (exit code 1) made the pipeline "fail" and
  blocked clean commits. The fix was to capture the exit code explicitly. The
  same care applies to `grep -q` in a pipeline, which can cause SIGPIPE.

### Step 2: base documentation

- README: what it is, who it is for and not for, how it works, security.
- Architecture, roadmap, `SECURITY.md` (threat model and accepted risks),
  `CONTRIBUTING.md` (branch flow, Conventional Commits, privacy rule).
- ADRs 0001–0010. Decisions 0001–0009 come from the original plan, and 0010
  (privacy) was added now.

### Environment and a change in the roadmap order

- The verified environment:
  - Fedora 45, kernel 7.3, with the `xfrm_interface` module available and
    `esp4` loaded;
  - systemd-resolved running, SELinux enforcing, Docker daemon without
    SELinux;
  - Docker 29, Node 24, git 2.56.
- Missing: **Rust**, the **Tauri system libraries** (webkit2gtk etc.) and
  **shellcheck**.
- Installing Rust at the user level (rustup) was blocked by the permission
  policy of the automated session, and the libraries need `sudo`. Both were
  left to the maintainer (see README, "Prerequisites").
- So the roadmap was reordered. First comes what can be validated right away,
  with Docker and Node only:
  1. repository tooling (hooks, commitlint, shellcheck via Docker);
  2. the **engine** (container), the part with the highest technical risk,
     because replacing "VIP on the physical interface" with "dedicated XFRM
     interface" is the biggest change from the validated flow. It comes with an
     end-to-end test against a local IPsec gateway.

  The Rust core and the GUI come next.

### Step 3: repository tooling (`chore/tooling`)

- `package.json` at the root, with **exact** versions: lefthook 2.2.2,
  commitlint 21.2.4. npm 11 blocks dependency install scripts by default.
  lefthook's `postinstall` stayed blocked, and the hooks are installed by the
  project's own `prepare`, which is explicit and auditable.
- Hooks (`lefthook.yml`):

  | Hook | What it runs |
  |---|---|
  | pre-commit | private data guard + shellcheck on the staged `.sh` files |
  | commit-msg | guard on the message + commitlint |
  | pre-push | guard over the whole history |

- `scripts/lint-sh.sh`: uses the system shellcheck or, without it, the
  `koalaman/shellcheck:v0.11.0` image, pinned by digest, without network and
  with the repository read-only. That way linting works before the maintainer
  installs shellcheck.
- CI (`.github/workflows/ci.yml`):
  - actions pinned by **commit SHA**, with the tag in a comment (protection
    against rewritten tags in the supply chain);
  - minimal permissions (`contents: read`) and `persist-credentials: false`;
  - jobs: `npm run check`, commitlint over the PR range and gitleaks over the
    full history (image pinned by digest).
- gitleaks ran locally over the history and found nothing.
- dependabot updates actions, npm and the base image digest. It **ignores**
  Ubuntu version jumps, because 20.04 is pinned on purpose (ADR-0001).

### Step 4: engine (`feat/engine-imagem`)

Goal: validate, before any Rust code, the biggest change from the manual flow.
The VIP, the routes and the DNS leave the physical interface and go to a
**dedicated XFRM interface**. The container leaves `--privileged` and keeps
only `NET_ADMIN`.

**What was done**

- `engine/Dockerfile`:
  - `ubuntu:20.04` pinned by digest;
  - the same package list as the validated flow, with strongSwan pinned to
    `5.8.2-1ubuntu3.6`;
  - `systemd` without recommends, only for `resolvectl`.
- `engine/entrypoint.sh`:
  - validates the variables;
  - cleans up the leftovers of a previous session;
  - creates `ipsc0` (`type xfrm if_id N`, MTU 1400);
  - starts charon;
  - hands the VICI socket over to the user's gid (0660);
  - on SIGTERM, stops charon and undoes everything.
- `engine/updown.sh`:
  - routes on the XFRM interface marked with `proto 241`;
  - full tunnel with `/1` routes and a host route to the gateway;
  - split DNS via `resolvectl`, through the host D-Bus;
  - validated arguments.
- `tests/engine/e2e.sh`: a **test IPsec gateway** (the same image in responder
  mode) mimics a legacy VPN: IKEv1 aggressive, `keyid:` group PSK + XAuth,
  modp1024/768, a VIP pool and DNS. The engine connects to it exactly as the
  app will. Two scenarios: the normal cycle and recovery after `SIGKILL`.

**Result:** tunnel established, VIP on the dedicated interface, route, split
DNS and **ping through the tunnel**. After `docker stop`, nothing is left on
the host. Nothing is left either after a `SIGKILL` followed by a reconnection.
The default route and the physical interface stay intact. The test takes about
10 s and was repeated to rule out flakiness.

**Problems found and how they were solved**

1. **The DNS received from the gateway did not reach the updown.** The log
   showed `handling INTERNAL_IP4_DNS attribute failed`. With the `resolve`
   plugin off, nobody handled the attribute. The `updown` plugin only passes
   the DNS servers on (`PLUTO_DNS4_<n>`) with
   `charon.plugins.updown.dns_handler = yes`, which is off by default. Fix:
   turn the option on.
2. **Orphan SAs were not removed.** `ip -o xfrm state` separates blocks with
   `\`, which stuck to the fields (`dst 203.0.113.2\`). Fix: replace `\` with a
   space in awk before reading the fields. The output of `ip xfrm state`
   contains keys, so it is only read by awk and never printed.
3. **Orphan policies with `if_id` cannot be removed by iproute2 5.5.**
   Investigated with a harmless policy (documentation addresses and an
   `if_id` without an interface):
   - `delete` by selector without `if_id`: `ENOENT`;
   - `delete ... if_id`: 5.5 does not accept the argument;
   - `deleteall` with a filter: `ENOENT`;
   - `delete index N`: the kernel also requires the `if_id`.

   Modern iproute2 (6.11/7.0) accepts `if_id` and removes the policy.
   Analysis: these policies only exist after a `SIGKILL` and are inert,
   because they only match traffic from the XFRM interface with that `if_id`.
   The test **proved** they do not get in the way of reconnecting. With the
   same VIP, charon replaces them (same selector) and removes them on
   shutdown.

   Decision: do not embed a second iproute2 in the image now. The limitation is
   documented, the entrypoint warns in the log, and the item went to the
   roadmap. The test policy and the leftovers of the first runs were removed
   from the kernel with a throwaway container with a modern iproute2.
4. **Race condition in the test: an old socket taken as "ready".** After a
   `SIGKILL`, the socket file stayed in the directory, with mode 0660, and the
   test moved on before the new engine finished cleaning up. Fix: delete the
   socket before starting the engine and wait for the readiness log line. That
   became a rule of the **engine contract for the app** (see architecture).
5. **charon logs the XAuth username at level 1.** The password and the PSK do
   not show up, and there is an automatic assertion for that in the E2E. But
   the line `XAuth authentication of '<user>'` does, so the app masks the known
   identities before showing the log.

**Other details checked**

- The engine's `docker inspect` contains no secrets (asserted in the E2E).
- `version` via VICI works as a **regular user** through the socket bind
  mount. The test uses a minimal Python client for the VICI protocol. It is the
  proof that the app can control charon without root.
- The kernel loads the `xfrm_interface` module on its own when the interface is
  created, so neither `SYS_MODULE` nor mounting `/lib/modules` is needed.

### Step 5: full tunnel in the test (`test/engine-tunel-completo`)

- The `updown` path for the full tunnel (`0.0.0.0/0`) was still untested. It is
  exactly the path that can create a loop: if the IKE/ESP traffic to the
  gateway enters the tunnel itself, the connection drops.
- Testing it on the host network would divert all the machine's traffic during
  the test. Fix: scenario 3 runs the engine in the network namespace of the
  test network.
- A pitfall found on the way: **do not mount the host D-Bus in that mode**.
  `resolvectl` turns the interface name into an index in its own namespace and
  would send that index to the host's systemd-resolved, configuring the DNS of
  another interface. In production that does not happen, because the engine
  always uses `--network host`.
- The test gateway got a `tudo` child (`local_ts = 0.0.0.0/0`). strongSwan
  picks the child with the most exact TS, so scenario 1 keeps using
  `rede-teste`.
- Result: routes `0.0.0.0/1` + `128.0.0.0/1` on the XFRM interface, a host
  route to the gateway through the original path, ping without a loop. On
  `terminate`, *down* removes everything, and the default route stays intact.

---

## 2026-11-16: Rust core

### Step 6: prerequisites installed by the maintainer

- The maintainer installed Rust (rustup, stable toolchain 1.100.0), the Tauri
  libraries (webkit2gtk 4.1 etc.) and shellcheck. `npm run check` started using
  the native shellcheck, and the Docker fallback stays available.
- The maintainer added the XAuth login and the IP of the work machine reached
  through RDP to `.git/info/private-patterns`. The whole history was scanned
  again with the new patterns, with no findings.
- The private repository was created on GitHub, and `main` was pushed. Note:
  the commits carried the author's personal email. That became a pending item
  to decide before making the repository public (resolved in 0.2.0: the
  history was rewritten with the GitHub noreply address).

### Step 7: Rust workspace (`chore/scaffold-rust`)

- Workspace `Cargo.toml`:
  - edition 2024, resolver 3, `publish = false`;
  - workspace lints: `unsafe_code = forbid`, `unwrap_used`/`expect_used`/
    `print_stdout` as warnings, and clippy runs with `-D warnings`;
  - release profile with thin LTO and strip.
- `rust-toolchain.toml` pins Rust to 1.100.0. Updating it needs its own PR.
- `deny.toml`:
  - permissive licenses on the allow list;
  - RustSec vulnerabilities and yanked crates denied;
  - only the crates.io registry accepted as a source;
  - the project's own crates out of the license check until it is defined.
- Crates:
  - `ipsec-client-core`, for now only the skeleton and the planned modules;
  - `ipsec-client-cli`, with clap and a test that validates the CLI definition.
- `scripts/check-rust.sh` runs fmt → clippy → tests → cargo-deny and joined
  `npm run check`. The pre-commit runs `cargo fmt --check` when there are
  staged `.rs` files.
- CI: its own Rust job, with the toolchain read from `rust-toolchain.toml`, the
  `Swatinem/rust-cache` cache and `cargo-deny-action`, all pinned by SHA.
  dependabot also covers cargo.
- New resume document: current state, next step and how to set up the
  environment. It is updated on every merge, so work can resume from any point.

### Step 8: `core::engine` in Rust (`feat/engine-rust`)

- Dependencies:
  - `bollard` 0.21, compiled **without** the HTTP/TCP features, only the local
    Unix socket. Less surface and fewer dependencies;
  - `tokio`, `thiserror`, `tracing`, `sha2`/`hex`, `tar` and `rustix` (uid/gid
    without `unsafe`).
- bollard's API changed a lot in 0.21 (builders in `query_parameters`,
  `ContainerCreateBody`, `body_full`). The signatures were checked in the
  downloaded source code instead of guessed.
- Modules:
  - `context`: `engine/` files embedded, hash, tag and deterministic tar
    (mtime 0, owner root). A test compares `FILES` with the real content of
    `engine/`, so the hash never stops covering a new file;
  - `spec`: pure translation to the Docker API, with the same validations as
    the entrypoint and least-privilege tests (host network, only `NET_ADMIN`,
    `privileged` off, `security_opt` depending on SELinux/AppArmor);
  - `docker`: `ensure_image`, `start`, `stop` and `is_running`.
- Implementation decisions:
  - **Graceful recreation.** ADR-0002 talks about `rm -f` of the previous
    container, but `rm -f` sends SIGKILL and would leave leftovers on the host
    if a session were active. `start` runs `docker stop` (10 s, the entrypoint
    cleans up) and then removes it. The integration test covers that path by
    calling `start` twice;
  - **Runtime directory.** Created with 0700 and refused if it is a symlink,
    not a directory or owned by another user. That prevents another local user
    from preparing the socket path;
  - **Readiness.** The readiness line + `connect()` on the socket, with a 20 s
    timeout. On failure, the error carries the last 40 lines of the entrypoint
    log;
  - `clippy.toml` allows `unwrap`/`expect` only in tests.
- Finding: the build output through the API has ANSI color codes. They are now
  stripped (`strip_ansi`, with a test) before reaching the UI.
- The build through the API uses the classic builder, which still works on
  Docker 29. The first build takes about 35 s. With the image cached, the full
  integration test (start, recreate, stop) takes about 1 s.
- Checked after the test: no interface, container or socket left.

### Step 9: VICI client in Rust (`feat/vici`)

- **Decision (ADR-0011): our own implementation instead of `rsvici`.**
  - `rsvici` 0.1.5 is pre-1.0, has 4 dependencies and serializes messages via
    serde into buffers that **are not wiped**. `load-shared` carries the PSK
    and the password.
  - The protocol is small and there was already a Python reference in the
    repository.
- Implemented in three layers:
  - `message`: wire format, with values in `Zeroizing`, `Debug` without values
    and a depth limit;
  - `client`: packets, a command connection supporting streamed commands and an
    event connection;
  - `charon`: the `Charon` trait and types without identities.
- Unit tests (28 in core): byte-by-byte wire format; roundtrip; 7 malformed
  inputs; excessive nesting; oversized fields; `Debug` without secrets;
  parsing of `list-sa` and the events; a fake VICI server (socketpair) for the
  `streamed_request` flow.
- **Integration test `vici_docker.rs`.** The test gateway starts through the
  Docker API itself (`tests/support`). Then, without any `swanctl`:
  1. `version` (5.8.2);
  2. `load-shared` of the PSK and the XAuth password;
  3. `load-conn` and `list-conns`;
  4. `initiate`, with the `child-updown` (up) event received;
  5. `list-sas`: ESTABLISHED, VIP 198.51.100.x, child INSTALLED with the right
     TS;
  6. ping through the tunnel, with the byte counters going up;
  7. logs without PSK or password;
  8. `terminate`, with the down event and the routes removed;
  9. `unload-conn`.

  The test takes about 2 s. Cleanup always runs, even if an assertion fails,
  because the scenario runs in a separate task.
- clippy now runs with `--all-features`, to analyze the Docker test code too.

### Step 10: profiles (`feat/perfis`)

- **Model** (schema 1, serde with `deny_unknown_fields`): gateway; IKE
  (version, aggressive, encap, proposals, rekey, DPD); authentication (`psk`
  or `psk-xauth` method, `local_id`, `remote_id`); child (name, ESP proposals,
  rekey, TS, DPD and close actions); network (VIP, MTU and DNS: servers,
  domains, received DNS, default route).
- **Decisions:**
  - The **XAuth username is not in the profile.** It is a credential, like the
    password. That way the work profile can be handed to a colleague without
    the creator's employee id.
  - Authentication is an enumerated `method`, extensible to `eap`/`pubkey` in a
    schema 2 without breaking existing profiles. `parse_profile` reads
    `schema_version` before the rest, which is where migrations will go.
  - **ADR-0012: `presets.local/` is not embedded in the binary**, as the plan
    had it. A local build published by mistake would carry the work data.
    Distribution is done by exporting and importing JSON.
  - `close_action`: the validated flow uses `start`, and the profile generates
    `restart`. In charon's config parser, both are the same action, and the
    integration test confirms that charon accepts `restart`.
- **Validation** field by field, returning every problem to the UI: id slug,
  gateway host, IKEv1 required for aggressive and XAuth, proposal format,
  CIDRs, domains in resolved's format, MTU, durations, identities without
  quotes or control characters. The domain and IP rules are the same as in
  `updown.sh`.
- **Generation:** `to_load_conn` and `updown_command`. The arguments go in
  single quotes, and `shell_quote` refuses any quote or control character as a
  second barrier.
- **Store:** atomic saves (temporary file + `rename`), files 0600, directory
  0700, imports limited to 1 MiB and no overwriting without `overwrite`. Ids
  with `../` do not escape the directory.
- **Tests** (43 in core): insta snapshots of the `load-conn` of both presets
  (**manual review:** the legacy preset snapshot reproduces exactly the
  proposals, times and rounds of the validated flow); all presets valid and
  canonical (roundtrip); errors reported on every field; JSON with `"psk"`
  refused; unknown version refused; store with permissions, export/import and
  path traversal.
- **Integration:** `vici_docker.rs` stopped using a hand-written connection and
  now generates the `load-conn` from
  `tests/fixtures/perfil-gateway-teste.json`. The whole chain (JSON →
  validation → `load-conn` → real charon → tunnel → ping) passes in about
  2.4 s.
- Docs: the profiles guide (format, swanctl → profile mapping, handing it to a
  colleague) and `presets/README.md`.

### Step 11: secrets (`feat/segredos`)

- **Keyring:** the `keyring` 4 crate was split into `keyring-core` + backends.
  The first choice was the pure Rust backend (zbus), but `cargo-deny`
  **blocked it**: RustSec flagged `zvariant`, `zbus_names` and
  `zvariant_derive` as *unmaintained* (RUSTSEC-2026-0370/0371/0372), because
  they were merged into `zbus` 6 and the backend still uses `zbus` 5. Instead
  of opening an exception in `deny.toml`, the switch was to the **libdbus**
  backend (`dbus-secret-service-keyring-store`, crypto in Rust), which passes
  cleanly. libdbus exists on any Linux desktop. Building it needs
  `dbus-devel`/`libdbus-1-dev`, which was added to the README and to CI.
- **Backend API** checked in the source: `Store::new()`, `build(service, user,
  modifiers)` with a friendly label (it shows up like this in Seahorse: "IPsec
  Client: <profile> (<field>)").
- **`Credentials`:** `SecretString` everywhere, including the XAuth username,
  which is not secret but is personal; `Debug` only with presence/absence and
  the masked username; empty values count as missing; `shared_secrets` builds
  the `load-shared` secrets: the PSK owned by the group id and the XAuth one
  owned by the username.
- **`Redactor`:** masks the XAuth username and the group id in log lines,
  including the id without the `keyid:` prefix, which is how charon logs it.
  Identities shorter than 3 characters are not masked, so the text is not
  ruined.
- **Tests** (53 unit tests in core): `Debug` without secrets; absences and
  empty values; secret owners; in-memory store; `<profile>:<field>` keys;
  masks; real charon lines masked.
- **Real keyring:** the `keyring_real_roundtrip` test (optional, `--ignored`)
  created, read and deleted an item in the GNOME Keyring of the maintainer's
  session and passed.
- **Integration:** `vici_docker.rs` now loads the secrets via
  `Credentials::shared_secrets`, and checks that the logs, after the
  `Redactor`, carry neither the XAuth username nor the group id. charon, by
  itself, logs both, and the test confirms that too.

### Step 12a: session in core (`feat/sessao-cli`, first part)

- **Real failure signatures.** Each typical failure was triggered on purpose
  against the test gateway, with `swanctl`, and the charon 5.8.2 lines were
  captured:

  | Triggered failure | Captured line |
  |---|---|
  | Wrong PSK | `calculated HASH does not match HASH payload` |
  | Wrong password | `XAuth authentication of '…' (myself) failed` |
  | Wrong IKE proposal | `received NO_PROPOSAL_CHOSEN` before the IKE_SA |
  | Wrong ESP proposal | the same warning, after the IKE_SA was established |
  | Nonexistent gateway | `retransmit N of request` + `not established after` |

  Those lines became the tests of `session::diagnose`.
- **`core::diagnostics`:** root; `XDG_RUNTIME_DIR`; Docker; `xfrm_interface`
  (through `/sys/module` or `/lib/modules`); systemd-resolved; system D-Bus;
  ports 500/4500, by parsing `/proc/net/udp*`, without root. `doctor` adds the
  keyring and the image.
- **`core::session`:**
  - `establish`, generic over `Charon` and tested with a mock: it loads the
    secrets before the connection and, if a credential is missing, does not
    touch charon;
  - `connect` (preflight → image → engine → events → negotiation), which stops
    the engine on any failure;
  - `status` and `disconnect`, which work from any process, because the
    container carries the profile label;
  - events for state, masked log, tunnel drop and recovery.
- A concurrency detail: logs arrive through another VICI connection. Before
  diagnosing a failure, the session waits 300 ms for the last ones to arrive.
- **Integration `session_docker.rs`:** wrong password → `XauthCredentials`,
  without identities in the detail and with nothing left running; connection
  with events; ping; second connection refused (`AlreadyConnected`); `status`;
  `disconnect`; idempotence. It takes about 4 s.

### Step 12b: CLI (`feat/sessao-cli`, second part)

- **`ipsec-client-cli`** (clap): `doctor`, `profiles`
  (list/show/import/export/validate/delete/path), `secrets`
  (set/status/forget), `connect`, `status`, `logs`, `disconnect`. Guide in
  `docs/cli.md`.
- **Credentials in `connect`:**
  - first the keyring; whatever is missing is asked in the terminal, with
    passwords read from the TTY without echo (`rpassword`);
  - `--remember` stores them in the keyring;
  - `--credentials-stdin` reads `key=value` lines from standard input, like
    `docker login --password-stdin`, for automation and for the E2E. Secrets as
    arguments or environment variables were discarded on purpose: they show up
    in `ps` and in the history.
- **Foreground:** progress and log show up during the negotiation; after
  connecting, `Ctrl+C` disconnects, and tunnel drops and recoveries show up on
  screen. `--detach` returns the terminal, and the tunnel is an independent
  container.
- **Scripts:** `status` exits with 3 when disconnected, and `doctor` exits with
  1 when something prevents connecting.
- **`logs`:** masks the group id and the XAuth username of the active profile.
  The profile comes from the container label, and the username from the
  keyring.
- **`tests/cli/e2e.sh`** (`npm run test:cli`): a real user's flow against the
  test gateway, with a temporary `XDG_CONFIG_HOME` and fake credentials via
  stdin, without touching the maintainer's keyring or profiles: doctor; import,
  list and validate (profile saved as 0600); wrong password → a clear message
  without exposed identities and with the host clean; connect `--detach`, with
  output without secrets; status with VIP; ping; logs without secrets;
  disconnect, with status = 3 and the host clean. The test passed and joined
  CI.

### Step 13: importing `swanctl.conf` (`feat/import-swanctl`, moved up)

- **Why move it up:** it is the shortest path for the maintainer to create the
  work profile from the file that already works, without typing values and
  without them going through this session, and to hand the configuration to
  the colleague.
- **Parser** for the strongswan.conf format: sections, `key = value`, `#`
  outside quotes and warnings for `include`.
- **Security:**
  - the `secrets` section is **skipped without being stored**: the parser only
    counts the opening and closing braces;
  - a `secret` key outside `secrets` makes the import refuse;
  - `xauth_id` is not copied, because the username is a credential;
  - the tests check that neither the profile JSON nor `Debug` contains the
    fixture's secrets.
- **Mapping:** connection fields, `local*`/`remote*` rounds (psk, xauth) and
  the first child; actions `start`/`restart` → `restart`,
  `route`/`trap`/`hold` → `trap`, `none`/`clear` → `none`; durations with an
  s/m/h/d suffix; names turned into ids (`rede_interna` → `rede-interna`);
  certificates and EAP refused with a clear message.
- **Fixture:** the same structure as the maintainer's real `swanctl.conf`
  (tabs, comments, `#proposals`, `local-1`/`local-2`, `secrets` with quotes
  and `#` inside), with fake values.
- **CLI:** `profiles import-swanctl`, with `--dns`/`--domain`/
  `--use-received-dns`, because DNS does not exist in swanctl.conf, and
  `--output` to write a 0600 file.
- **CLI E2E:** imports the test gateway's `swanctl.conf` (which has `secrets`
  with fake credentials), checks that the profile has no secrets or username,
  and **connects with that profile**, with a ping through the tunnel. Passed.

### Step 14: first real use and the quick start guide

- The maintainer installed the CLI and imported the work profile with
  `profiles import-swanctl`, passing the DNS and the domain from the old
  script. Then they stored the credentials with `secrets set`, without any
  value going through the repository.
- Two stumbles became documentation: `cargo: command not found` (missing
  `~/.cargo/env` in the shell) and placeholder markers copied literally.
- A new quick start guide: simple steps for a colleague who received the
  exported profile (install, import, `secrets set`, `connect`, error table).

---

## 2026-11-16: new direction, a desktop app without Docker

The maintainer set the final goal: a **good-looking, simple and secure** app
that replaces FortiClient, distributed as an **AppImage**, where the user
installs nothing besides the app itself.

**Points raised in the planning**

- **npm does not reach the user.** Node and npm are build tools. Tauri
  compiles the frontend into the binary. Putting the frontend in a container
  was discarded: it solves nothing and brings Docker back.
- **Docker was the real "installing things"**, besides putting the user in the
  `docker` group, which is equivalent to root.
- **Flatpak was discarded.** The sandbox prevents netlink/XFRM and privileged
  services, and working around it voids the sandbox. The formats are AppImage
  + rpm + deb.

**The maintainer's decisions**

- an **embedded** engine: strongSwan compiled by us and a privileged helper
  installed **once**, with the administrator password;
- the **newest strongSwan that works**, with 5.8.2 as a fallback;
- the order **proof → GUI → engine → release**.

**What changes and what stays**

- The core (`vici`, `profiles`, `secrets`, `session`, `diagnostics`) is
  reused.
- `core::engine` gets a backend abstraction.
- The `entrypoint.sh` logic goes to the helper side.
- The roadmap was rewritten with phases A to E.

### Step 15: embedded engine proof (`spike/engine-nativo`)

**Goal:** prove that Docker can be removed from the user's side by compiling
our own strongSwan, and pick the version.

**Build** (`native-engine/`):

- official tarballs of 6.1.0, 6.0.7, 5.9.14 and 5.8.2, checked by a **pinned
  SHA-256** and the **GPG signature** (key `948F…BA77`, versioned in
  `native-engine/keys/`);
- Ubuntu 22.04 builder (glibc 2.35, the same base as the AppImage), with a
  deterministic output tarball.

Three stumbles, all solved:

1. **Plugins missing in the static executable.** charon started with
   `aes_plugin_create not found`. In a static build, the plugins only get in
   with `--enable-monolithic`, which generates a constructor table. To generate
   it, strongSwan needs Python **at build time**, which was added to the
   builder.
2. **5.8.2 did not link** with gcc 11. It is old code against `-fno-common`,
   the default since gcc 10. Fix: `-fcommon` only for 5.8.x.
3. **A 12 MB package** because of the build `.a` libraries. After removing
   them, it is about 1.8 MB compressed and 4 MB installed, with charon
   depending **only on glibc** (gmp is static).

**Test:**

- `native-engine/spike/` builds a **Fedora 45** image (the same glibc 2.44 and
  modern iproute2 as the maintainer's host) only as a stand-in for root,
  because the real privilege model is phase D;
- `tests/engine/e2e.sh` got `IPSEC_CLIENT_E2E_ENGINE_IMAGE` and
  `IPSEC_CLIENT_E2E_EXPECT_VERSION`. The test gateway is still Ubuntu's
  strongSwan 5.8.2, mimicking legacy VPNs.

**New scenario 4: PFS with modp768 only in phase 2.** The maintainer's legacy
config says the gateway "asked for DH1". The first test failed and revealed an
IKEv1 rule: **phase 2 carries only one DH group, the one of the first
proposal**. strongSwan drops the proposals with other groups. Since the
maintainer's list starts with modp1024, the real VPN negotiates modp1024 PFS
today. The scenario now offers modp768 first, and passes.

**Result:** **all four versions passed the 26 checks** (4 scenarios). 6.1.0
logged no weak algorithm warnings. Bonus: with the modern iproute2, the
`entrypoint.sh` now **removes the orphan XFRM policies** (0 left in scenario
2). The old limitation only exists in the Docker image.

**Decisions (ADRs):** 0013 (embedded engine, no Docker for the user); 0014
(privileged helper, authorized once: polkit + sandboxed `systemd-run`); 0015
(AppImage + rpm + deb, no Flatpak); 0016 (strongSwan 6.1.0, with 5.8.2 as a
fallback).

**Recorded limitation:** without OpenSSL, there are no ECP curves. The example
IKEv2 preset now uses `curve25519`.

**CI:** a new `native-engine` job (`npm run test:native`).

### Step 16: GUI scaffold (`chore/scaffold-gui`)

**Versions**, all pinned exactly: Tauri **2.12** stable (the `tauri` crate
already has a 3.0 alpha, avoided on purpose); `tauri-specta` 2.0.0-rc.29, with
`specta` rc.28 and `specta-typescript` 0.0.15; React 19.3, Vite 8, Tailwind
4.3, TypeScript 7, biome 2.6, vitest 5.

npm 11 blocks dependency install scripts. None of the new dependencies needs
them; only lefthook has one, and it stays blocked.

**`src-tauri/`:**

- thin commands over `core` (ADR-0009);
- **own DTOs** (`dto.rs`): `core` does not depend on specta, and only what the
  UI needs crosses into the webview;
- a Rust test (`bindings_are_up_to_date`) compares the versioned
  `src/bindings.ts` with the generated one. To regenerate:
  `UPDATE_BINDINGS=1 cargo test -p ipsec-client-app bindings`.

**Security:** strict CSP (`script-src 'self'`, no remote content; `style-src`
allows inline because of the positioning styles of the Radix components);
`freezePrototype`; a capability with only `core:default`; no shell or fs
plugins.

**`src/`:** shadcn/ui color tokens in OKLCH, with a light and a dark theme that
follow the system or are forced by `data-theme`; `Button` and `Card`
components; a temporary screen already using the typed IPC; tests with
`mockIPC`.

**Temporary identity:** `assets/logo.svg` (a shield with a lock in the theme
colors), from which `tauri icon` generates the icons.

**cargo-deny:** the Tauri tree brought two *unmaintained* warnings, `paste`
(via specta) and `proc-macro-error` (via glib-macros, from the GTK3 stack of
Tauri 2). Both are **compile-time macros**, with no code in the binary, and
without an alternative. They became justified exceptions in `deny.toml`. Unlike
the zbus case (step 11), where there was an alternative.

**CI and hooks:** the Rust job got the Tauri libraries; the checks job runs
biome, tsc, vitest and the build; the pre-commit runs biome on staged web
files; `npm run check` covers everything.

### Step 17: connection screen (`feat/ui-conexao`)

**IPC (`src-tauri`):**

- commands `list_profiles`, `credentials_status`, `connect`, `disconnect`,
  `status` and `forget_credentials`, plus the typed `SessionUpdate` event;
- errors in tauri-specta's `Result` mode: TypeScript gets
  `{status: "error", error: UiError}`, with kind, message and details, instead
  of untyped exceptions;
- `disable_serde_phases()` keeps one TypeScript type per DTO.

**Security and robustness decisions:**

- the UI never receives secrets: `credentials_status` only returns what is
  stored and the masked username. `CredentialsInput` only comes in and has a
  redacted `Debug`;
- commands are `async` (synchronous Tauri commands run on the UI thread), and
  disk and keyring go to `spawn_blocking`;
- `ConnectionLock` refuses a second `connect` in progress, for example on a
  double click;
- `CredentialsDialog` wipes the typed values on close and after submitting.

**UI:** profile selector (remembered between runs, localStorage protected by
try/catch); central button with a state ring (gray, pulsing amber, green);
card with virtual IP, gateway, networks, time and traffic; the credentials
dialog asks **only for what is missing** and checks "remember" when there is a
keyring; errors show the likely cause, collapsible details and "Enter
credentials"; masked, copyable log panel; the status polled every 2 s also
**adopts connections opened by the CLI**.

**Tests:** 12 in the frontend, counting the `connectionReducer` ones and the
`HomeScreen` ones with `mockIPC`. jsdom needed a `ResizeObserver` stub because
of Radix. Rust got tests for the DTOs and for `apply_input`.

### Step 18: preset manager (`feat/ui-perfis`)

Importing and exporting presets was an explicit request from the maintainer.

**Security invariant: the webview never handles file paths.**

- The **native dialog** (`tauri-plugin-dialog`, called from Rust; no dialog
  permission for JS in the capability) and **drag and drop**
  (`WindowEvent::DragDrop`, handled in Rust) produce an `ImportPreview` with an
  **opaque single-use token**.
- The UI confirms with `confirm_import(token, options)`. A used or unknown
  token is refused.
- The preview reads at most 1 MiB and detects the type by extension or content
  (JSON or swanctl.conf).

**Dependency:** the dialog plugin uses the GTK3 picker by default, which Tauri
already loads. The XDG portal backend would pull zbus 5, blocked by cargo-deny
in step 11. `cargo tree` confirms there is no zbus.

**Commands:** `choose_import_file`; `confirm_import` (id/name, DNS for
swanctl.conf, `overwrite`); `profile_details`; `export_profile` (save dialog,
0600 file, no credentials); `delete_profile` (optionally deleting the
credentials).

**UI:** Connection | Profiles navigation; a profiles screen with cards that
expand the details; an import dialog with preview, DNS for swanctl.conf, id
conflict warning and collapsible conversion warnings; delete confirmation;
toasts (sonner). On export, the toast reminds that the PSK goes through
another channel.

**Tests:** 16 in the frontend. Rust tests the type detection, the single-use
token and the DNS validation.

**Left for later:** the profile editor (create, edit, duplicate), which became
`feat/ui-editor-perfis` in the roadmap.

### Step 19: the app really opens, and the System tab (`feat/ui-diagnostico`)

- **Smoke test of the real app.** A `tauri build --debug --no-bundle` (with the
  embedded frontend) run for a few seconds, a few times: the app opens and runs
  stable in the maintainer's Wayland session, without capability, plugin or CSP
  errors. That kind of problem escapes mocked tests.
- **System tab:** the `run_diagnostics` command, reusing
  `core::diagnostics::doctor`, with an icon per level and the fix hint; the
  system/light/dark theme in `data-theme` on `<html>`, applied before the first
  render and remembered; app and engine versions.
- **Accessibility:** biome refused `role="radio"` on a button, and the theme
  selector now uses a real visually hidden `<input type="radio">`, with visible
  focus on the label.
- 18 tests in the frontend.

### Step 20: first real connection through the app

The maintainer ran the app (`npm run tauri dev`, still with the Docker engine)
against the work VPN, a FortiGate with IKEv1 aggressive and group PSK + XAuth.
Result, read in the app's log panel:

- **phase 1:** established. The gateway picked the **second** proposal of the
  list (AES-128/SHA1/modp1024);
- **XAuth:** authenticated. The username and the group id show up **masked** in
  the log, which confirms the `Redactor` in real use (a short id becomes `***`);
- **virtual IP:** received and installed on the dedicated interface;
- **phase 2:** AES-256/SHA1 with **modp1024 PFS**. That confirms the step 15
  finding: in IKEv1, only the group of the first proposal is offered, so
  modp768 is never negotiated;
- the route and the split DNS went to the dedicated interface;
- **disconnection:** phase 2 and phase 1 DELETEs sent to the gateway, route
  removed and DNS reverted.

Pending items and ideas from here: a long-duration test (hours, with RDP),
through rekey and DPD; the copied log still showing the gateway, DNS and
internal domains (idea: "copy anonymized log", done in step 33).

### Step 21: license (GPL-3.0-or-later, ADR-0017)

- **Facts:** strongSwan is GPL-2.0-or-later, with an exception for LGPL
  libraries and OpenSSL; GMP, static in charon, is LGPLv3/GPLv2; all Rust and
  frontend dependencies shipped in the app are permissive and compatible with
  GPLv3; the app talks to charon over a socket, so it is not a work combined
  with strongSwan.
- **Choice:** GPL-3.0-or-later. For a security tool that replaces a
  proprietary client, it guarantees that redistributed derivatives stay open
  and auditable, and it is from the same family as the embedded strongSwan.
- **Applied:** `LICENSE` with the official text; SPDX in the `Cargo.toml`s,
  `package.json` and the Tauri package metadata; `deny.toml` with exceptions
  only for our crates; *inbound = outbound* contributions.
- **Compliance of what is distributed:** `THIRD_PARTY_NOTICES.md` points to the
  exact source of strongSwan (tarball + SHA-256 + GPG) and of GMP (Ubuntu
  22.04 package, with the version in `BUILDINFO`); the engine build packs both
  license texts in `share/licenses/`.

### Step 22: tray, notifications and single instance (`feat/tray`)

- **Problem:** the official notification and single-instance plugins use
  **zbus** on Linux, with no way to switch. cargo-deny blocks zbus 5 (step 11).
- **Solution without a new dependency:**
  - **notifications:** a direct call to
    `org.freedesktop.Notifications.Notify` through libdbus (the `dbus` crate,
    already there for the keyring). Best effort: without a notification server,
    it does nothing;
  - **single instance:** an `app.sock` Unix socket in
    `$XDG_RUNTIME_DIR/ipsec-client/`. The second run sends `show` and exits,
    and the first brings the window to the front. A stale socket from a dead
    instance is replaced;
  - **tray:** Tauri's own `tray-icon` feature (libappindicator, loaded at
    runtime). Creation is protected against panics, so without the library or
    on GNOME without the AppIndicator extension the app goes on without a tray.
- **State monitor** (Rust, every 3 s): updates the tray tooltip, status and
  "Disconnect", and notifies the **transitions** (connected, dropped and
  reconnecting, back, disconnected). `transition_notice` is pure and tested,
  and the first read only syncs, without notifying. Since it reads the engine
  state, it also works for sessions opened by the CLI.
- **"Quit"** does not bring the VPN down, which is an independent process (as
  in the CLI). Closing the window also quits the app, without hiding it in the
  tray, because on GNOME without the extension the window would be
  unreachable.
- **Smoke test:** with instance A open, B exited in about 300 ms with code 0,
  and A stayed open.

### Step 23: second real connection, with DPD

- The repeated real session showed **DPD working**: charon sent a `DPD
  request` and the FortiGate answered `DPD_ACK`. NAT-T keepalives are going out
  too. The maintainer confirmed the tray.

### Step 24: privileged helper and native backend (`feat/helper`, phase D)

**Design, security first:**

- **`crates/proto`** (serde only): JSON-per-line protocol and the validation
  rules, the same as in `entrypoint.sh`, **shared** by the app and the helper.
  `deny_unknown_fields`: an extra field, such as a hidden path or command, is
  refused (tested).
- **`crates/helper`:** the only root binary, with minimal dependencies (serde,
  rustix and dbus).
  - Identity through `SO_PEERCRED`, filled in by the kernel.
  - Authorization through **polkit** (`CheckAuthorization` with
    `unix-process{pid, start-time, uid}`, non-interactive; the policy uses
    `allow_active=yes`).
  - The test mode replaces polkit with a uid list, and only root can start the
    helper that way.
  - **The helper runs nothing the client sends:** the client only sends
    `if_id`, MTU, log level, the interface name (validated) and the profile id.
    Paths, command and sandbox are fixed in the helper. (Step 32 found that
    charon itself still accepted a command from the client over VICI; see
    ADR-0019.)
  - The engine starts with `systemd-run --unit=ipsec-client-engine`, sandboxed
    (the setup is tested as a pure function) and with the **reused
    `entrypoint.sh`** as ExecStart.
  - Readiness through the VICI socket. `stop` via `systemctl stop`, which is
    graceful: the entrypoint cleans up the host. `logs` via `journalctl`.
- **`unsafe`:** the workspace forbids it. The helper uses `deny` instead of
  `forbid` for **one** commented exception: adopting file descriptor 3
  inherited from systemd socket activation.
- **`entrypoint.sh` in native mode:** `ProtectSystem=strict` makes the root
  read-only, so the runtime config goes to the `RuntimeDirectory` and comes in
  through `STRONGSWAN_CONF`, including the base; the socket is per user
  (`/run/ipsec-client/<uid>`); the engine build uses
  `--with-piddir=/run/ipsec-client-engine`; the Docker mode stays the same.
- **`core`:** `NativeEngine` (the helper client, tested with a fake helper);
  `Backend` (native when installed, Docker as a fallback, forced with
  `IPSEC_CLIENT_BACKEND`), used by the session, the diagnostics and the CLI
  `logs`.
- **Bug found on the way:** the profile MTU was ignored, and the engine always
  used 1400. Now it comes from the profile.
- **Packaging (`packaging/`):** helper units, with their own hardening and
  `RuntimeDirectoryPreserve` so sessions do not drop when the helper restarts;
  the polkit policy; `install-native.sh`, which refuses a tarball with paths
  outside `opt/ipsec-client/engine`, installs into `/opt/ipsec-client` and runs
  `restorecon`, because the maintainer's host has SELinux enforcing.
- **Verification without root:** unit tests of proto, the helper and the
  client; the helper refuses to run without root; `test:docker` and `test:cli`
  green, proving the Docker path did not change.

### Step 25: installer fix

- The maintainer's first run of `scripts/test-native.sh` failed with an
  "unknown argument: -d" error. The script function was called `install` and
  **shadowed the system `install` command**: `install -d` called the function
  itself. The functions are now `do_install`/`do_uninstall`.
- The installer is now tested as root in a container, with `systemctl`
  replaced by a stub.

### Step 26: second root test run and the socket race

- **Progress confirmed on the maintainer's computer:** the network component
  installed (unit, socket and SELinux), **polkit authorized** the local session
  and the **embedded engine started under systemd**, sandboxed.
- **Failure:** the app got `Permission denied` when connecting to the VICI
  socket. It was a **race in the helper**: it considered the engine ready as
  soon as *root itself* connected to the socket. That happens before
  `entrypoint.sh` hands the socket over to the user. In the Docker mode it did
  not happen, because there readiness is the log line written after the
  handover.
- **Fix:** the helper only replies `Started` once the socket has been handed
  over (owner and mode). A unit test uses a real socket.

### Step 27: embedded engine validated as root

`scripts/test-native.sh` passed entirely on the maintainer's computer (Fedora
45, SELinux enforcing), **with no Docker in the VPN path**: Docker only runs
the test gateway. Proven: the installation (units, polkit policy,
`restorecon`); polkit authorizing the active local session without a password;
the systemd sandbox (strongSwan 6.1.0 as a restricted transient unit); and the
full CLI flow through the embedded engine (wrong password with a clear message
and a clean host, connection, status with VIP, ping, logs without secrets via
`journalctl`, disconnection with a clean host).

### Step 28: the app installs the network component (`feat/setup-appimage`)

**Goal:** whoever downloads the AppImage installs nothing by hand. The first
time, the app offers "Install network component", the system asks for the
administrator password **once**, and after that connecting no longer asks for
a password. Decision recorded in **ADR-0018**.

**How it works:**

1. `scripts/stage-network-component.sh` builds the **component bundle** in
   `target/network-component/`: helper, strongSwan tarball, engine scripts,
   units, policy, the installer itself, `VERSION` and `SHA256SUMS` (2.4 MB).
2. `npm run bundle`, with `src-tauri/tauri.release.conf.json`, runs that script
   before the build and puts the directory among the app resources, in
   `<resources>/network`.
3. When installing, `core::component` copies the bundle to a 0700 directory in
   `$XDG_RUNTIME_DIR` (root cannot read the AppImage FUSE mount), checks the
   hashes in the copy and runs `pkexec <copy>/install-native.sh install
   --bundle <copy> --origin app`, with a clean environment.
4. The installer, as root, copies the bundle to a directory **of its own**,
   refuses links and special files, checks `SHA256SUMS` and the full list,
   refuses a tarball outside `opt/ipsec-client/engine`, installs and records
   `VERSION`, `SHA256SUMS` and `ORIGIN`.
5. The app compares the installed `SHA256SUMS` with that of the bundle it
   carries. If they differ, the state is **outdated** and the "Update" button
   shows up. Removal uses the installer kept in `/opt/ipsec-client/libexec`,
   only after checking that it and the directories above it belong to root and
   that nobody else can change them.

**In the interface:** System → Network component (state, versions,
Install/Update/Reinstall and Remove with confirmation); a first-use notice on
the main screen; an "Install network component" button on the prerequisite
error. With an active connection, installing and removing are refused.

**In the terminal:** `ipsec-client-cli component status [--bundle]`,
`component install --bundle <dir>` and `component uninstall`.

**Diagnostics:** without the component and without Docker, the "Network
component" check now **fails**, with the hint to install the component (before,
AppImage users saw a hint to install Docker).

**Tests:** `tests/packaging/install-test.sh` (`npm run test:packaging`, also in
CI): installs, reinstalls, removes and refuses tampered files, symlinks,
incomplete bundles, tarballs outside the engine, invalid origins and running
without root. Rust: `SHA256SUMS` parsing, bundle integrity, private copy modes,
installation record and state, `pkexec` results (126 = cancelled, 127 = not
authorized), clean environment and root ownership for removal. Frontend: the
card and the main screen.

**Findings on the way:**

- `tempfile` creates directories with 0755. The 0700 mode is now set at
  creation, with a test.
- The system cargo (installed by dnf) won over rustup in PATH and ignored
  `rust-toolchain.toml`. The scripts now always load `~/.cargo/env`.
- The first package build revealed an old scaffold error: `"category":
  "Network"` is not a valid Tauri category. Changed to `Utility`.

### Step 29: installing like a real app (`chore/instalacao-desktop`)

**The maintainer's request:** install it "nicely", the way a user would, with
the app showing up in the menu, and a short guide in the repository.

**What the first `.rpm`/`.deb` showed:** the package name `i-psec-client`
(Tauri derives it from `productName`, "IPsec Client" in kebab-case, with no
way to change it; the same name went into the resources path, with a space);
maintainer "github", taken from the identifier; a menu shortcut generated from
the product name, in the `Utility` category.

**Fixes:** `productName` = `ipsec-client` (package `ipsec-client`, resources in
`/usr/lib/ipsec-client/network`; the displayed name is still "IPsec Client");
`publisher` = "IPsec Client contributors"; our own desktop entry
(`src-tauri/linux/ipsec-client.desktop`): `Name=IPsec Client`, `Network`
category, generic name and description in English and Portuguese, keywords so
the menu search finds it by "vpn", `StartupWMClass=ipsec-client`. Checked in
the Tauri code: without `enableGTKAppId`, GTK uses the program name as the app
id, so GNOME binds the window to the shortcut.

**Documentation:** a new installation guide (which file, installing rpm, deb
and AppImage, first use, update, uninstall, building from source, common
problems); the colleague's quick start now uses the app; the README rewritten
with the current state; SECURITY.md got the threat model of the embedded
engine.

**Point to watch:** a `.deb` built on Fedora depends on `libappindicator3-1`,
which does not exist on recent Debian/Ubuntu. The release `.deb` must be built
on Ubuntu, where Tauri picks `libayatana-appindicator3-1`.

**Behavior** (documented): closing the window quits the app, but does not
disconnect the VPN, because the engine is a system service. When reopened, the
app shows the active connection.

### Step 30: validation on the real VPN and version 0.1.0

**The maintainer's validation (2026-11-16), through an end user's path:**

1. installed `ipsec-client-0.0.0-1.x86_64.rpm` with `dnf install` and opened
   **IPsec Client** from the applications menu;
2. the app showed **"Network component update"**, because the installation done
   by `test-native.sh` had no version record. They clicked **Update now** and
   typed the administrator password: the real `pkexec`;
3. connected to the **work VPN** (FortiGate, IKEv1 aggressive + group PSK +
   XAuth) through the embedded strongSwan 6.1.0, **without Docker**, and
   reached the work machine through Remmina.

**Checked on the system after the test** (metadata only, no VPN logs):
`rpm -q ipsec-client` = `ipsec-client-0.0.0-1.x86_64`;
`/opt/ipsec-client/ORIGIN` = `app`; the installed `SHA256SUMS` identical to the
app bundle's.

That validates end to end the embedded engine on the real VPN, the component
installation and update by the app, and the installation as an app.

**Version 0.1.0:** from 0.0.0 to **0.1.0** in the Rust workspace, in
`tauri.conf.json` and in `package.json`. The first working version.
The CHANGELOG got a `[0.1.0] - unreleased` section. The `v0.1.0` tag and the
packages on GitHub Releases are left for `chore/release`, as planned, so the
tag points to the commit that CI packages.

Expected effect when installing the 0.1.0 `.rpm` over 0.0.0: the app shows
"Network component update", because the package `VERSION` changes. That is
the update flow working.

### Step 31: v0.1.0 release packages (`chore/release-pacotes`)

**The maintainer's request:** build the version's rpm, deb and AppImage and
show how to publish them on GitHub.

**Where to build.** Not on the maintainer's Fedora, but in an **Ubuntu 22.04
container** (`packaging/release/Dockerfile`, called by
`scripts/build-release.sh`), the same environment CI will use:

- **AppImage:** it only runs on systems with a glibc equal to or newer than
  the one it was built on. Built on Ubuntu 22.04, it requires glibc 2.34 and
  runs on Ubuntu 22.04+, Debian 12+ and Fedora;
- **.deb:** built on Ubuntu, it depends on `libayatana-appindicator3-1`, the
  right name on current Ubuntu/Debian (the one built on Fedora asked for
  `libappindicator3-1`);
- **privacy:** the code comes from the commit (`git archive`) and is compiled
  in `/build`, so no path from the maintainer's home goes into the binaries.
  Checked with `strings`;
- **Rust and Node** enter the image verified by checksum (rustup-init
  `.sha256` and Node's `SHASUMS256.txt`). `npm ci` runs with
  `--ignore-scripts`.

**The first build found a problem that would have broken the AppImage.**
Inside it, `ipsec-client-helper` did not match the component's `SHA256SUMS`.
When assembling the AppImage, linuxdeploy walks the app's ELF executables and
rewrites their RPATH. Since the app checks the hashes before installing, it
would have refused the component as "corrupted". The `.deb` and the `.rpm`
do not go through linuxdeploy and were intact.

**Fix:**

- the helper goes into the package as `ipsec-client-helper.gz`. No loose ELF
  is left for linuxdeploy to touch, and the installer decompresses it after
  checking the hash;
- a new `scripts/verify-release.sh`, which runs at the end of every release
  build. It extracts the three packages and refuses if the component does not
  match its own `SHA256SUMS`, or if the AppImage requires a glibc above 2.35.
  Run on the old packages, it refused the AppImage, as expected.

**Publishing:** `docs/release.md` has the step by step:

- prepare the version, create the tag and build the packages;
- check them;
- publish through the GitHub website (Releases → Draft a new release → attach
  the files → pre-release) or with `gh release create`;
- who can download while the repository is private.

The release notes are in `docs/releases/v0.1.0.md`. The push and the
publication are done by the maintainer, because this environment has no
GitHub credentials.

**Verifier fix.** In the tag build, the `.rpm` check aborted with no message.
Ubuntu 22.04's `rpm2cpio` extracts Tauri packages in full (the cpio is
valid), but exits with code 1, and `pipefail` killed the script. Now the
verifier ignores `rpm2cpio`'s exit code and leaves the decision to the
component check, which fails if the extraction comes out incomplete. Each
failed extraction shows a clear message. The `v0.1.0` tag, not yet pushed,
was moved to the commit with the fix, and the packages were built again from
it.

### Step 32: security audit and privilege escalation fix (`fix/engine-updown-forcado`)

**The maintainer's request:** a security audit of the app (improvements,
optimizations and vulnerabilities), above all to make sure no work data had
leaked. v0.1.0 was already published and with a colleague.

**Leak scan: clean.** Done read-only, counting occurrences without ever
printing values:

- the guard (`check-private.sh --tracked` and `--history`), plus the branch
  and tag names and the tag message;
- a second net of values taken from the private files (gateway, DNS, domain
  and labels, group id, remote network), plus the home path, hostname and
  email.

It covered the tree, the 70 commits of every ref and the three v0.1.0
packages, including the strongSwan and the helper compressed inside them.
Result: no work data anywhere. In the packages, the only occurrence was the
username inside the util-linux copyright, which is someone else's email
(false positive).

Notes:

- the private files, which were in the project folder outside git with mode
  644 (the `vpn-docker/` copy has the PSK), were moved out, readable only by
  the maintainer;
- the `.rpm` built on the host, which is not the release one, contains the
  home path and must not be distributed;
- strongSwan 6.1.0 is the latest version and already fixes all the 2026 CVEs.

**The vulnerability: local escalation to root.** In the embedded engine:

1. charon runs as root;
2. the user talks VICI to it;
3. VICI lets the client choose the `updown` of each CHILD_SA;
4. charon runs that command through `/bin/sh -c`.

A malicious program running as the user of the active session could, with no
password (polkit `allow_active`), start the engine through the helper and load
a connection with `updown = <command>`. That command would run as root. The
sandbox did not stop the worst: `/proc/sys` was writable and the disks showed
up in `/dev`. It cannot be exploited from the network or by the gateway.

**Fix (ADR-0019):**

- **minimal strongSwan patch** (`native-engine/patches/`):
  - with `force_updown` and `force_if_id` in the config, the VICI parser
    ignores the client's `updown` and `if_id`;
  - it is applied after the SHA-256 + GPG check and recorded in `BUILDINFO`;
  - packaging, the release build and `verify-release.sh` refuse an engine
    without the patch. The verifier already refuses the v0.1.0 packages;
- **arguments without a shell:** the entrypoint writes the config and the
  `updown` arguments, one per line, into a root-owned file (`--args-file`).
  The profile DNS now goes through the helper (`StartParams.dns`, validated in
  `proto`). An old helper refuses the new field, and the app offers "Update
  network component";
- **socket and helper:**
  - VICI socket `0600` owned by the uid, in a root-owned `0711` directory.
    Before, it was `0660` for the effective group;
  - `stop` and `logs` only for the owner, and `status` without another user's
    profile;
  - the helper serves up to 8 connections in parallel, with 3 s for the
    request to arrive;
- **engine sandbox:** `PrivateDevices`, `ProtectKernelTunables`,
  `ProtectKernelLogs`, `ProtectProc=invisible`, `PrivateIPC`, `UMask=0077` and
  `SystemCallFilter=@system-service`.

**Tests:**

- **`test:native`, scenario 5:** a VICI client tries `updown = /bin/touch ...`
  and `if_id = 999`.
  - With the patch: the command does not run, the official updown applies the
    route and DNS, and the SAs get root's `if_id`.
  - With the v0.1.0 engine: the test fails ("charon ran the client's
    updown"). That confirms the flaw and that the test detects it.
- **`test:systemd` (new):** a Fedora 45 "computer" with real systemd in a
  privileged container. In it:
  - installation with the real installer;
  - connection through the CLI, with the profile DNS going through the helper;
  - check of the applied sandbox (read-only `/proc/sys`, invisible disks);
  - user bob cannot stop or read alice's VPN log, does not see her profile and
    cannot reach her VICI;
  - stalled connections do not block the helper;
  - the attack is blocked;
  - cleanup.

**The systemd test found two bugs that the plain container hid** (there, root
has every capability):

- after the `chown` to the user, the sandbox root (without `CAP_FOWNER`) can
  no longer `chmod` the socket. The order was swapped;
- the helper checked readiness by connecting to the socket. Without
  `CAP_DAC_OVERRIDE`, it cannot connect to another uid's `0600` socket. Now it
  checks the file's owner, mode and type, which the entrypoint only hands over
  after charon opens it.

**Verification:** `npm run check` green, including cargo-deny with no
advisories. `test:native` (5 scenarios), `test:systemd`, `test:packaging`,
`test:engine`, `test:docker` and `test:cli` green.

**Next:** `feat/log-anonimizado` and `chore/higiene-privacidade`, then the
v0.1.1 release. Whoever has v0.1.0 must update the app **and** click "Update"
on the network component.

### Step 33: anonymized log for sharing (`feat/log-anonimizado`)

**Reason (audit, step 32):** the log panel's "Copy" took the text as shown on
screen, with only the XAuth username and the group id masked. The gateway,
virtual IP, internal networks, DNS and the organization's domains went along.
The most likely way for work data to leak in the future is someone pasting
that log into an issue.

**What changed:**

- **`secrets::Anonymizer`** (Rust): besides the identities, it replaces with
  stable placeholders within the same text:
  - the gateway (name or IP) with `<gateway>`;
  - the profile DNS servers with `<dns-N>`;
  - the profile domains with `<domain-N>`, case-insensitive;
  - **every** IPv4/IPv6 address with `<ip-N>`.

  `0.0.0.0`, loopback and broadcast stay as they are, since they help reading
  the log, as do times, versions, SPIs and proposals.
  `session::anonymizer_for` builds the anonymizer from the profile;
- **app:** the button became "Copy anonymized". The text goes through the new
  `anonymize_log` command, and a notice asks to review it before sharing. If
  the anonymization fails, **nothing** is copied. The command does not open
  the keyring, because the screen lines already come with the username masked;
- **CLI:** `logs --anonymize`. It anonymizes the whole log at once, so the
  same address keeps the same placeholder, including with `--follow`.

**Tests:**

- unit tests with a real charon log (with RFC 5737 data and `example.com`):
  nothing leaks, and the placeholders are consistent. The cases `...IP`,
  `IP[port]`, `IP/mask` and `fe80::1` are replaced; proposals, versions,
  times, MAC and accented text stay intact;
- vitest for the panel: it copies the text from Rust and, without it, copies
  nothing;
- `tests/cli/e2e.sh`: `logs --anonymize` does not contain the gateway, the
  VIP, the domain or the group.

### Step 34: privacy hygiene (`chore/higiene-privacidade`)

**Reason (audit, step 32):** the scan of the packages for private data had
been done by hand.

**What changed:**

- **`scripts/check-private.sh --artifacts <dir>`.**
  - **What it does:** extracts the AppImage, deb and rpm, including the
    `engine.tar.gz` and the helper compressed inside them, and looks for the
    private patterns, the home path and the hostname (the generic ones are
    left out). It shows only the files.
  - **When it runs:** at the end of `scripts/build-release.sh`, on the host,
    which is where the patterns are.
  - **False positives in the AppImage.** In the Ubuntu libraries it carries
    (ICU, WebKit, glib, pango, librsvg), two short patterns matched 3-character
    fragments. The util-linux copyright matched the email of one of its
    developers. Comparing with the original libraries of the build image gave
    **identical counts**: the occurrences come from upstream. So, in
    third-party libraries the scan only looks for the home and the hostname,
    and the patterns are kept for our own files;
  - **Proof both ways:** the v0.1.0 packages pass clean, and the `.rpm` built
    on the host (with the home path) is refused.
- **Dev server (`vite.config.ts`).** Its root is the project folder: an
  exported profile or a swanctl.conf with a PSK left there would be served on
  `127.0.0.1:1420`. `server.fs.deny` now refuses `*.conf`, `*.key`,
  `trabalho*.json`, `presets.local/`, `vpn-*/` and `target/`, besides Vite's
  own patterns. Tested with fake files: 403 for them (also with `?raw` and
  `?import`) and 200 for the app.
- **Private files outside the project.** `trabalho.json`, the `vpn-docker/`
  copy (with the PSK) and the original plan left the project folder, readable
  only by the maintainer. The original `~/vpn-docker` was not touched.

### Step 35: v0.1.1 release (built, not published)

- Version 0.1.1 (`chore/versao-0.1.1`):
  - CHANGELOG and notes explaining how to update: the app **and** the network
    component;
  - `THIRD_PARTY_NOTICES.md` now records the modification to strongSwan (the
    ADR-0019 patch, with a date).
- Before the tag: `check-private.sh --history` and `--tracked` clean,
  gitleaks with no findings, `npm audit` with no vulnerabilities and
  cargo-deny with no advisories.
- **First tag build:** `verify-release.sh` approved the three packages (intact
  component, strongSwan with the patches, glibc 2.34). The new private data
  scan, however, flagged `usr/bin/ipsec-client`.
  - **Investigation**, without printing pattern or match: a short line of the
    patterns matched **3 isolated bytes in the `.text` section** (machine
    code), with no readable character next to them. The x86 instructions
    formed 3 letters by coincidence. False positive.
  - **Fix** (`fix/check-private-elf`): in ELF executables, the patterns apply
    only to the readable text (`strings -n 4`). Truly embedded data ends up in
    readable text. The home path and the hostname are still searched in the
    raw bytes.
  - **Check:** 0.1.1 and 0.1.0 clean, the host `.rpm` still refused. The
    second net (real values of the private profile, only counted) also found
    nothing in the 0.1.1 packages.
- The `v0.1.1` tag, not yet pushed, was moved to the merge with the fix, and
  the packages were built again from it.
- **v0.1.1 was never published.** Its fixes went into v0.2.0 (step 36).

### Step 36: English, bilingual app and v0.2.0 (`feat/i18n-core`, `feat/i18n-ui`, `docs/english`)

**The maintainer's requests:**

- replace the personal email in the commits before making the repository
  public;
- take advantage of that history rewrite (which needs a force push anyway) to
  switch the documentation and the app to English, with Portuguese as a
  second language, and ship it as the second version.

**Commit email.** The history was rewritten (`git filter-branch`) with the
GitHub noreply address as author and committer, the tags were recreated on
the new commits, and the repository's `user.email` now uses that address. A
bundle of the old history was kept outside the repository until the force
push is confirmed. The old commit messages stay in Portuguese; new ones are in
English.

**Maintainer's decisions:**

- the app follows the system language (Portuguese for `pt*` locales, English
  otherwise), and the language can be changed in System → Appearance;
- user documentation in both languages (English first, Portuguese in
  `docs/pt-BR/`); technical documentation in English only;
- the CLI, the helper and the scripts speak English;
- the next version is **0.2.0**, because the user-facing language changes.

**Rust (`ipsec_client_core::i18n`).** `Lang { En, Pt }`, chosen from the
locale (`LC_ALL` → `LC_MESSAGES` → `LANG`) or set by the app. The `tr!` and
`trf!` macros take the English and Portuguese texts side by side, so the two
cannot drift apart, and also work inside `thiserror` messages. Every message
the app shows (errors, failure diagnoses, diagnostics, connection steps,
tray) goes through them. Diagnostics checks got a stable `id`, so the app no
longer compares translated names. The CLI never sets the language: it always
speaks English. A separate integration test runs the core in Portuguese.

**Frontend (`src/i18n`).** No new dependency: an `en` dictionary and a `pt`
one with the same shape, enforced by TypeScript; a provider that picks the
system language (`navigator.languages`), remembers the choice in
`localStorage`, sets `<html lang>` and tells Rust through the new
`set_language` command, so errors and the tray menu follow the UI. Byte sizes
are formatted for the chosen language. The anonymized log placeholder for domains became
`<domain-N>` (it was `<dominio-N>`), since that text goes into bug reports.

**Scripts and packaging.** Messages in English in the installer, the engine
entrypoint and updown, the build, release and test scripts. The markers the
code waits for (`[engine] ready`, `[network component]`, `[test-gateway]
ready`) changed together with the code that reads them. The desktop entry
keeps the Portuguese name and keywords, so the menu search still works in
Portuguese.

**Documentation.** README, SECURITY, CONTRIBUTING, CHANGELOG, the
architecture, profiles, CLI, release and roadmap documents, the ADRs (with
English file names), this log and the resume guide (`docs/resume.md`) are in
English. The installation guide, the quick start, the README and the security
policy have Portuguese copies (`README.pt-BR.md` and `docs/pt-BR/`). The
v0.1.0 release notes stay in Portuguese, as they were published.

**Finding on the way:** the native E2E scenario 5 failed intermittently with
"charon did not log...". `docker logs | grep -q` under `pipefail`: `grep -q`
exits on the first match, the producer gets SIGPIPE, and the pipeline fails
even though the line was there. The same pattern in `check-private.sh` could
hide a match in an ELF file. Both now use `grep -q pattern < <(command)`, and
CONTRIBUTING records the rule.

**Scanner false positive in the 0.2.0 build.** The private data scan flagged
`engine.tar.gz` in the three packages. Checked without printing pattern or
match: one short pattern matched the **compressed bytes** of the tarball
only; the decompressed stream and the extracted files were clean, and the
home path and hostname were absent. gzip output is pseudo-random, so a short
case-insensitive pattern matches by chance (0.1.1 passed by luck). The
scanner now skips `.gz` files when applying the patterns, since their content
is already extracted next to them and scanned there. Checked both ways: the
0.2.0 packages pass, and a synthetic package with a marker inside the nested
`engine.tar.gz` is refused, pointing at the extracted file.

**Verification:** `npm run check` green; `test:native` (5 scenarios),
`test:systemd`, `test:packaging`, `test:engine`, `test:docker` and `test:cli`
green with the English scripts; vitest covers the Portuguese screen and the
language switch.

### Step 37: audit of 0.2.0, part 1 (credentials, private data, supply chain)

**The maintainer's request:** a new audit of the published 0.2.0 (security,
no credential leaks, optimization, compatibility, interface), plus lighter
base images, a better-looking interface and more IPsec modes. It became a
**program in phases, one per session** (roadmap, "Post-0.2.0 program").
Decisions: fix clear security, leak or compatibility flaws right away and
propose the rest; no release until the maintainer decides.

**Repository leftover.** A `git fetch` brought back a `v0.1.1` tag: it had
been pushed before the email rewrite, and a force push does not delete a
remote tag that is gone locally. It kept the old history, with the personal
email, reachable on the remote. Deleted locally; the maintainer deletes it on
the remote.

**Canary sweep.** New scenario in `test:systemd` (real systemd, helper and
polkit): connect to the test gateway and look for the test PSK, password and
username in every regular file, process command line and environment, the
journal, the unit properties, the CLI output and the exported profile. The
patterns live in files under `/dev`, which the sweep skips, so that `grep`'s
own command line does not match.

- PSK and password: **nowhere**, while connected and after disconnecting.
- Username: the sweep failed on `ipsec-client-cli logs`. The masking only
  replaced identities the process knew, and the username comes from the
  credentials: without them in the keyring, another process showed it raw,
  including in "Copy anonymized".
- No core dump limit anywhere.

**Fixes:**

- `fix/mask-unknown-identities`: identities are masked by the shape of the
  charon line as well. The strongSwan source prints identities quoted (184
  formats), in brackets after an address (26) and, at log level 2, as
  `xauth_id = value`; those three shapes are masked, and quoted values that
  are not identities (plugin, config, interface, file) are left alone. The
  anonymizer applies it after the known replacements and before the
  addresses.
- `fix/no-core-dumps`: `RLIMIT_CORE=0` in the app and the CLI (inherited by
  the webview processes), `LimitCORE=0` in the engine and helper units.

**Supply chain:** `npm audit` and `cargo deny` clean; Actions by SHA, images
by digest, strongSwan by SHA-256 + GPG; gitleaks and the private data guard
clean; shipped binaries with PIE, full RELRO and non-executable stack.
strongSwan 6.1.0 is the latest release and fixes the September 2026 CVEs.

**Strategic finding:** the 6.1.0 announcement says IKEv1 is now disabled by
default and will be removed in a future release. The embedded build enables
it, so nothing breaks today, but every supported profile is IKEv1. Recorded
in the audit report (P1) and in SECURITY.md as an open decision.

**Report:** [audits/v0.2.0.md](audits/v0.2.0.md), with the fixed, proposed
and accepted findings and what was checked.

### Step 38: audit of 0.2.0, part 2 (privilege boundaries)

**Scope:** every place where a less trusted side talks to a more trusted
one, as listed in the roadmap: user → helper, sandbox, VICI → charon, gateway
→ client, webview → Rust, installer, files and scripts, CI.

**Method.** Static reading of the protocol, the helper, the units, the engine
scripts, the installer, the 18 Tauri commands and the CI. The strongSwan
6.1.0 source (same SHA-256 as the build) was searched for every VICI option
that takes a path or a command, every process execution and every place where
text from the peer is logged. `systemd-analyze security` on both units.

**What was found and fixed** (`fix/helper-boundary`, `fix/untrusted-input`,
`fix/install-in-place`):

- **B1, interface chosen by the client.** The entrypoint deletes the
  interface it is given before creating it, and the helper accepted any name:
  a user could have root delete `docker0` or a bridge. The protocol now takes
  only `ipsc<N>`, and the entrypoint leaves alone a link that is not XFRM.
- **B2, the log of someone else's session.** `journalctl --unit` returns the
  last lines of the unit, whatever the session. The lines are now selected by
  `_SYSTEMD_INVOCATION_ID`, read in the same `systemctl show` as the state
  and the owner. A startup failure uses `--since` (the invocation ID is gone
  with the unit).
- **B3, slow clients.** The read timeout was per read. Now a deadline for the
  whole line, and four slots per uid.
- **B4, pid reuse before polkit.** The helper checks the uid of the pid
  between two reads of its start time.
- **B5, helper unit:** syscall filter, `ProtectKernelLogs`, `UMask`. Score
  3.6 → 2.1.
- **B6, gateway text in the terminal.** `xauth_generic.c` logs the gateway's
  XAuth message with `%.*s`. `printable()` in the core replaces control
  characters and bidirectional overrides in every log line.
- **B7, installer run from a user directory.** With a root-owned bundle
  (`.rpm`/`.deb`), `pkexec` runs the installer in place.
- **B8/B9:** `IPSEC_CLIENT_HELPER_SOCKET` only in debug builds; imported
  files read with a real limit, regular files only.

**Two things the tests taught.**

- The "idle clients" scenario of `test:systemd` had never held a connection:
  the `helper` function of the script passed only the JSON on and dropped
  `--hold`. It passed because nothing was being tested.
- Once it really held connections, it hung: the first version of the per-uid
  slots built the guard inside `bool::then_some`, which evaluates its argument
  even when the answer is "no". Dropping that guard locked the mutex that was
  still held. Fixed with `then(|| ...)` after releasing the lock, and covered
  by a unit test.

**Not changed, and why.** `charon.initiator_only` would stop charon from
answering IKE negotiations started by strangers, but it also drops a phase 1
rekey started by the gateway: it needs a test on the real VPN (P8). The VICI
`cert.file` option lets the session owner probe whether a file exists (P9);
it goes with the next engine rebuild. The rest is in the report.

**Verification:** `npm run check`, `test:systemd` (with the new scenarios),
`test:engine`, `test:native`, `test:packaging`, `test:docker` and `test:cli`.

**Report:** [audits/v0.2.0.md](audits/v0.2.0.md#part-2-privilege-boundaries).

### Step 39: audit of 0.2.0, part 3 (compatibility across distributions)

**Also asked by the maintainer:** a step-by-step guide for the P8 test (now
in the report, "How to test P8 on the real VPN") and the end of the 40-odd
branches on the remote. CONTRIBUTING now says to delete a branch after its
merge; every local branch merged into `main` was deleted, and the command
that deletes them on GitHub is in resume.md (this environment cannot push).

**Two harnesses** (`test/compat-matrix`):

- `tests/compat/run.sh` (`npm run test:compat`) runs `tests/systemd` on
  Ubuntu 22.04 and 24.04, Debian 12 and 13, Fedora 45, openSUSE Tumbleweed
  and AlmaLinux 10. The Dockerfile takes the base image as an argument and picks
  the package manager it finds. The helper and the CLI are built once in the
  release image, so they have the glibc floor of the published packages.
- `tests/compat/packages.sh` installs a `.deb`/`.rpm` with each package
  manager in a clean container, optionally updating from an older one, and
  checks libraries (`ldd`), the helper and charon starting, the bundle hashes
  and every tool the app and the engine call.

**What the matrix found:**

- The published 0.2.0 packages install everywhere, but a clean system lacks
  `pkexec` and `ip`; Fedora and openSUSE also lack `systemctl`,
  `systemd-run`, `journalctl`, `find` and `awk`. Desktop installations
  usually have them, which is why nobody noticed. Declared now (Tauri
  `depends`); on openSUSE `pkexec` is its own package, so the `.rpm` asks for
  the file `/usr/bin/pkexec`.
- Image-based systems: the installer wrote the policy to a read-only
  `/usr/share/polkit-1/actions`. polkit 124+ also reads
  `/etc/polkit-1/actions`; `pkaction --version` decides. While testing it, a
  second bug: with `/opt` as a link to `var/opt` (Fedora Atomic), GNU tar
  replaced the link with a directory and the files installed through the
  link vanished. `--keep-directory-symlink` fixes it; the installer also
  refuses links inside the engine tarball now that tar keeps them.
- Without systemd-resolved, the diagnostics said "active": the files in
  `/run/systemd/resolve` (the socket included) stay after it stops. The
  first fix connected to the Varlink socket, which passed on Debian and
  Ubuntu and failed on Fedora 45 and openSUSE: there (systemd 261/262) the
  socket belongs to systemd, which accepts the connection of a masked
  service and then resets it. The check now makes a Varlink call
  (`org.varlink.service.GetInfo`) and requires any reply; systemd 249
  answers "MethodNotImplemented", which counts.
- The helper unit runs with an empty `CapabilityBoundingSet` on all seven
  (audit P12), and the last scenario of `tests/systemd` swaps the uid list
  for the real polkit: the request of the helper works on polkit 0.105 to
  128. Until now, real polkit had only been exercised by hand on Fedora.
- RHEL family: the installation guide recommended the `.rpm` for RHEL, Rocky
  and AlmaLinux. On AlmaLinux 9 it does not install (no WebKitGTK 4.1, even
  with EPEL), and the AppImage needs glibc 2.35 (EL9 has 2.34) because of
  the WebKitGTK it carries. On AlmaLinux 10 it installs with EPEL + CRB and
  passes the whole systemd test (polkit 125), so it became the seventh
  distribution of the matrix; `packages.sh` enables EPEL for that family.
  `verify-release.sh` only looked at the app's own glibc; it now reads every
  ELF file in the AppImage.

**Test harness lessons.** The Ubuntu 24.04 image has a user on uid 1000, and
`pkaction` 0.105 exits with 1 even when it finds the action: the output
decides now. A release build from `HEAD` used to write over
`target/release-artifacts/0.2.0`, the published packages; it goes to
`<version>-dev-<commit>` now.

**Not testable here:** Wayland/X11, the tray, the AppImage on Ubuntu 24.04
with AppArmor's user namespace restriction, NVIDIA. They are in the report
as checks for the maintainer (P15).

**Verification:** `npm run check`, `test:packaging`, `test:compat` (seven
distributions), `tests/compat/packages.sh` with the 0.2.0 packages (from
0.1.0, six distributions) and with test packages of this branch (from 0.1.1,
seven), `test:cli`.

**Report:** [audits/v0.2.0.md](audits/v0.2.0.md#part-3-compatibility).

### Step 40: phase 4, optimization and base images (ADR-0020)

**The maintainer's decisions at the start:** the Docker engine leaves release
builds; the next release is 0.3.0; P9, P10 and P11 of the audit go in.

**P8 results from the maintainer.** On 2026-11-17 the maintainer ran the
steps: no `scheduling re…` line in the last 500 log lines, `0` negotiations
started by the gateway in three days, and the option was set and removed
right away. Not conclusive: the count only means something after a session
longer than the gateway's phase 1 lifetime, and step 2 needs a session that
long with the option on. P8 stays open; the release does not carry it.

**Measured first** (sizes and times are in ADR-0020): the engine builder,
the engine tarball, the binaries, the dependency trees, the images, the
frontend bundle.

**Static engine with musl** (`feat/engine-musl`):

- Alpine 3.24 builder; charon links musl and GMP statically and comes out as
  a static-pie executable. Alpine ships no license texts, so the GMP and musl
  ones were taken from their upstream tarballs (the first GMP download came
  back truncated and failed its signature, as it should; the GNU mirror gave
  the right file) and committed with their provenance.
- The stack protector cannot be seen with `strings` in a stripped static
  binary; counting canary reads (`%fs:0x28`) in the disassembly showed it
  (2728 against 2661 in the glibc build). The flags are now explicit.
- Patch `0002` (P9) refuses certificate paths and tokens in forced mode; a
  `test:systemd` scenario checks it. Running it against the old engine was
  not possible: staging refuses an engine without the patches of the commit,
  which is the guard that keeps a release from shipping without it.
- P7: `swanctl` goes to a tools tarball; the tests take it from there.
- Name resolution: `tests/systemd` names the gateway by host name, served to
  systemd-resolved by a 50-line DNS server in Python. The first run hung: the
  slow-client scenario `wait`s for its background jobs, and the DNS server
  was one of them. It runs as a transient unit now.

**P10 and P11** (`fix/helper-if-id-gateway-route`): the helper uses one
fixed `if_id`; the `updown` adds a host route to the gateway when the remote
network contains it, tested offline with fake `ip`/`resolvectl`
(`npm run test:updown`, in `npm run check` and CI), and the test fails with
the previous `updown`.

**No Docker engine in release builds** (`feat/release-without-docker`): the
`docker` feature, off by default. App 10.5 → 9.3 MB, CLI 5.0 → 3.9 MB and 159
→ 102 crates. Clippy and the tests run in both builds. Then `lto = "fat"`:
app 8.6 MB, CLI 3.7 MB, helper 604 KB.

**Debian 13 images:** the Docker engine and the test gateway moved off
Ubuntu 20.04. The image grew (173 → 209 MB, systemd 257 for `resolvectl`),
which is recorded as such. The failure diagnosis signatures still match on
strongSwan 6.0.1.

**Verification:** `npm run check`, `test:packaging`, `test:native`,
`test:systemd`, `test:engine`, `test:docker`, `test:cli` and `test:compat`
(seven distributions).

### Step 41: phase 5a, interface review and visual directions

**Tooling** (`scripts/ui-review/`, kept for 5b): the real app built by Vite
with `mockIPC` fixtures (RFC 5737/2606 data) and scenarios chosen by URL
(first use, idle, credentials, connecting, connected, error, outdated
component, profiles, profile details, system), screenshotted by headless
Firefox in a throwaway profile: 10 scenarios × English/Portuguese ×
light/dark, plus the minimum width. A tiny server holds the page's `load`
event until the scenario has clicked through, because Firefox takes the
screenshot on `load`. Tailwind 4 only scans the Vite root, so the review
stylesheet adds `@source` for `src/`. Output in `target/ui-review/`, never
published.

**What the review found** (fixed in 5b whatever the direction):

- contrast in the light theme: white on the primary button 4.28:1,
  "Preparing…" in amber 2.18:1, "Connected" in green 3.26:1 (the dark theme
  passes everything);
- no visible keyboard focus on the hand-made buttons (tabs, profile
  expander, error dismiss, show password);
- the dialog close button says "Fechar" in English too; diagnostic icons are
  read as "ok/warn/fail" in Portuguese;
- animations ignore "reduce motion";
- "IPsec Client" wraps to two lines in the header at 440 and 380 px;
- the error message mixes what happened with what to do, in one long
  technical block;
- first use has two competing primary actions.

**Directions** (gallery `target/ui-review/gallery/index.html`): A, native
GNOME (libadwaita look); B, the whole screen colored by the connection state,
with big numbers and tabs at the bottom; C, compact and technical, with the
log always visible. Recommended: B. Waiting for the maintainer's choice.
