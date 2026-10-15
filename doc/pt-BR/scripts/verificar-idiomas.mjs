#!/usr/bin/env node
// Confere que as pastas de idioma de `doc/` dizem a mesma coisa (paridade, não tradução).
//
// Uso (da raiz do repositório):  node doc/pt-BR/scripts/verificar-idiomas.mjs
// Saída: um relatório por verificação e código de saída 1 se algo falhar.
//
// O que confere, para cada par de arquivos de mesmo prefixo numérico (`NN-`):
//   1. todo documento existe em todas as pastas de idioma (README, llms.txt e _*.md também);
//   2. os metadados do topo têm as chaves do idioma (pt-BR: título, resumo, ...; en-US: title, summary, ...);
//   3. os links entre arquivos e as âncoras resolvem dentro da própria pasta;
//   4. os conjuntos de códigos `UC-…`, `D…` e `PRB-…` citados são iguais nos dois lados;
//   5. o mesmo número de tabelas, de blocos de código e de imagens.
// Regra do projeto: toda mudança na documentação vale para todas as pastas de idioma (AGENTS.md).

import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')), '../../..');
const docDir = path.join(root, 'doc');
const read = (p) => fs.readFileSync(p, 'utf8').split('\r').join('');

// idioma → chaves obrigatórias dos metadados
const KEYS = {
  'pt-BR': ['título', 'resumo', 'tipo', 'status', 'verificado-em', 'fontes'],
  'en-US': ['title', 'summary', 'type', 'status', 'verified-on', 'sources'],
};
const langs = fs.readdirSync(docDir, { withFileTypes: true }).filter((e) => e.isDirectory() && KEYS[e.name]).map((e) => e.name).sort();
if (langs.length < 2) { console.log('menos de duas pastas de idioma conhecidas: nada a conferir'); process.exit(0); }

const idOf = (f) => (f.match(/^(\d+[a-z]?)-/) || [])[1] || (f.startsWith('_div') ? '_divergencias' : f.replace(/\.[a-z]+$/, ''));
const files = {};
for (const l of langs) {
  files[l] = {};
  for (const f of fs.readdirSync(path.join(docDir, l))) if (f.endsWith('.md') || f === 'llms.txt') files[l][idOf(f)] = f;
}

let failures = 0;
const report = [];
function check(name, problems) {
  if (problems.length) {
    failures += problems.length;
    report.push(`✗ ${name}: ${problems.length} problema(s)`);
    for (const p of problems.slice(0, 40)) report.push(`    - ${p}`);
    if (problems.length > 40) report.push(`    … e mais ${problems.length - 40}`);
  } else report.push(`✓ ${name}`);
}

// ---- 1. paridade de arquivos ----
{
  const problems = [];
  const ids = new Set(langs.flatMap((l) => Object.keys(files[l])));
  for (const id of [...ids].sort()) for (const l of langs) if (!files[l][id]) problems.push(`${l}: falta o documento "${id}" (existe em ${langs.filter((x) => files[x][id]).join(', ')})`);
  check(`paridade de arquivos entre ${langs.join(' e ')} (${ids.size} documentos)`, problems);
}

const text = (l, id) => (files[l][id] ? read(path.join(docDir, l, files[l][id])) : '');

// ---- 2. metadados ----
{
  const problems = [];
  for (const l of langs) {
    for (const [id, f] of Object.entries(files[l])) {
      if (f === 'llms.txt' || f.startsWith('_') || id === '00') continue;
      const fm = read(path.join(docDir, l, f)).match(/^---\n([\s\S]*?)\n---/);
      if (!fm) { problems.push(`${l}/${f}: sem metadados`); continue; }
      for (const k of KEYS[l]) if (!new RegExp(`^${k}:`, 'm').test(fm[1])) problems.push(`${l}/${f}: falta "${k}" nos metadados`);
    }
  }
  check('metadados no idioma de cada pasta', problems);
}

// ---- 3. links e âncoras ----
const slug = (h) => h.toLowerCase().replace(/[`*_~]/g, '').replace(/<[^>]+>/g, '').replace(/[^\p{L}\p{N}\s-]/gu, '').trim().replace(/\s+/g, '-');
{
  const problems = [];
  for (const l of langs) {
    const dir = path.join(docDir, l);
    const texts = Object.fromEntries(Object.values(files[l]).filter((f) => f.endsWith('.md')).map((f) => [f, read(path.join(dir, f))]));
    const anchors = Object.fromEntries(Object.entries(texts).map(([f, t]) => [f, new Set([...t.matchAll(/^#{1,6} (.+)$/gm)].map((m) => slug(m[1])))]));
    for (const [f, t] of Object.entries(texts)) {
      const noCode = t.replace(/```[\s\S]*?```/g, '').replace(/`[^`\n]*`/g, '');
      for (const m of noCode.matchAll(/\]\(([^)\s]+)\)/g)) {
        if (/^(https?:|mailto:)/.test(m[1])) continue;
        const [file, anchor] = m[1].split('#');
        const target = file ? path.resolve(dir, file) : path.join(dir, f);
        if (!fs.existsSync(target)) { problems.push(`${l}/${f}: link para arquivo inexistente: ${m[1]}`); continue; }
        if (anchor && path.dirname(target) === dir && target.endsWith('.md') && !anchors[path.basename(target)].has(anchor)) problems.push(`${l}/${f}: âncora inexistente: ${m[1]}`);
      }
    }
  }
  check('links e âncoras dentro de cada pasta', problems);
}

// ---- 4 e 5. paridade de conteúdo ----
const setOf = (t, re) => new Set([...t.matchAll(re)].map((m) => m[0]));
const diff = (a, b) => [...a].filter((x) => !b.has(x)).sort();
{
  const base = langs[0];
  const problems = [];
  for (const id of Object.keys(files[base])) {
    for (const l of langs.slice(1)) {
      if (!files[l][id] || !files[base][id].endsWith('.md')) continue;
      const a = text(base, id), b = text(l, id);
      for (const [label, re] of [['UC-', /\bUC-[A-Z]{3}-\d{3}\b/g], ['D…', /\bD\d+(\.\d+)?\b/g], ['PRB-', /\bPRB-\d+\b/g]]) {
        const sa = setOf(a, re), sb = setOf(b, re);
        const onlyA = diff(sa, sb), onlyB = diff(sb, sa);
        if (onlyA.length || onlyB.length) problems.push(`${id}: códigos ${label} diferentes — só em ${base}: [${onlyA.slice(0, 6).join(', ')}] só em ${l}: [${onlyB.slice(0, 6).join(', ')}]`);
      }
      const count = (t, re) => (t.match(re) || []).length;
      for (const [label, re] of [['blocos de código', /^```/gm], ['imagens', /!\[[^\]]*\]\(/g], ['linhas de tabela', /^\|/gm]]) {
        if (id === '28' && label === 'linhas de tabela') continue; // o glossário se reagrupa por letra em cada idioma
        const na = count(a, re), nb = count(b, re);
        if (na !== nb) problems.push(`${id}: ${label}: ${na} em ${base}, ${nb} em ${l}`);
      }
    }
  }
  check('códigos UC-/D/PRB citados, blocos de código, imagens e tabelas iguais entre os idiomas', problems);
}

console.log(report.join('\n'));
console.log(failures ? `\n${failures} problema(s)` : '\ntudo certo');
process.exit(failures ? 1 : 0);
