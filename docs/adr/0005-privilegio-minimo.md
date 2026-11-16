# ADR-0005: Privilégio mínimo no container

- **Status:** Aceito. No engine embutido, o sandbox é do systemd (ADR-0014).
- **Data:** 2026-11-15

## Contexto

O fluxo manual rodava o container com `--privileged`, `--cap-add SYS_MODULE` e
`/lib/modules` montado. Com isso, o container tinha acesso total ao host:
dispositivos, módulos do kernel e todas as capabilities.

## Decisão

- O container recebe só o necessário:
  - `--network host`, porque o XFRM e as rotas precisam estar no namespace do
    host;
  - `--cap-add NET_ADMIN`, para netlink XFRM, rotas, endereços e interfaces;
  - as capabilities padrão do Docker, que incluem `NET_BIND_SERVICE` para as
    portas 500/4500;
  - bind de `/run/dbus/system_bus_socket`, para o `resolvectl` falar com o
    systemd-resolved do host;
  - bind do diretório do socket VICI (`$XDG_RUNTIME_DIR/ipsec-client`).
- `--security-opt label=disable` só quando o host usa SELinux, e
  `apparmor=unconfined` só quando usa AppArmor. Sem isso, o acesso ao socket
  D-Bus e ao bind do socket VICI é bloqueado.
- **Sem** `SYS_MODULE` e sem `/lib/modules`. O módulo `xfrm_interface` é
  carregado automaticamente pelo kernel quando a interface é criada.
- Existe um "modo privilegiado" (`--privileged`) só como reserva, desligado
  por padrão. Ele aparece nas configurações avançadas, com aviso.

## Consequências

- Se o charon for comprometido, o estrago fica limitado à rede do host. O
  invasor não ganha acesso a disco, dispositivos ou módulos.
- Em hosts com políticas de segurança diferentes, pode ser preciso o modo de
  reserva. O diagnóstico ajuda a identificar esses casos.

## Alternativas consideradas

- **`--privileged` sempre (fluxo manual).** Superfície de ataque
  desnecessária.
- **User namespaces / rootless.** O XFRM no namespace do host exige
  `CAP_NET_ADMIN` no namespace inicial.
