// Applies a language's translation table to a diagram source (fonte/*.json is English).
// Usage: node traduzir.mjs <fonte.json> <saida.json> <lang> [tabela.json]
// The table is flat: { "English string": "translation" }. Only the display keys are translated
// (never ids, types or references); a string missing from the table stays in English and is reported.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname } from 'node:path';

const [fonte, saida, lang, tabelaPath] = process.argv.slice(2);
if (!fonte || !saida || !lang) {
  console.error('Usage: node traduzir.mjs <fonte.json> <saida.json> <lang> [tabela.json]');
  process.exit(2);
}

const CHAVES = new Set(['title', 'label', 'sublabel', 'note', 'items', 'tag']);
const tabela = lang === 'en' || !tabelaPath ? {} : JSON.parse(readFileSync(tabelaPath, 'utf8'));
const faltando = new Set();

// A string that is only an identifier-like token (ids, ports, file names) needs no translation.
const semTexto = (s) => !/[A-Za-z]{3,}/.test(s) || /^[\w./:<>*{}\-]+$/.test(s);

function percorrer(v, chave) {
  if (Array.isArray(v)) return v.map((x) => percorrer(x, chave));
  if (v && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, percorrer(x, k)]));
  if (typeof v === 'string' && CHAVES.has(chave) && lang !== 'en') {
    if (tabela[v] !== undefined) return tabela[v];
    if (!semTexto(v)) faltando.add(v);
  }
  return v;
}

const origem = JSON.parse(readFileSync(fonte, 'utf8'));
mkdirSync(dirname(saida), { recursive: true });
writeFileSync(saida, JSON.stringify(percorrer(origem, ''), null, 2) + '\n');
if (faltando.size) console.error(`  [${lang}] ${faltando.size} string(s) not in the table (kept in English): ${[...faltando].slice(0, 3).join(' | ')}${faltando.size > 3 ? ' ...' : ''}`);
