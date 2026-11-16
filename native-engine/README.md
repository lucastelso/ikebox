# Engine embutido (strongSwan compilado)

O app leva o próprio strongSwan, compilado a partir do código-fonte oficial,
e por isso o usuário não precisa de Docker (ADR-0013 e ADR-0016).

| Arquivo | Função |
|---|---|
| `versions.txt` | Versões suportadas, cada uma com caminho e **SHA-256** do tarball |
| `keys/strongswan-release.asc` | Chave pública de release do strongSwan (para conferir a assinatura GPG) |
| `build/Dockerfile` | Builder Ubuntu 22.04 (glibc 2.35, a mesma base do AppImage) |
| `patches/*.patch` | Patches de segurança do IPsec Client, aplicados **depois** da conferência do código-fonte. Hoje há um só: o cliente VICI não escolhe o `updown` (executado como root) nem o `if_id` (ADR-0019) |
| `build/build-strongswan.sh` | Baixa o tarball, confere SHA-256 e GPG, aplica os patches (todos precisam aplicar), compila (estático, monolítico, sem OpenSSL) e gera um tarball determinístico |
| `build.sh` | Roda o builder: `native-engine/build.sh [versão\|all]` |
| `spike/` | Teste do engine compilado contra o gateway de teste, num "host" Fedora 45 simulado |

```bash
npm run build:engine     # compila a versão padrão (a primeira de versions.txt)
npm run test:native      # E2E completo (5 cenários, inclusive o ataque do ADR-0019)
npm run test:systemd     # systemd de verdade em container: instalador, helper, sandbox, 2 usuários
native-engine/build.sh all && native-engine/spike/run-e2e.sh 5.8.2   # reserva
```

O resultado vai para `native-engine/dist/` (ignorado pelo git):
`ipsec-client-engine-<versão>-x86_64.tar.gz`, com o charon, o swanctl, a
configuração e um `BUILDINFO` (versão, hash, patches com SHA-256, opções de
build, dependências). O empacotamento (`scripts/stage-network-component.sh`),
o build de release e o `verify-release.sh` recusam um engine sem os patches do
commit. Ao trocar a versão do strongSwan, porte o patch se ele não aplicar; as
versões de reserva 5.x não têm o patch portado.
O charon depende **só da glibc**.

Plugins incluídos: kernel-netlink, socket-default, vici, updown,
xauth-generic, unity, nonce, random, drbg, aes, des, sha1, sha2, md5, hmac,
gcm, chapoly, curve25519, gmp (modp, inclusive 768/1024), pem, pkcs1, pkcs8,
x509 e pubkey. O kdf entra nas versões 6.x. Sem OpenSSL, **não há curvas
ECP**.
