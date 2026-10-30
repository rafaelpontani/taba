#!/usr/bin/env node
// Relatório do benchmark local: lê os JSON do OpenMessaging Benchmark de <pasta>/raw e escreve <pasta>/results.md e
// <pasta>/charts/*.svg. Sem dependências (Node 22).
//   node bench/report.mjs bench/results/<pasta>
// Não elege vencedor: mostra os números lado a lado, com a mediana e o intervalo (mínimo a máximo) das rodadas.
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const BANNER = '> **Resultado local, não representativo.** Compara os sistemas entre si, nesta máquina e nesta rodada; não é número absoluto e não vale para outro hardware.';

const NAME = /^([a-z0-9]+)-(default|fsync)-(.+)-(\d+)\.json$/;

// Notas de cada sistema: o que a comparação não esconde.
export const SYSTEM_NOTES = {
  taba: 'Taba: um nó, fator de replicação 1; o corpo no servidor tem 8 bytes a mais que o payload (a hora de publicação); uma requisição HTTP/1.1 por chamada, com lote de leitura de até 100 mensagens.',
  kafka: 'Kafka: KRaft, um nó, fator 1, `acks=all`; o produtor agrupa mensagens (`linger.ms=1`).',
  redpanda: 'Redpanda: um nó, fator 1, medido pelo driver do Kafka; `default` liga o write caching do tópico, `fsync` o desliga.',
  rabbitmq: 'RabbitMQ: um nó, fila de quórum e mensagem persistente (já faz fsync por confirmação, então `default` e `fsync` são iguais).',
  nats: 'NATS JetStream: um nó, fluxo em arquivo; `fsync` usa `sync_interval: always`.',
};

// A confirmação do consumo não é igual nos sistemas, e isso muda a vazão de drenagem.
export const ACK_NOTES = [
  'Confirmação no consumo (workloads `backlog-*`): RabbitMQ e NATS confirmam **cada mensagem**; o Taba também confirma cada mensagem, mas o driver manda as confirmações de uma leitura numa **requisição só** (`POST …/messages/ack`) e publica em lotes de até 100 com 1 ms de espera; Kafka e Redpanda confirmam por **offset, em lote** (`enable.auto.commit=false`, commit a cada leitura do consumidor, como o driver do OMB faz). O custo por mensagem do Kafka e do Redpanda é por isso menor, e o número de drenagem deles não é comparável ponto a ponto com o dos outros.',
  'Drenagem do backlog: a média do consumo desde o pico do backlog, quando o consumidor é liberado, até o backlog acabar. O OMB mantém a produção na taxa configurada durante a drenagem.',
];

const FOOTNOTE_SAME = { rabbitmq: 'a fila de quórum já faz fsync por confirmação: as duas colunas de durabilidade são iguais' };

export function median(values) {
  if (values.length === 0) return NaN;
  const s = [...values].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
}

const mean = (xs) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : NaN);

export function parseName(file) {
  const m = NAME.exec(file);
  return m ? { system: m[1], durability: m[2], workload: m[3], round: Number(m[4]) } : null;
}

// Vazão de consumo de um backlog: a média do consumo depois do pico do backlog (o consumidor é liberado ali) enquanto ainda
// resta backlog. Antes do pico o consumidor está parado e entraria como zero. NaN quando o resultado não tem backlog.
export function drainRate(result) {
  const backlog = result.backlog ?? [];
  const consume = result.consumeRate ?? [];
  const peak = backlog.reduce((best, v, i) => (v > backlog[best] ? i : best), 0);
  if (!(backlog[peak] > 0)) return NaN;
  const rates = [];
  for (let i = peak + 1; i < consume.length && i < backlog.length && backlog[i] > 0; i++) rates.push(consume[i]);
  return mean(rates);
}

// A curva de drenagem: o consumo de cada amostra depois do pico do backlog (a amostra 0 é a primeira com o consumidor solto),
// enquanto ainda resta backlog. Vazia quando o resultado não tem backlog.
export function drainCurve(result) {
  const backlog = result.backlog ?? [];
  const consume = result.consumeRate ?? [];
  const peak = backlog.reduce((best, v, i) => (v > backlog[best] ? i : best), 0);
  if (!(backlog[peak] > 0)) return [];
  const curve = [];
  for (let i = peak + 1; i < consume.length && i < backlog.length && backlog[i] > 0; i++) curve.push(consume[i]);
  return curve;
}

