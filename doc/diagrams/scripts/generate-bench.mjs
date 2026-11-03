#!/usr/bin/env node
// Gera os gráficos de benchmark da documentação (doc/diagrams/img/<lang>/bench-*.svg) a partir de uma rodada do benchmark
// local. Não passa pelo Archify. A fonte é doc/diagrams/data/bench-results.json, o resumo (summary.json) que o
// bench/report.mjs escreve; ele é versionado, e os SVGs saem dele.
//   node doc/diagrams/scripts/generate-bench.mjs                                # todos os idiomas, do JSON versionado
//   node doc/diagrams/scripts/generate-bench.mjs --from bench/results/<pasta>   # copia o summary.json da rodada e gera
//   node doc/diagrams/scripts/generate-bench.mjs --lang=pt-BR
// Os textos estão em inglês aqui e passam pela tabela doc/diagrams/i18n/<lang>.json (o que falta fica em inglês, com aviso).
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { SYSTEM_NAMES, barChart, cumulative, lineChart } from '../../../bench/charts.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const DATA = resolve(root, 'doc/diagrams/data/bench-results.json');

const from = process.argv.indexOf('--from');
if (from > 0) {
  const src = resolve(process.argv[from + 1], 'summary.json');
  if (!existsSync(src)) { console.error(`não há ${src}: rode node bench/report.mjs na pasta antes`); process.exit(1); }
  mkdirSync(dirname(DATA), { recursive: true });
  copyFileSync(src, DATA);
  console.log(`dados: ${DATA}`);
}
const data = JSON.parse(readFileSync(DATA, 'utf8'));
const langArg = process.argv.find((a) => a.startsWith('--lang='));
const langs = langArg ? [langArg.slice(7)] : ['en-US', ...readdirSync(resolve(root, 'doc/diagrams/i18n')).filter((f) => f.endsWith('.json')).map((f) => f.slice(0, -5))];

// Os sistemas da comparação, na ordem fixa (a cor segue o sistema).
const SYSTEMS = ['taba', 'rabbitmq', 'nats'];
const DUR = 'fsync';
const STEP = data.sampleSeconds ?? 10;

