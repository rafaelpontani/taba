#!/usr/bin/env node
// Gera doc/pt-BR/26a-casos-de-uso-implementados.md e 26b-casos-de-uso-planejados.md.
//
// Fonte dos casos: doc/20a, 20b e 20c (o catálogo histórico, que o teste `uc_coverage` confere).
// Fonte dos testes: as linhas `/// UC: UC-XXX-NNN` do código (a mesma regra do `uc_coverage.rs`),
// lidas de verdade, não da coluna "Testes" dos documentos (que pode estar velha).
//
// Uso (da raiz do repositório):  node doc/pt-BR/scripts/gerar-casos-de-uso.mjs

import fs from 'node:fs';
import path from 'node:path';
import { execSync } from 'node:child_process';

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')), '../../..');
const rel = (f) => path.relative(root, f).split(path.sep).join('/');

function walk(dir, accept, skip = []) {
  if (!fs.existsSync(dir)) return [];
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) return skip.includes(e.name) ? [] : walk(p, accept, skip);
    return accept(p) ? [p] : [];
  });
}

// ---- 1. testes que citam cada caso ----
const crateDir = path.join(root, 'crates/taba-node');
const sources = [
  ...walk(path.join(crateDir, 'src'), (p) => p.endsWith('.rs')).map((f) => [f, crateDir]),
  ...walk(path.join(crateDir, 'tests'), (p) => p.endsWith('.rs')).map((f) => [f, crateDir]),
  ...fs.readdirSync(path.join(root, 'connectors'), { withFileTypes: true })
    .filter((e) => e.isDirectory())
    .flatMap((e) => walk(path.join(root, 'connectors', e.name), (p) => /\.(rs|cs)$/.test(p), ['target', 'bin', 'obj', 'dist']))
    .map((f) => [f, root]),
].sort((a, b) => (a[0] < b[0] ? -1 : 1));