// As medidas de um JSON do OMB. Latências em ms; vazão em mensagens por segundo.
export function metrics(result) {
  return {
    drain: drainRate(result),
    curve: drainCurve(result),
    publish: mean(result.publishRate ?? []),
    consume: mean(result.consumeRate ?? []),
    p50: result.aggregatedEndToEndLatency50pct,
    p99: result.aggregatedEndToEndLatency99pct,
    p999: result.aggregatedEndToEndLatency999pct,
  };
}

// O OMB amostra as taxas a cada 10 s.
const SAMPLE_SECONDS = 10;

const KEYS = ['drain', 'publish', 'consume', 'p50', 'p99', 'p999'];

// Agrupa as rodadas: { workload -> { durability -> { system -> { rounds, stats } } } }.
export function summarize(rawDir) {
  const groups = {};
  for (const file of readdirSync(rawDir).filter((f) => f.endsWith('.json')).sort()) {
    const id = parseName(file);
    if (!id) continue;
    let result;
    try {
      result = JSON.parse(readFileSync(join(rawDir, file), 'utf8'));
    } catch {
      continue;
    }
    const cell = ((groups[id.workload] ??= {})[id.durability] ??= {});
    (cell[id.system] ??= { runs: [] }).runs.push(metrics(result));
  }
  for (const byDurability of Object.values(groups)) {
    for (const bySystem of Object.values(byDurability)) {
      for (const entry of Object.values(bySystem)) {
        entry.stats = {};
        // A mediana das rodadas em cada amostra; a curva vai até onde alguma rodada ainda tinha backlog.
        const longest = Math.max(0, ...entry.runs.map((r) => r.curve.length));
        entry.curve = Array.from({ length: longest }, (_, i) => median(entry.runs.map((r) => r.curve[i]).filter((v) => Number.isFinite(v))));
        for (const key of KEYS) {
          const values = entry.runs.map((r) => r[key]).filter((v) => Number.isFinite(v));
          entry.stats[key] = values.length
            ? { median: median(values), min: Math.min(...values), max: Math.max(...values) }
            : null;
        }
      }
    }
  }
  return groups;
}

function fmt(value, digits) {
  if (value >= 1000) return Math.round(value).toLocaleString('en-US');
  return value.toFixed(digits);
}

function cellText(stat, digits, runs) {
  if (!stat) return 'n/d';
  if (runs <= 1) return fmt(stat.median, digits);
  return `${fmt(stat.median, digits)} (${fmt(stat.min, digits)}–${fmt(stat.max, digits)})`;
}

const COLUMNS = [
  ['publish', 'publicado (msg/s)', 0],
  ['consume', 'consumido (msg/s)', 0],
  ['p50', 'ponta a ponta p50 (ms)', 1],
  ['p99', 'p99 (ms)', 1],
  ['p999', 'p99.9 (ms)', 1],
];

// Os workloads backlog-* medem a leitura de um backlog: a latência ponta a ponta ali é o tempo de espera na fila e não diz nada.
const BACKLOG_COLUMNS = [
  ['drain', 'drenagem do backlog (msg/s)', 0],
  ['publish', 'publicado (msg/s)', 0],
  ['consume', 'consumido, média da rodada (msg/s)', 0],
];

export const isBacklog = (workload) => workload.startsWith('backlog-');

export function table(bySystem, order, columns = COLUMNS) {
  
  const lines = ['| Sistema | Rodadas | ' + columns.map((c) => c[1]).join(' | ') + ' |', '|---|---|' + columns.map(() => '---').join('|') + '|'];
  for (const system of order.filter((s) => bySystem[s])) {
    const entry = bySystem[system];
    const runs = entry.runs.length;
    lines.push(`| ${system} | ${runs} | ` + columns.map(([key, , digits]) => cellText(entry.stats[key], digits, runs)).join(' | ') + ' |');
  }
  return lines.join('\n');
}

