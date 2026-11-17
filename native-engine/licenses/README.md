# Licenses of what the engine links statically

The embedded engine is one static executable. Besides strongSwan (whose
license texts come from its own source tarball at build time), it contains:

| Component | Version | License | Files here |
|---|---|---|---|
| GNU MP (GMP) | 6.3.0, Alpine package `gmp-static` | LGPL v3 or later, or GPL v2 or later (dual) | `gmp/COPYING.LESSERv3`, `gmp/COPYINGv2`, `gmp/COPYINGv3` |
| musl libc | 1.2.6, Alpine package `musl-dev` | MIT | `musl/COPYRIGHT` |

The texts were taken from the upstream source tarballs of those versions,
checked on 2026-11-17:

- `gmp-6.3.0.tar.xz`, SHA-256
  `a3c2b80201b89e68616f4ad30bc66aee4927c3ce50e33929ca819d5c43538898`, good
  signature from key `343C2FF0FBEE5EC2EDBEF399F3599FF828C67298` (Niels Möller);
- `musl-1.2.6.tar.gz`, SHA-256
  `d585fd3b613c66151fc3249e8ed44f77020cb5e6c1e635a616d3f9f82460512a`, good
  signature from key `836489290BB6B70F99FFDA0556BCDB593020450F` (musl libc).

`native-engine/build/build-strongswan.sh` copies them into the engine bundle
(`share/licenses/`). When the builder image moves to other versions of GMP or
musl, update these files and this table.
