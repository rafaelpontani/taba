#!/usr/bin/env node
// Confere se os metadados (o bloco --- do topo) de cada .md da documentação são YAML válido, que é o que o GitHub e o
// preview do VS Code leem: um valor sem aspas não pode ter ": " nem " #", nem começar com um indicador do YAML (` [ { "
// ' & * ! | > % @ #). Um valor assim quebra o preview ("Nested mappings are not allowed in compact mappings") e some da
// página do GitHub. A correção é pôr o valor entre aspas duplas; os outros scripts leem os metadados com ou sem aspas.
//
// Uso (da raiz do repositório):
//   node doc/pt-BR/scripts/metadados.mjs              # confere doc/<idioma>/*.md; código de saída 1 se algo falhar
//   node doc/pt-BR/scripts/metadados.mjs --corrigir   # põe entre aspas os valores que precisam
// O verificar.mjs também roda esta conferência.

import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')), '../../..');

// O que torna um escalar sem aspas inválido (ou diferente do texto) no YAML.
function plainProblem(value) {
  if (/^[`\[\]{}"'&*!|>%@#,]/.test(value)) return `começa com "${value[0]}"`;
  if (/^[-?:](\s|$)/.test(value)) return `começa com "${value[0]} "`;
  if (/:(\s|$)/.test(value)) return 'tem ": "';
  if (/\s#/.test(value)) return 'tem " #"';
  return null;
}

function quoted(value) {
  return '"' + value.replace(/\\/g, '\\\\').replace(/"/g, '\\"') + '"';
}

// Os problemas de um valor de "chave: valor" (uma lista [a, b] é conferida item a item).
function valueProblem(value) {
  if (value === '') return null;
  if (/^"(?:[^"\\]|\\.)*"$/.test(value) || /^'(?:[^']|'')*'$/.test(value)) return null;
  if (value.startsWith('[') && value.endsWith(']')) {
    for (const item of value.slice(1, -1).split(',').map((s) => s.trim()).filter(Boolean)) {
      const p = plainProblem(item);
      if (p) return `o item "${item}" da lista ${p}`;
    }
    return null;
  }
  return plainProblem(value);
}

export function checkFrontmatter(file, text, fix = false) {
  const problems = [];
  const eol = text.includes('\r\n') ? '\r\n' : '\n';
  const lines = text.split(/\r?\n/);
  if (lines[0] !== '---') return { problems, text };
  const end = lines.indexOf('---', 1);
  if (end < 0) return { problems: [`${file}: os metadados não fecham com ---`], text };
  let changed = false;
  for (let i = 1; i < end; i++) {
    const m = lines[i].match(/^([^\s:#][^:]*):(?: (.*))?$/);
    if (!m) {
      problems.push(`${file}:${i + 1}: linha que não é "chave: valor": ${lines[i].slice(0, 60)}`);
      continue;
    }
    const value = (m[2] ?? '').trim();
    const p = valueProblem(value);
    if (!p) continue;
    if (fix && !(value.startsWith('[') && value.endsWith(']'))) {
      lines[i] = `${m[1]}: ${quoted(value)}`;
      changed = true;
    } else {
      problems.push(`${file}:${i + 1}: "${m[1]}" ${p}: ponha o valor entre aspas duplas`);
    }
  }
  return { problems, text: changed ? lines.join(eol) : text };
}

// Todos os .md das pastas de idioma de doc/ (as pastas com dois códigos, como pt-BR e en-US).
export function docFiles() {
  const docRoot = path.join(root, 'doc');
  return fs.readdirSync(docRoot, { withFileTypes: true })
    .filter((e) => e.isDirectory() && /^[a-z]{2}-[A-Z]{2}$/.test(e.name))
    .flatMap((e) => fs.readdirSync(path.join(docRoot, e.name)).filter((f) => f.endsWith('.md')).map((f) => path.join(docRoot, e.name, f)));
}

export function checkAll(fix = false) {
  const problems = [];
  for (const full of docFiles()) {
    const rel = path.relative(root, full).split(path.sep).join('/');
    const text = fs.readFileSync(full, 'utf8');
    const result = checkFrontmatter(rel, text, fix);
    if (result.text !== text) {
      fs.writeFileSync(full, result.text);
      console.log(`corrigido: ${rel}`);
    }
    problems.push(...result.problems);
  }
  return problems;
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1'))) {
  const problems = checkAll(process.argv.includes('--corrigir'));
  for (const p of problems) console.log(`  - ${p}`);
  console.log(problems.length ? `\n${problems.length} problema(s) nos metadados` : 'metadados: tudo certo');
  process.exit(problems.length ? 1 : 0);
}
