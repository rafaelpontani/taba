// Applies a language's translation table to a diagram source (source/*.json is English, en-US).
// Usage: node translate.mjs <source.json> <output.json> <lang> [table.json]
// The table is flat: { "English string": "translation" }. Only the display keys are translated
// (never ids, types or references); a string missing from the table stays in English and is reported.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname } from 'node:path';

const [source, output, lang, tablePath] = process.argv.slice(2);
if (!source || !output || !lang) {
  console.error('Usage: node translate.mjs <source.json> <output.json> <lang> [table.json]');
  process.exit(2);
}

const DISPLAY_KEYS = new Set(['title', 'label', 'sublabel', 'note', 'items', 'tag']);
const table = lang === 'en-US' || !tablePath ? {} : JSON.parse(readFileSync(tablePath, 'utf8'));
const missing = new Set();

// A string that is only an identifier-like token (ids, ports, file names) needs no translation.
const noText = (s) => !/[A-Za-z]{3,}/.test(s) || /^[\w./:<>*{}\-]+$/.test(s);

function walk(v, key) {
  if (Array.isArray(v)) return v.map((x) => walk(x, key));
  if (v && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(x, k)]));
  if (typeof v === 'string' && DISPLAY_KEYS.has(key) && lang !== 'en-US') {
    if (table[v] !== undefined) return table[v];
    if (!noText(v)) missing.add(v);
  }
  return v;
}

const input = JSON.parse(readFileSync(source, 'utf8'));
mkdirSync(dirname(output), { recursive: true });
writeFileSync(output, JSON.stringify(walk(input, ''), null, 2) + '\n');
if (missing.size) console.error(`  [${lang}] ${missing.size} string(s) not in the table (kept in English): ${[...missing].slice(0, 3).join(' | ')}${missing.size > 3 ? ' ...' : ''}`);