for (const lang of langs) {
  const table = lang === 'en-US' ? {} : JSON.parse(readFileSync(resolve(root, 'doc/diagrams/i18n', `${lang}.json`), 'utf8'));
  const missing = new Set();
  const T = (s) => {
    if (lang === 'en-US') return s;
    if (!(s in table)) missing.add(s);
    return table[s] ?? s;
  };
  const out = resolve(root, 'doc/diagrams/img', lang);
  mkdirSync(out, { recursive: true });
  const write = (name, svg) => { writeFileSync(join(out, name), svg); console.log(`SVG: doc/diagrams/img/${lang}/${name}`); };

  const cell = (workload, system, nodes) => data.cells[workload]?.[DUR]?.[nodes > 1 ? `${system}-x${nodes}` : system];
  const stat = (workload, system, nodes, key) => cell(workload, system, nodes)?.stats?.[key] ?? NaN;
  const present = (workload, nodes) => SYSTEMS.filter((s) => cell(workload, s, nodes));
  const NODES = [[1, T('1 node')], [3, T('3 nodes')]];
  const name = (s) => SYSTEM_NAMES[s];
  const local = T('local run, not representative');
  const machine = data.machine ?? {};
  const note = `${T('Same limits for every system')}: ${T('1 node')} ${machine.server_cpus ?? '?'} ${T('cores')} / ${machine.server_memory_gb ?? '?'} GB · ${T('3 nodes')} ${machine.cluster_node_cpus ?? '?'} ${T('cores')} / ${machine.cluster_node_memory_mb ?? '?'} MB ${T('each')} · ${T('durability')}: fsync`;
  const bars = (workload, key) => SYSTEMS.filter((s) => NODES.some(([n]) => cell(workload, s, n)))
    .map((s) => ({ key: s, label: name(s), values: NODES.map(([n]) => stat(workload, s, n, key)) }));

  // 1. Vazão máxima, um nó e três.
  if (data.cells['max-rate-1kb']) {
    write('bench-throughput.svg', barChart({
      title: T('Maximum throughput, 1 KB messages'), subtitle: `${T('messages consumed and acknowledged per second')} · ${local}`,
      unit: 'msg/s', categories: NODES.map(([, l]) => l), series: bars('max-rate-1kb', 'consume'), note,
    }));
    // 2. Linha do tempo: publicado e consumido, e o acumulado.
    for (const [n, where] of NODES) {
      const systems = present('max-rate-1kb', n);
      if (!systems.length) continue;
      write(`bench-timeline-${n}.svg`, lineChart({
        title: `${T('Messages per second over time')}, ${where}`, subtitle: `${T('maximum rate; consumed (solid) and published (dashed)')} · ${local}`,
        unit: 'msg/s', xLabel: T('seconds of the measured phase'), note,
        series: systems.flatMap((s) => [
          { key: s, label: `${name(s)} ${T('published')}`, values: cell('max-rate-1kb', s, n).series.publish, step: STEP, dash: true },
          { key: s, label: `${name(s)} ${T('consumed')}`, endLabel: name(s), values: cell('max-rate-1kb', s, n).series.consume, step: STEP },
        ]),
        legend: [
          ...systems.map((s) => ({ key: s, label: name(s), line: true })),
          { label: T('published'), dash: true }, { label: T('consumed'), line: true },
        ],
      }));
      write(`bench-processed-${n}.svg`, lineChart({
        title: `${T('Messages processed over time')}, ${where}`, subtitle: `${T('total consumed and acknowledged since the start, maximum rate')} · ${local}`,
        unit: T('messages'), xLabel: T('seconds of the measured phase'), note,
        series: systems.map((s) => ({ key: s, label: name(s), values: cumulative(cell('max-rate-1kb', s, 1 * n).series.consume, STEP), step: STEP })),
      }));
    }
    // 3. Eficiência: mensagens por núcleo de CPU.
    write('bench-efficiency.svg', barChart({
      title: T('Messages per CPU core'), subtitle: `${T('maximum rate; consumed per second divided by the average cores the server used')} · ${local}`,
      unit: T('msg/s per core'), categories: NODES.map(([, l]) => l), series: bars('max-rate-1kb', 'perCore'), note,
    }));
  }
  // 4. Latência a taxa fixa.
  if (data.cells['simple-1kb']) {
    const cats = NODES.flatMap(([n, l]) => [[n, 'p50', `p50 · ${l}`], [n, 'p99', `p99 · ${l}`]]);
    write('bench-latency.svg', barChart({
      title: T('End-to-end latency at 10,000 msg/s'), subtitle: `${T('from publish to delivery, lower is better')} · ${local}`,
      unit: 'ms', categories: cats.map(([, , l]) => l), note,
      series: SYSTEMS.map((s) => ({ key: s, label: name(s), values: cats.map(([n, k]) => stat('simple-1kb', s, n, k)) })),
    }));
  }
  // 5. Leitura de um backlog de 1 GB.
  if (data.cells['backlog-1kb']) {
    write('bench-backlog.svg', barChart({
      title: T('Reading a 1 GB backlog'), subtitle: `${T('messages consumed and acknowledged per second while the backlog lasts')} · ${local}`,
      unit: 'msg/s', categories: NODES.map(([, l]) => l), series: bars('backlog-1kb', 'drain'), note,
    }));
  }
  // 6. Ordem por chave.
  if (data.cells['keyed-1kb']) {
    write('bench-keyed.svg', barChart({
      title: T('Ordered by key, 16 keys at 10,000 msg/s'), subtitle: `${T('messages consumed per second')} · ${local}`,
      unit: 'msg/s', categories: NODES.map(([, l]) => l), series: bars('keyed-1kb', 'consume'),
      note: T('Taba keeps the order per key (FIFO queue); the RabbitMQ and NATS drivers ignore the key'),
    }));
  }
  // 7. Queda de um nó.
  if (data.cells['failover-1kb']) {
    const systems = present('failover-1kb', 3);
    const events = systems.flatMap((s) => cell('failover-1kb', s, 3).events ?? []);
    const at = (action) => { const e = events.filter((x) => x.action === action).map((x) => x.at); return e.length ? Math.round(e.reduce((a, b) => a + b, 0) / e.length) : null; };
    const markers = [['kill', T('node killed')], ['start', T('node back')]].filter(([a]) => at(a) !== null).map(([a, l]) => ({ at: at(a), label: l }));
    write('bench-failover.svg', lineChart({
      title: T('A node is killed under load, 3 nodes'), subtitle: `${T('5,000 msg/s published; messages consumed per second')} · ${local}`,
      unit: 'msg/s', xLabel: T('seconds of the measured phase'), markers, note,
      series: systems.map((s) => ({ key: s, label: name(s), values: cell('failover-1kb', s, 3).series.consume, step: STEP })),
    }));
  }
  // 8. Memória parada e sob carga.
  if (data.startup && Object.keys(data.startup).length) {
    const idle = (s, n) => data.startup[n > 1 ? `${s}-x${n}` : s]?.idleMb ?? NaN;
    const cats = NODES.flatMap(([n, l]) => [[n, 'idle', `${T('at rest')} · ${l}`], [n, 'load', `${T('maximum rate')} · ${l}`]]);
    write('bench-memory.svg', barChart({
      title: T('Server memory'), subtitle: `${T('sum of every node, at rest and the peak at maximum rate')} · ${local}`,
      unit: 'MB', categories: cats.map(([, , l]) => l), note,
      series: SYSTEMS.map((s) => ({ key: s, label: name(s), values: cats.map(([n, k]) => (k === 'idle' ? idle(s, n) : stat('max-rate-1kb', s, n, 'mem'))) })),
    }));
  }
  if (missing.size) console.warn(`aviso: ${missing.size} texto(s) sem tradução em ${lang}: ${[...missing].join(' | ')}`);
}
