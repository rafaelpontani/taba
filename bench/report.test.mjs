// Teste do relatório com os JSON de exemplo de bench/testdata.
//   node --test bench/report.test.mjs
import assert from 'node:assert/strict';
import { cpSync, existsSync, mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { BANNER, generate, median, parseName } from './report.mjs';

const here = dirname(fileURLToPath(import.meta.url));

function run() {
  const dir = mkdtempSync(join(tmpdir(), 'bench-report-'));
  cpSync(join(here, 'testdata'), dir, { recursive: true });
  generate(dir);
  return dir;
}

/// UC: UC-BNC-003
test('a mediana e os nomes dos arquivos', () => {
  assert.equal(median([3, 1, 2]), 2);
  assert.equal(median([4, 1, 2, 3]), 2.5);
  assert.deepEqual(parseName('taba-default-simple-1kb-2.json'), { system: 'taba', durability: 'default', workload: 'simple-1kb', round: 2 });
  assert.equal(parseName('machine.json'), null);
});

/// UC: UC-BNC-003
test('o relatório abre com a faixa de máquina local', () => {
  const md = readFileSync(join(run(), 'results.md'), 'utf8');
  assert.ok(md.split('\n').slice(0, 4).join('\n').includes(BANNER));
});

/// UC: UC-BNC-003
test('a tabela traz a mediana e o intervalo das rodadas', () => {
  const md = readFileSync(join(run(), 'results.md'), 'utf8');
  // Taba: p99 de 9.5, 11 e 9 -> mediana 9.5, de 9.0 a 11.0.
  assert.match(md, /\| taba \| 3 \|.*9\.5 \(9\.0–11\.0\)/);
  // Uma só rodada: sem intervalo.
  assert.match(md, /\| rabbitmq \| 1 \| 9,000 \| 9,000 \| 8\.0 \| 30\.0 \| 90\.0 \|/);
});

/// UC: UC-BNC-003
test('os gráficos SVG saem e a lista das falhas aparece', () => {
  const dir = run();
  for (const f of ['simple-1kb-default-latency.svg', 'simple-1kb-default-throughput.svg', 'simple-1kb-fsync-latency.svg']) {
    assert.ok(existsSync(join(dir, 'charts', f)), f);
    assert.match(readFileSync(join(dir, 'charts', f), 'utf8'), /^<svg /);
  }
  assert.match(readFileSync(join(dir, 'results.md'), 'utf8'), /nats default simple-1kb 1/);
});

/// UC: UC-BNC-003
test('o relatório não elege vencedor e anota o RabbitMQ', () => {
  const md = readFileSync(join(run(), 'results.md'), 'utf8');
  assert.doesNotMatch(md, /vencedor:|melhor sistema|ranking/i);
  assert.match(md, /rabbitmq: a fila de quórum já faz fsync/);
});
