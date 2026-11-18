# Release signing key

`ipsec-client-release.asc` is the public key that signs the releases
([ADR-0030](../../docs/adr/0030-release-signing-and-provenance.md)): the
`.rpm` packages and `SHA256SUMS` (`SHA256SUMS.asc`). How it was made and how
to check a download: [docs/release.md](../../docs/release.md#signing-key-once)
and [docs/installation.md](../../docs/installation.md#check-the-download).

Its fingerprint is also on the README and on every release page, so a key
swapped here would not go unnoticed.
