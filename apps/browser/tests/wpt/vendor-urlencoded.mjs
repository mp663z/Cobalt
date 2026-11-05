// Run with `node apps/browser/tests/wpt/vendor-urlencoded.mjs` after updating
// the pinned WPT source. Only UTF-8 string entries are representable by the
// browser's bounded single-line text/hidden/choice form model. File entries,
// formdata events, non-UTF-8 charsets and lone surrogates are out of scope.
import { readFileSync, writeFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const directory = new URL('.', import.meta.url);
const source = readFileSync(new URL('urlencoded2.window.js', directory), 'utf8');
const cases = [];
runInNewContext(source, {
  formSubmissionTemplate: () => entry => cases.push(entry),
  File: class File {},
});
const lines = [
  '# WPT html/semantics/forms/form-submission-0/urlencoded2.window.js',
  '# UTF-8 string entries only. Columns: description, name, value, expected; UTF-8 hex.',
];
for (const entry of cases) {
  if (typeof entry.value !== 'string' || entry.formEncoding && entry.formEncoding !== 'utf-8') continue;
  if ([entry.name, entry.value].some(value => /[\uD800-\uDFFF]/u.test(value))) continue;
  lines.push([entry.description, entry.name, entry.value, entry.expected]
    .map(value => Buffer.from(value, 'utf8').toString('hex')).join('\t'));
}
writeFileSync(new URL('urlencoded2.tsv', directory), `${lines.join('\n')}\n`);
console.log(`${lines.length - 2} cases`);
