/**
 * Conventional Commits, com mensagens em pt-BR.
 * @see https://commitlint.js.org
 * @type {import('@commitlint/types').UserConfig}
 */
export default {
  extends: ['@commitlint/config-conventional'],
  rules: {
    // Siglas (ADR, VICI, XFRM) e nomes próprios no assunto são comuns em pt-BR.
    'subject-case': [0],
    'header-max-length': [2, 'always', 100],
    'body-max-line-length': [1, 'always', 100],
    'footer-max-line-length': [0],
  },
};
