/**
 * Flags user-facing text written directly in JSX: text between tags and string values
 * of labelling attributes. Such text belongs in the catalogs in locales/ (principle A5).
 */

const LABEL_ATTRIBUTES = new Set([
  'alt',
  'aria-description',
  'aria-label',
  'aria-placeholder',
  'aria-roledescription',
  'aria-valuetext',
  'label',
  'placeholder',
  'title',
]);

/** Text with at least one letter; punctuation, symbols and numbers alone are fine. */
const hasWords = (text) => /\p{L}/u.test(text);

/** @type {import('eslint').Rule.RuleModule} */
export default {
  meta: {
    type: 'problem',
    docs: { description: 'Disallow user-facing text written directly in JSX' },
    messages: {
      literal: 'User-facing text "{{text}}" must come from the locale catalogs (use t()).',
    },
    schema: [],
  },
  create(context) {
    const report = (node, text) =>
      context.report({ node, messageId: 'literal', data: { text: text.trim() } });

    return {
      JSXText(node) {
        if (hasWords(node.value)) report(node, node.value);
      },
      JSXAttribute(node) {
        const name = typeof node.name.name === 'string' ? node.name.name : '';
        if (!LABEL_ATTRIBUTES.has(name) || !node.value) return;
        const value =
          node.value.type === 'JSXExpressionContainer' ? node.value.expression : node.value;
        if (value.type === 'Literal' && typeof value.value === 'string' && hasWords(value.value)) {
          report(value, value.value);
        }
        if (value.type === 'TemplateLiteral' && value.quasis.some((q) => hasWords(q.value.raw))) {
          report(value, context.sourceCode.getText(value));
        }
      },
    };
  },
};