// Barras agrupadas em SVG. series: [{ label, values: [..] }] (uma série por sistema), categories: rótulos do eixo x.
export function barChart({ title, unit, categories, series }) {
  const palette = ['#2563eb', '#dc2626', '#059669', '#d97706', '#7c3aed', '#0891b2', '#be185d'];
  const width = 720, height = 360, left = 64, right = 16, top = 48, bottom = 64;
  const plotW = width - left - right, plotH = height - top - bottom;
  const max = Math.max(1e-9, ...series.flatMap((s) => s.values.filter(Number.isFinite)));
  const groupW = plotW / Math.max(1, categories.length);
  const barW = Math.min(36, (groupW * 0.8) / Math.max(1, series.length));
  const esc = (t) => String(t).replace(/&/g, '&amp;').replace(/</g, '&lt;');
  const out = [
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}" width="${width}" height="${height}" font-family="sans-serif" font-size="12">`,
    `<rect width="${width}" height="${height}" fill="#ffffff"/>`,
    `<text x="${left}" y="24" font-size="15" font-weight="bold" fill="#111827">${esc(title)}</text>`,
    `<text x="${left}" y="40" fill="#4b5563">${esc(unit)} · local, não representativo</text>`,
  ];
  for (let i = 0; i <= 4; i++) {
    const y = top + plotH - (plotH * i) / 4;
    out.push(`<line x1="${left}" y1="${y}" x2="${width - right}" y2="${y}" stroke="#e5e7eb"/>`);
    out.push(`<text x="${left - 6}" y="${y + 4}" text-anchor="end" fill="#4b5563">${fmt((max * i) / 4, 1)}</text>`);
  }
  categories.forEach((cat, ci) => {
    const gx = left + ci * groupW + (groupW - barW * series.length) / 2;
    series.forEach((s, si) => {
      const v = s.values[ci];
      if (!Number.isFinite(v)) return;
      const h = (plotH * v) / max;
      out.push(`<rect x="${gx + si * barW}" y="${top + plotH - h}" width="${barW - 2}" height="${h}" fill="${palette[si % palette.length]}"><title>${esc(s.label)}: ${fmt(v, 1)} ${esc(unit)}</title></rect>`);
    });
    out.push(`<text x="${left + ci * groupW + groupW / 2}" y="${top + plotH + 18}" text-anchor="middle" fill="#111827">${esc(cat)}</text>`);
  });
  series.forEach((s, si) => {
    const x = left + si * 110;
    out.push(`<rect x="${x}" y="${height - 28}" width="12" height="12" fill="${palette[si % palette.length]}"/>`);
    out.push(`<text x="${x + 16}" y="${height - 18}" fill="#111827">${esc(s.label)}</text>`);
  });
  out.push('</svg>');
  return out.join('\n') + '\n';
}

