import { RuleTester } from 'eslint';
import tseslint from 'typescript-eslint';
import { describe, it } from 'vitest';
import rule from './no-literal-ui-text.js';

RuleTester.describe = describe;
RuleTester.it = it;

const tester = new RuleTester({
  languageOptions: {
    parser: tseslint.parser,
    parserOptions: { ecmaFeatures: { jsx: true } },
  },
});

tester.run('no-literal-ui-text', rule, {
  valid: [
    '<p>{t("welcome-title")}</p>',
    '<p title={t("hint")}>{count} · 42%</p>',
    '<MotionConfig reducedMotion="user" />',
    '<div className="welcome" id="main" />',
    '<p>\n  {text}\n</p>',
  ],
  invalid: [
    { code: '<p>Hello world</p>', errors: [{ messageId: 'literal' }] },
    { code: '<p>Ciao è</p>', errors: [{ messageId: 'literal' }] },
    { code: '<button aria-label="Close" />', errors: [{ messageId: 'literal' }] },
    { code: '<img alt={"A photo"} />', errors: [{ messageId: 'literal' }] },
    { code: '<input placeholder={`Search ${x}`} />', errors: [{ messageId: 'literal' }] },
  ],
});