const tests = new Map(); // código -> Set('arquivo::função')
for (const [file, base] of sources) {
  const shown = path.relative(base, file).split(path.sep).join('/');
  const isCs = file.endsWith('.cs');
  let pending = [];
  for (const raw of fs.readFileSync(file, 'utf8').split('\n')) {
    const line = raw.trimStart();
    if (line.startsWith('/// UC:')) {
      pending.push(...line.slice('/// UC:'.length).split(',').map((s) => s.trim()).filter(Boolean));
      continue;
    }
    if (!pending.length || !line || line.startsWith('#[') || (isCs && line.startsWith('[')) || line.startsWith('//')) continue;
    let name = null;
    if (isCs) {
      if (line.includes('(')) name = line.split('(')[0].trim().split(/\s+/).pop();
    } else if (line.includes('fn ')) {
      name = line.split('fn ')[1].split(/[(<]/)[0];
    }
    if (name) {
      for (const code of pending) {
        if (!tests.has(code)) tests.set(code, new Set());
        tests.get(code).add(`${shown}::${name.trim()}`);
      }
    }
    pending = [];
  }
}

// ---- 2. os casos documentados ----
const areaNames = {};
const areaIndex = fs.readFileSync(path.join(root, 'doc/20-casos-de-uso.md'), 'utf8');
for (const m of areaIndex.matchAll(/^\| `([A-Z]{3})` \| ([^|]+) \|/gm)) areaNames[m[1]] = m[2].trim();

const cases = []; // {code, area, text, source, level, retired, group}
for (const f of ['20a-casos-de-uso-cliente.md', '20b-casos-de-uso-no-dados.md', '20c-casos-de-uso-no-cluster.md']) {
  let group = '';
  for (const line of fs.readFileSync(path.join(root, 'doc', f), 'utf8').split('\n')) {
    const h = line.match(/^#{3,4} (.+)/);
    if (h) group = h[1].trim();
    if (!line.startsWith('| UC-')) continue;
    const cells = line.split('|').slice(1, -1).map((c) => c.trim());
    const code = cells[0];
    if (!/^UC-[A-Z]{3}-\d{3}$/.test(code)) continue;
    cases.push({
      code,
      area: code.slice(3, 6),
      text: cells[1] ?? '',
      source: cells[2] ?? '',
      level: cells[3] ?? '',
      retired: /\(retirado/.test(cells[1] ?? ''),
    });
  }
}

const live = cases.filter((c) => !c.retired);
const done = live.filter((c) => tests.has(c.code));
const todo = live.filter((c) => !tests.has(c.code));
const retired = cases.filter((c) => c.retired);
const commit = execSync('git rev-parse --short HEAD', { cwd: root }).toString().trim();
const today = new Date().toISOString().slice(0, 10);

const esc = (s) => s.replace(/\|/g, '\\|');
const testCell = (code) => {
  const list = [...tests.get(code)];
  const shown = list.slice(0, 2).map((t) => `\`${t}\``).join('<br>');
  return list.length > 2 ? `${shown}<br>+${list.length - 2}` : shown;
};


// divide o texto do caso em situação (título) e resultado, no primeiro ": " fora de crases
function splitCase(text) {
  let inCode = false;
  for (let i = 0; i < text.length - 1; i++) {
    if (text[i] === '`') inCode = !inCode;
    if (!inCode && text[i] === ':' && text[i + 1] === ' ') return [text.slice(0, i).trim(), text.slice(i + 2).trim()];
  }
  return [null, text];
}
function card(c, tail) {
  const [situation, result] = splitCase(c.text);
  const br = '  \n'; // quebra de linha do Markdown
  let out = '### ' + c.code + '\n\n';
  if (situation) out += '**Caso:** ' + situation + br + '**Então:** ' + result + br;
  else out += '**Caso:** ' + result + br;
  out += '**Nível:** ' + c.level + br + tail + '\n---\n\n';
  return out;
}

function byArea(list) {
  const m = new Map();
  for (const c of list) {
    if (!m.has(c.area)) m.set(c.area, []);
    m.get(c.area).push(c);
  }
  return m;
}

const levelHelp = 'Nível: **U** unitário, **I** um nó real, **C** cluster simulado, **K** conector.';

// ---- 26a ----
let a = `---
título: Casos de uso implementados
resumo: O catálogo dos casos de uso do Taba que têm ao menos um teste que os cita, por área, com o caso, o nível e os testes que o provam.
tipo: referência
status: estável
verificado-em: ${today} @ ${commit}
fontes: [crates/taba-node/tests, crates/taba-node/src, connectors/rust, connectors/csharp, doc/20a-casos-de-uso-cliente.md, doc/20b-casos-de-uso-no-dados.md, doc/20c-casos-de-uso-no-cluster.md]
---

# Casos de uso implementados

> **Gerado.** Este arquivo é produzido por \`node doc/pt-BR/scripts/gerar-casos-de-uso.mjs\`, que lê as linhas \`/// UC: UC-XXX-NNN\` do código: não se edita à mão. Formato dos códigos e regra de cobertura: [26-casos-de-uso.md](26-casos-de-uso.md). Os casos **sem teste** estão em [26b-casos-de-uso-planejados.md](26b-casos-de-uso-planejados.md).

**Placar (${today}, commit ${commit}):** **${done.length}** de **${live.length}** casos têm ao menos um teste (${(100 * done.length / live.length).toFixed(1)}%); **${todo.length}** não têm; **${retired.length}** estão retirados (o código não é reaproveitado).

${levelHelp} Cada caso é um bloco: o **código** no cabeçalho; abaixo, **Caso** (a situação), **Então** (o resultado esperado), **Nível** e **Testes** (\`arquivo::função\`, relativo a \`crates/taba-node\`, ou à raiz do repositório para os conectores).

## Cobertura por área

| Área | Assunto | Com teste | Sem teste |
|---|---|---:|---:|
`;
const doneBy = byArea(done), todoBy = byArea(todo);
const areas = [...new Set(live.map((c) => c.area))];
for (const ar of areas) a += `| \`${ar}\` | ${areaNames[ar] ?? ''} | ${(doneBy.get(ar) ?? []).length} | ${(todoBy.get(ar) ?? []).length} |\n`;

for (const ar of areas) {
  const list = doneBy.get(ar) ?? [];
  if (!list.length) continue;
  a += `\n## ${ar}: ${areaNames[ar] ?? ''}\n\n`;
  for (const c of list) {
    const list2 = [...tests.get(c.code)].map((x) => `  - \`${x}\``).join('\n');
    a += card(c, `**Testes:**\n${list2}\n`);
  }
}
a += `\n## Fontes e provas\n\n- Os casos vêm de \`doc/20a\`, \`20b\` e \`20c\` (o catálogo histórico, conferido pelo teste \`crates/taba-node/tests/uc_coverage.rs\`); os testes, das linhas \`/// UC:\` do código.\n- Regenerar: \`node doc/pt-BR/scripts/gerar-casos-de-uso.mjs\`.\n`;
fs.writeFileSync(path.join(root, 'doc/pt-BR/26a-casos-de-uso-implementados.md'), a);

// ---- 26b ----
let b = `---
título: Casos de uso planejados
resumo: Os casos de uso documentados que ainda não têm teste que os cite — o que falta provar — por área, com o motivo quando o próprio caso o diz.
tipo: referência
status: parcial
verificado-em: ${today} @ ${commit}
fontes: [doc/20a-casos-de-uso-cliente.md, doc/20b-casos-de-uso-no-dados.md, doc/20c-casos-de-uso-no-cluster.md, crates/taba-node/tests/uc_coverage.rs]
---

# Casos de uso planejados (sem teste)

> **Gerado** por \`node doc/pt-BR/scripts/gerar-casos-de-uso.mjs\`. Um caso aqui **não quer dizer que o comportamento não existe**: quer dizer que **nenhum teste o cita** hoje. Alguns recursos já funcionam (e aparecem em outros documentos como ✅) sem prova por caso; outros são ⬜ planejados. Os casos com teste estão em [26a-casos-de-uso-implementados.md](26a-casos-de-uso-implementados.md).

**Placar (${today}, commit ${commit}):** **${todo.length}** casos sem teste, de **${live.length}**. Os casos marcados "(pendente: D##)" dependem de uma dúvida ainda aberta ([31-pendencias.md](31-pendencias.md)).

${levelHelp} Para fechar um caso, cite-o num teste (\`/// UC: UC-XXX-NNN\`) do nível indicado e regenere ([26-casos-de-uso.md](26-casos-de-uso.md)).

## Onde estão as lacunas

| Área | Assunto | Sem teste | Com teste |
|---|---|---:|---:|
`;
for (const ar of areas) { if ((todoBy.get(ar) ?? []).length) b += `| \`${ar}\` | ${areaNames[ar] ?? ''} | ${todoBy.get(ar).length} | ${(doneBy.get(ar) ?? []).length} |\n`; }
for (const ar of areas) {
  const list = todoBy.get(ar) ?? [];
  if (!list.length) continue;
  b += `\n## ${ar}: ${areaNames[ar] ?? ''}\n\n`;
  for (const c of list) b += card(c, `**Fonte histórica:** ${c.source}\n`);
}
b += `\n## Casos retirados\n\nOs códigos nunca são reaproveitados: ${retired.map((c) => c.code).join(', ') || 'nenhum'}.\n`;
b += `\n## Fontes e provas\n\n- Catálogo: \`doc/20a\`, \`20b\`, \`20c\`. Testes: as linhas \`/// UC:\` do código (regra de \`uc_coverage.rs\`).\n- Regenerar: \`node doc/pt-BR/scripts/gerar-casos-de-uso.mjs\`.\n`;
fs.writeFileSync(path.join(root, 'doc/pt-BR/26b-casos-de-uso-planejados.md'), b);

console.log(`casos: ${live.length} ativos, ${done.length} com teste, ${todo.length} sem, ${retired.length} retirados`);
