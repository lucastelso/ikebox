# ADR-0015: Distribuição em AppImage, rpm e deb (sem Flatpak)

- **Status:** Aceito
- **Data:** 2026-11-16

## Contexto

O usuário não deve precisar instalar nada além do app: nem Docker, nem
Node/npm, nem Rust.

Node, npm e Rust são ferramentas de **build**. O Tauri compila o frontend
(HTML/JS/CSS) para dentro do binário, então o usuário nunca vê o npm.

O app precisa de um componente com privilégio no host (ADR-0014).

## Decisão

- Formatos, gerados pelo bundler do Tauri na CI (Ubuntu 22.04, pela
  compatibilidade de glibc):
  - **AppImage**: baixe, dê permissão de execução e abra. Leva o WebKitGTK. No
    primeiro uso, instala o componente de rede com a senha de administrador,
    uma vez;
  - **rpm** (Fedora/openSUSE) e **deb** (Debian/Ubuntu): trazem o helper, a
    unit do systemd e a policy do polkit.
- Releases no GitHub, com `SHA256SUMS` assinado. Atualização automática
  assinada fica como opcional (Tauri updater, que suporta AppImage).
- **Sem Flatpak.**

## Consequências

- Um único download funciona na maioria das distribuições com glibc ≥ 2.35.
- O AppImage é grande, porque leva o WebKitGTK (cerca de 80–100 MB).

## Alternativas consideradas

- **Flatpak.** O sandbox impede netlink/XFRM e serviços com privilégio no host.
  Contornar exige `flatpak-spawn --host`, que anula o sandbox, e o Flathub
  recusa apps assim. É o mesmo motivo de os plugins de VPN do NetworkManager
  não existirem em Flatpak.
- **Snap.** Tem o mesmo problema de confinamento e menos adoção no público-alvo
  (Fedora).
- **Frontend num container.** Não resolve nada (o frontend já vai compilado no
  binário) e traria o Docker de volta.