// Linhas em SVG. series: [{ label, values: [..] }] (uma linha por sistema), values[i] é o ponto i do eixo x (step segundos cada).
export function lineChart({ title, unit, xLabel, step, series }) {
  const palette = ['#2563eb', '#dc2626', '#059669', '#d97706', '#7c3aed', '#0891b2', '#be185d'];
  const width = 720, height = 360, left = 64, right = 16, top = 48, bottom = 72;
  const plotW = width - left - right, plotH = height - top - bottom;
  const points = Math.max(2, ...series.map((s) => s.values.length));
  const max = Math.max(1e-9, ...series.flatMap((s) => s.values.filter(Number.isFinite)));
  const x = (i) => left + (plotW * i) / (points - 1);
  const y = (v) => top + plotH - (plotH * v) / max;
  const esc = (t) => String(t).replace(/&/g, '&amp;').replace(/</g, '&lt;');
  const out = [
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}" width="${width}" height="${height}" font-family="sans-serif" font-size="12">`,
    `<rect width="${width}" height="${height}" fill="#ffffff"/>`,
    `<text x="${left}" y="24" font-size="15" font-weight="bold" fill="#111827">${esc(title)}</text>`,
    `<text x="${left}" y="40" fill="#4b5563">${esc(unit)} · local, não representativo</text>`,
  ];
  for (let i = 0; i <= 4; i++) {
    const gy = top + plotH - (plotH * i) / 4;
    out.push(`<line x1="${left}" y1="${gy}" x2="${width - right}" y2="${gy}" stroke="#e5e7eb"/>`);
    out.push(`<text x="${left - 6}" y="${gy + 4}" text-anchor="end" fill="#4b5563">${fmt((max * i) / 4, 1)}</text>`);
  }
  const ticks = Math.min(points - 1, 6);
  for (let t = 0; t <= ticks; t++) {
    const i = Math.round(((points - 1) * t) / Math.max(1, ticks));
    out.push(`<text x="${x(i)}" y="${top + plotH + 16}" text-anchor="middle" fill="#4b5563">${Math.round(i * step)}</text>`);
  }
  out.push(`<text x="${left + plotW / 2}" y="${top + plotH + 32}" text-anchor="middle" fill="#4b5563">${esc(xLabel)}</text>`);
  series.forEach((s, si) => {
    const color = palette[si % palette.length];
    const pts = s.values.map((v, i) => (Number.isFinite(v) ? `${x(i).toFixed(1)},${y(v).toFixed(1)}` : null)).filter(Boolean);
    if (pts.length > 1) out.push(`<polyline points="${pts.join(' ')}" fill="none" stroke="${color}" stroke-width="2"><title>${esc(s.label)}</title></polyline>`);
    s.values.forEach((v, i) => {
      if (Number.isFinite(v)) out.push(`<circle cx="${x(i).toFixed(1)}" cy="${y(v).toFixed(1)}" r="3" fill="${color}"><title>${esc(s.label)}: ${fmt(v, 1)} ${esc(unit)} (${Math.round(i * step)} s)</title></circle>`);
    });
    const lx = left + si * 110;
    out.push(`<rect x="${lx}" y="${height - 20}" width="12" height="12" fill="${color}"/>`);
    out.push(`<text x="${lx + 16}" y="${height - 10}" fill="#111827">${esc(s.label)}</text>`);
  });
  out.push('</svg>');
  return out.join('\n') + '\n';
}

