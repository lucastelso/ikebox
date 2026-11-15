# ADR-0009: IPC tipado entre UI e Rust com tauri-specta

- **Status:** Aceito
- **Data:** 2026-11-15

## Contexto

A GUI (React/TypeScript) chama comandos Rust e recebe eventos (estado da
sessão, logs). Com strings soltas e tipos duplicados à mão, os dois lados
divergem sem ninguém perceber.

## Decisão

- A GUI usa Tauri v2. Os comandos ficam em `src-tauri` e são finos: delegam
  para `crates/core`.
- O `tauri-specta` gera os bindings TypeScript dos comandos e eventos a partir
  dos tipos Rust. A versão fica fixada exatamente, porque a v2 ainda está em RC.
- O CLI (`crates/cli`) usa o mesmo `core`. Toda lógica de negócio fica fora da
  camada de UI.

## Consequências

- Mudar um tipo no Rust quebra a compilação do TypeScript. É o comportamento
  desejado.
- Depender de uma versão RC é um risco. Por isso a versão é fixada, e uma
  atualização vira um PR isolado.

## Alternativas consideradas

- **`invoke` com tipos escritos à mão.** Divergem com o tempo.
- **ts-rs.** Gera os tipos, mas não os comandos e eventos.
