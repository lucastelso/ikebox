# Release signing key

`ikebox-release.asc` is the public key that signs the releases
([ADR-0030](../../docs/adr/0030-release-signing-and-provenance.md)): the
`.rpm` packages and `SHA256SUMS` (`SHA256SUMS.asc`). How it was made and how
to check a download: [docs/release.md](../../docs/release.md#signing-key-once)
and [docs/installation.md](../../docs/installation.md#check-the-download).

Fingerprint of the primary key (certification only, kept off-line):

```
6F3F 99EA 25FA F76F CF98  655C B357 4751 2185 8FC5
```

It signs through an Ed25519 subkey (`195A 22CA 60B6 974F AB6B  20A5 ABCD D98A
70AF 682D`), the only part CI holds. The fingerprint is also on the README
and on every release page, so a key swapped here would not go unnoticed.