export function generate(dir) {
  const rawDir = join(dir, 'raw');
  if (!existsSync(rawDir)) throw new Error(`não há ${rawDir}`);
  const groups = summarize(rawDir);
  const machine = existsSync(join(dir, 'machine.json')) ? JSON.parse(readFileSync(join(dir, 'machine.json'), 'utf8')) : {};
  const failed = existsSync(join(dir, 'failed.txt')) ? readFileSync(join(dir, 'failed.txt'), 'utf8').trim().split('\n').filter(Boolean) : [];
  const order = ['taba', 'kafka', 'redpanda', 'rabbitmq', 'nats'];
  const systems = new Set();
  for (const w of Object.values(groups)) for (const d of Object.values(w)) for (const s of Object.keys(d)) systems.add(s);
  const sorted = [...order.filter((s) => systems.has(s)), ...[...systems].filter((s) => !order.includes(s)).sort()];

  mkdirSync(join(dir, 'charts'), { recursive: true });
  const md = ['# Benchmark local', '', BANNER, ''];
  md.push('## Máquina e limites', '');
  md.push(`- Data: ${machine.date ?? 'n/d'}; commit do Taba: ${machine.taba_commit ?? 'n/d'}; commit do OMB: ${machine.omb_commit ?? 'n/d'}`);
  md.push(`- CPU: ${machine.cpu_model ?? 'n/d'}; o Docker enxerga ${machine.docker_cpus ?? '?'} núcleos e ${machine.docker_memory_gb ?? '?'} GB (${machine.docker_os ?? '?'}, Docker ${machine.docker_version ?? '?'})`);
  md.push(`- Servidor de cada sistema: ${machine.server_cpus ?? '?'} núcleos e ${machine.server_memory_gb ?? '?'} GB (igual para todos); executor do OMB: ${machine.runner_cpus ?? '?'} núcleos e ${machine.runner_memory_gb ?? '?'} GB`);
  md.push(`- Perfil ${machine.profile ?? '?'}: ${machine.rounds ?? '?'} rodada(s) de ${machine.duration_minutes ?? '?'} min, com ${machine.warmup_minutes ?? '?'} min de aquecimento que não entram nos percentis`);
  if (machine.images) md.push(`- Imagens: ${Object.entries(machine.images).filter(([, v]) => v).map(([k, v]) => `${k} \`${v}\``).join(', ')}`);
  md.push('');

  const footnotes = new Set();
  for (const workload of Object.keys(groups).sort()) {
    md.push(`## ${workload}`, '');
    for (const durability of ['default', 'fsync']) {
      const bySystem = groups[workload][durability];
      if (!bySystem) continue;
      md.push(`### Durabilidade: ${durability}`, '', table(bySystem, sorted, isBacklog(workload) ? BACKLOG_COLUMNS : COLUMNS), '');
      for (const s of Object.keys(bySystem)) if (FOOTNOTE_SAME[s] && groups[workload].default && groups[workload].fsync) footnotes.add(`${s}: ${FOOTNOTE_SAME[s]}`);
      const present = sorted.filter((s) => bySystem[s]);
      if (isBacklog(workload)) {
        const drain = barChart({
          title: `${workload} — drenagem do backlog (${durability})`,
          unit: 'msg/s',
          categories: ['drenagem'],
          series: present.map((s) => ({ label: s, values: [bySystem[s].stats.drain?.median ?? NaN] })),
        });
        writeFileSync(join(dir, 'charts', `${workload}-${durability}-drain.svg`), drain);
        const curve = lineChart({
          title: `${workload} — consumo ao longo da drenagem (${durability})`,
          unit: 'msg/s',
          xLabel: 'segundos depois de soltar o consumidor (mediana das rodadas)',
          step: SAMPLE_SECONDS,
          series: present.map((s) => ({ label: s, values: bySystem[s].curve })),
        });
        writeFileSync(join(dir, 'charts', `${workload}-${durability}-curve.svg`), curve);
        md.push(`![drenagem](charts/${workload}-${durability}-drain.svg) ![curva](charts/${workload}-${durability}-curve.svg)`, '');
        continue;
      }
      const latency = barChart({
        title: `${workload} — latência ponta a ponta (${durability})`,
        unit: 'ms',
        categories: ['p50', 'p99', 'p99.9'],
        series: present.map((s) => ({ label: s, values: [bySystem[s].stats.p50?.median, bySystem[s].stats.p99?.median, bySystem[s].stats.p999?.median].map((v) => v ?? NaN) })),
      });
      const throughput = barChart({
        title: `${workload} — vazão (${durability})`,
        unit: 'msg/s',
        categories: ['publicado', 'consumido'],
        series: present.map((s) => ({ label: s, values: [bySystem[s].stats.publish?.median ?? NaN, bySystem[s].stats.consume?.median ?? NaN] })),
      });
      writeFileSync(join(dir, 'charts', `${workload}-${durability}-latency.svg`), latency);
      writeFileSync(join(dir, 'charts', `${workload}-${durability}-throughput.svg`), throughput);
      md.push(`![latência](charts/${workload}-${durability}-latency.svg) ![vazão](charts/${workload}-${durability}-throughput.svg)`, '');
    }
  }
  if (failed.length) {
    md.push('## Células que falharam', '', ...failed.map((f) => `- ${f}`), '');
  }
  md.push('## Diferenças conhecidas', '');
  for (const s of sorted) if (SYSTEM_NOTES[s]) md.push(`- ${SYSTEM_NOTES[s]}`);
  for (const f of footnotes) md.push(`- ${f}`);
  if (Object.keys(groups).some(isBacklog)) md.push(...ACK_NOTES);
  md.push('- Um nó por sistema: o cluster de três nós não é medido aqui.', '- A tabela mostra a mediana das rodadas e, entre parênteses, o mínimo e o máximo; os sistemas não são ordenados.', '');
  writeFileSync(join(dir, 'results.md'), md.join('\n'));
  return join(dir, 'results.md');
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const dir = process.argv[2];
  if (!dir) {
    console.error('uso: node bench/report.mjs <pasta de resultados>');
    process.exit(2);
  }
  try {
    console.log(`relatório: ${generate(dir)}`);
  } catch (e) {
    console.error(`erro: ${e.message}`);
    process.exit(1);
  }
}
