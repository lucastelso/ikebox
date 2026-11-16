# ADR-0012: Presets locais não são embutidos no binário

- **Status:** Aceito (substitui o trecho do ADR-0008 sobre embutir
  `presets.local/` no build)
- **Data:** 2026-11-16

## Contexto

O ADR-0008 previa que `presets.local/`, ignorado pelo git e com os perfis
reais da organização do usuário, seria embutido no binário quando existisse no
momento do build. Assim, um build local já viria com o perfil do trabalho.

O problema é que qualquer binário ou pacote compilado localmente (rpm, deb,
AppImage) passaria a carregar gateway, id do grupo, redes e DNS internos. Esse
binário pode ser publicado por engano numa release, anexado a uma issue ou
enviado a alguém de fora. O dado privado sairia sem que ninguém o visse no
diff. É exatamente o que o ADR-0010 quer impedir.

## Decisão

- O binário embute só os exemplos genéricos de `presets/`, quando isso for
  útil, nunca `presets.local/`.
- Perfis reais chegam ao app por **importação de arquivo** (JSON ou, no futuro,
  `swanctl.conf`). Para distribuir à equipe, o caminho é **exportar** o perfil,
  que não contém credenciais, e enviá-lo por um canal privado.
- `presets.local/` continua no `.gitignore`, como lugar conveniente para os
  arquivos reais antes de importá-los.

## Consequências

- Nenhum artefato de build contém dados da organização.
- Um passo a mais para o usuário: importar o perfil uma vez. Na UI, isso é um
  botão.

## Alternativas consideradas

- **Embutir com um aviso.** Depende de disciplina a cada release, e um único
  descuido já vaza os dados.
- **Embutir só em builds de debug.** Builds de debug também circulam.
