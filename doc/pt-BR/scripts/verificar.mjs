#!/usr/bin/env node
// Confere a documentação de doc/pt-BR contra o código (sem rede, sem compilar).
//
// Uso (da raiz do repositório):  node doc/pt-BR/scripts/verificar.mjs
// Saída: um relatório por verificação e código de saída 1 se algo falhar.

import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')), '../../..');
const docDir = path.join(root, 'doc/pt-BR');
const src = path.join(root, 'crates/taba-node/src');
const read = (p) => fs.readFileSync(p, 'utf8').split('\r').join('');
const walk = (d) => fs.readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(path.join(d, e.name)) : [path.join(d, e.name)]));

const files = fs.readdirSync(docDir).filter((f) => f.endsWith('.md') || f === 'llms.txt').sort();
const texts = Object.fromEntries(files.map((f) => [f, read(path.join(docDir, f))]));
const generated = new Set(['26a-casos-de-uso-implementados.md', '26b-casos-de-uso-planejados.md', '30-decisoes.md', '31-pendencias.md']);

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

// ---- 1. links ----
function slug(heading) {
  return heading
    .toLowerCase()
    .replace(/[`*_~]/g, '')
    .replace(/<[^>]+>/g, '')
    .replace(/[^\p{L}\p{N}\s-]/gu, '')
    .trim()
    .replace(/\s+/g, '-');
}
const anchors = {};
for (const [f, t] of Object.entries(texts)) {
  anchors[f] = new Set([...t.matchAll(/^#{1,6} (.+)$/gm)].map((m) => slug(m[1])));
}
{
  const problems = [];
  for (const [f, t] of Object.entries(texts)) {
    const noCode = t.replace(/```[\s\S]*?```/g, '').replace(/`[^`\n]*`/g, '');
    for (const m of noCode.matchAll(/\]\(([^)\s]+)\)/g)) {
      const link = m[1];
      if (/^(https?:|mailto:|#$)/.test(link)) continue;
      const [file, anchor] = link.split('#');
      const target = file ? path.resolve(docDir, file) : path.join(docDir, f);
      if (!fs.existsSync(target)) { problems.push(`${f}: link para arquivo inexistente: ${link}`); continue; }
      if (anchor && file && target.startsWith(docDir) && target.endsWith('.md')) {
        const a = anchors[path.basename(target)];
        if (a && !a.has(anchor)) problems.push(`${f}: âncora inexistente: ${link}`);
      } else if (anchor && !file) {
        if (!anchors[f].has(anchor)) problems.push(`${f}: âncora inexistente: #${anchor}`);
      }
    }
  }
  check('links entre arquivos e para o repositório', problems);
}

// ---- 2. metadados e seção "Fontes e provas" ----
{
  const problems = [];
  const exempt = new Set(['README.md', 'llms.txt', '_divergencias.md', '00-plano-de-documentacao.md']);
  for (const [f, t] of Object.entries(texts)) {
    if (f === 'llms.txt' || f === '_divergencias.md') continue;
    const fm = t.match(/^---\n([\s\S]*?)\n---/);
    if (!fm) { if (f !== '00-plano-de-documentacao.md') problems.push(`${f}: sem metadados`); continue; }
    for (const key of ['título', 'resumo', 'tipo', 'status', 'verificado-em', 'fontes']) {
      if (!new RegExp(`^${key}:`, 'm').test(fm[1])) problems.push(`${f}: falta "${key}" nos metadados`);
    }
    const status = (fm[1].match(/^status: (.+)$/m) || [])[1];
    if (status && !['estável', 'parcial', 'planejado'].includes(status.trim())) problems.push(`${f}: status inválido "${status}"`);
    if (!exempt.has(f) && !/^## Fontes e provas/m.test(t)) problems.push(`${f}: sem a seção "Fontes e provas"`);
    // fontes: caminhos que existem?
    const list = (fm[1].match(/^fontes: \[(.*)\]$/m) || [])[1];
    if (list) {
      for (const p of list.split(',').map((s) => s.trim()).filter(Boolean)) {
        if (!fs.existsSync(path.join(root, p)) && !fs.existsSync(path.join(root, 'crates/taba-node', p)) && !fs.existsSync(path.join(src, p)))
          problems.push(`${f}: fonte inexistente: ${p}`);
      }
    }
  }
  check('metadados, status e "Fontes e provas"', problems);
}

// ---- 3. parâmetros de configuração ----
{
  const problems = [];
  const params = read(path.join(src, 'config/params/mod.rs'));
  const keys = [...params.matchAll(/p\("([a-z0-9.\-]+)",/g)].map((m) => m[1]);
  const doc = texts['22-configuracao.md'];
  for (const k of keys) if (!doc.includes('`' + k + '`')) problems.push(`chave do código ausente em 22-configuracao.md: ${k}`);
  const rows = [...doc.matchAll(/^\| `([a-z0-9.\-]+)`(?:<br>`([a-z0-9.\-]+)`)? \| `TABA_/gm)];
  for (const r of rows) for (const k of [r[1], r[2]].filter(Boolean)) if (!keys.includes(k)) problems.push(`chave documentada que não existe no código: ${k}`);
  check(`parâmetros de configuração (${keys.length} no código)`, problems);
}

// ---- 4. códigos de erro ----
{
  const problems = [];
  const catalog = [...read(path.join(src, 'core/error/mod.rs')).matchAll(/=> \("([a-z-]+\.[a-z-]+)", (\d+)\)/g)].map((m) => [m[1], m[2]]);
  const doc = texts['19-erros-e-limites.md'];
  for (const [code, status] of catalog) {
    const row = doc.split('\n').find((l) => l.includes('`' + code + '`') && l.startsWith('|'));
    if (!row) problems.push(`código ausente em 19-erros-e-limites.md: ${code}`);
    else {
      const heading = doc.slice(0, doc.indexOf(row)).split('\n').filter((l) => l.startsWith('## ')).pop() || '';
      const cells = row.split('|').map((c) => c.trim());
      const stated = cells.find((c) => /^\d{3}$/.test(c));
      if (stated && stated !== status) problems.push(`${code}: status ${stated} no documento, ${status} no código`);
      else if (!stated) {
        const m = heading.match(/(\d{3})/g) || [];
        if (m.length && !m.includes(status)) problems.push(`${code}: status ${status} fora do grupo "${heading}"`);
      }
    }
  }
  const known = new Set(catalog.map((c) => c[0]));
  const prefixes = new Set([...known].map((c) => c.split('.')[0]));
  for (const m of doc.matchAll(/^\| `([a-z-]+\.[a-z-]+)` \|/gm)) {
    if (prefixes.has(m[1].split('.')[0]) && !known.has(m[1])) problems.push(`código documentado que não existe: ${m[1]}`);
  }
  check(`códigos de erro (${catalog.length} no código)`, problems);
}

// ---- 5. rotas ----
{
  const problems = [];
  const apiText = ['15-api-visao-geral.md', '16-api-cliente.md', '17-api-operacao.md', '18-api-cluster.md'].map((f) => texts[f]).join('\n');
  const norm = (s) => s.replace(/\{[^}]*\}/g, '{}').replace(/\/+$/, '');
  const docRoutes = new Set([...apiText.matchAll(/(\/(?:topics|tokens|health|metrics|node|cluster|auth|stats)[A-Za-z0-9_{}\-/.]*)/g)].map((m) => norm(m[1])));
  const routes = [];
  for (const f of walk(path.join(src, 'http')).filter((p) => p.endsWith('.rs') && !p.endsWith('tests.rs'))) {
    for (const m of read(f).matchAll(/\.route\(\s*"([^"]+)"/g)) routes.push(m[1]);
  }
  for (const m of read(path.join(src, 'http/operations.rs')).matchAll(/\.route\(\s*\n?\s*"([^"]+)"/g)) routes.push(m[1]);
  for (const m of read(path.join(src, 'cluster/protocol/http.rs')).matchAll(/"(\/cluster\/[a-z\-/]*)"/g)) routes.push(m[1]);
  for (const m of read(path.join(src, 'cluster/protocol/http.rs')).matchAll(/format!\("(\/cluster\/[^"]+)"/g)) routes.push(m[1].replace(/\{[a-z_]+\}/g, '{}'));
  for (const r of new Set(routes)) {
    if (r === '/cluster/{*caminho}') continue;
    const n = norm(r.replace(/\{\*[^}]*\}/g, '{}'));
    if (![...docRoutes].some((d) => d === n || d.startsWith(n + '/') || n.startsWith(d + '/') || d.replace(/\{\}/g, '{}') === n)) problems.push(`rota do código sem menção nos documentos de API: ${r}`);
  }
  check(`rotas HTTP (${new Set(routes).size} no código)`, problems);
}

// ---- 6. casos de uso ----
{
  const problems = [];
  const histCodes = new Set();
  const areas = new Set();
  // O catálogo histórico (`doc/20*.md`), quando ainda existe; senão os cartões dos catálogos gerados
  // (`26a`, `26b`). Os cartões `UC-BKP` foram escritos à mão em 26a.
  const legacy = ['20a-casos-de-uso-cliente.md', '20b-casos-de-uso-no-dados.md', '20c-casos-de-uso-no-cluster.md'].filter((f) => fs.existsSync(path.join(root, 'doc', f)));
  for (const f of legacy) {
    for (const m of read(path.join(root, 'doc', f)).matchAll(/^\| (UC-([A-Z]{3})-\d{3}) \|/gm)) { histCodes.add(m[1]); areas.add(m[2]); }
  }
  if (!legacy.length) {
    for (const f of ['26a-casos-de-uso-implementados.md', '26b-casos-de-uso-planejados.md']) {
      for (const m of texts[f].matchAll(/^### (UC-([A-Z]{3})-\d{3})[ ]*$/gm)) { histCodes.add(m[1]); areas.add(m[2]); }
    }
  }
  for (const [f, t] of Object.entries(texts)) {
    if (generated.has(f)) continue;
    const plain = t.replace(/UC-XXX-NNN|UC-ÁREA-NNN|UC-[A-Z]{3}-NNN|UC-YYY-NNN|UC-MSG-003, UC-ESC-004/g, '');
    for (const m of plain.matchAll(/UC-([A-Z]{3})-(\d{3})/g)) if (!histCodes.has(m[0])) problems.push(`${f}: caso inexistente: ${m[0]}`);
    for (const m of plain.matchAll(/UC-([A-Z]{3})\b(?!-\d)/g)) if (!areas.has(m[1])) problems.push(`${f}: área inexistente: UC-${m[1]}`);
  }
  // os catálogos gerados citam só códigos reais
  for (const f of ['26a-casos-de-uso-implementados.md', '26b-casos-de-uso-planejados.md']) {
    for (const m of texts[f].matchAll(/^### (UC-[A-Z]{3}-\d{3})[ ]*$/gm)) if (!histCodes.has(m[1])) problems.push(`${f}: caso inexistente: ${m[1]}`);
  }
  check(`casos de uso (${histCodes.size} no catálogo histórico, ${areas.size} áreas)`, problems);
}

// ---- 7. métricas ----
{
  const problems = [];
  const defs = read(path.join(src, 'telemetry/metrics/defs.rs'));
  const names = [...defs.matchAll(/(?:def|histogram)\(\s*"([a-z0-9._]+)"/g)].map((m) => m[1]);
  const doc = texts['13-observabilidade.md'];
  for (const n of names) if (!doc.includes('`' + n + '`')) problems.push(`métrica ausente em 13-observabilidade.md: ${n}`);
  check(`métricas (${names.length} no código)`, problems);
}

// ---- 8. diagramas planejados ----
{
  const problems = [];
  let total = 0;
  for (const [f, t] of Object.entries(texts)) {
    for (const line of t.split('\n')) {
      if (!line.includes('DIAGRAMA PLANEJADO') || line.startsWith('>') && /Os diagramas ainda não/.test(line)) continue;
      if (!/^🖼️ \*\*DIAGRAMA PLANEJADO\*\*/.test(line)) continue;
      total++;
      if (!/tipo:/.test(line) || !/objetivo:/.test(line)) problems.push(`${f}: diagrama sem tipo ou objetivo`);
    }
  }
  check(`diagramas planejados (${total} marcados)`, problems);
}

console.log(report.join('\n'));
console.log(failures ? `\n${failures} problema(s)` : '\ntudo certo');
process.exit(failures ? 1 : 0);
