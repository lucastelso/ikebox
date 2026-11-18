/**
 * Conventional Commits, messages in English (the commits before 0.2.0 are in Portuguese).
 * @see https://commitlint.js.org
 * @type {import('@commitlint/types').UserConfig}
 */
export default {
  extends: ['@commitlint/config-conventional'],
  rules: {
    // Acronyms (ADR, VICI, XFRM) and proper names are common in subjects.
    'subject-case': [0],
    'header-max-length': [2, 'always', 100],
    'body-max-line-length': [1, 'always', 100],
    'footer-max-line-length': [0],
  },
};
