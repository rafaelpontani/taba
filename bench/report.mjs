#!/usr/bin/env node
// Relatório do benchmark local: lê os JSON do OpenMessaging Benchmark de <pasta>/raw (e as amostras de CPU e memória que o
// run.sh grava ao lado) e escreve <pasta>/results.md, <pasta>/summary.json e <pasta>/charts/*.svg. Sem dependências (Node 22).
//   node bench/report.mjs bench/results/<pasta>
// Não elege vencedor: mostra os números lado a lado, com a mediana e o intervalo (mínimo a máximo) das rodadas.
// O summary.json é o que doc/diagrams/scripts/generate-bench.mjs lê para os gráficos da documentação.
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { SYSTEM_NAMES, barChart, cumulative, lineChart } from './charts.mjs';

export { barChart, lineChart };

export const BANNER = '> **Resultado local, não representativo.** Compara os sistemas entre si, nesta máquina e nesta rodada; não é número absoluto e não vale para outro hardware.';

const NAME = /^([a-z0-9]+)(?:-x(\d+))?-(default|fsync)-(.+)-(\d+)\.json$/;

// Notas de cada sistema: o que a comparação não esconde.
export const SYSTEM_NOTES = {
  taba: 'Taba: um nó com fator de replicação 1, ou três nós com fator 3 atrás de um HAProxy (1 núcleo, contado nos recursos do Taba); o corpo no servidor tem 8 bytes a mais que o payload (a hora de publicação); uma requisição HTTP/1.1 por chamada, com lote de leitura de até 500 mensagens.',
  kafka: 'Kafka: KRaft, um nó, fator 1, `acks=all`; o produtor agrupa mensagens (`linger.ms=1`).',
  redpanda: 'Redpanda: um nó, fator 1, medido pelo driver do Kafka; `default` liga o write caching do tópico, `fsync` o desliga.',
  rabbitmq: 'RabbitMQ: fila de quórum e mensagem persistente (já faz fsync por confirmação, então `default` e `fsync` são iguais); no cluster, um membro da fila em cada nó e o cliente conectado direto nos nós.',
  nats: 'NATS JetStream: fluxo em arquivo; `fsync` usa `sync_interval: always`; no cluster, fluxo com 3 réplicas e o cliente conectado direto nos nós.',
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
  return m ? { system: m[1], nodes: Number(m[2] ?? 1), durability: m[3], workload: m[4], round: Number(m[5]) } : null;
}

// A chave de um sistema num tamanho de cluster: "taba" (um nó) ou "taba-x3".
export const systemKey = (system, nodes) => (nodes > 1 ? `${system}-x${nodes}` : system);
// O nome para mostrar: "Taba" ou "Taba ×3".
export const systemLabel = (system, nodes) => `${SYSTEM_NAMES[system] ?? system}${nodes > 1 ? ` ×${nodes}` : ''}`;

// Vazão de consumo de um backlog: a média do consumo depois do pico do backlog (o consumidor é liberado ali) enquanto ainda
// resta backlog. Antes do pico o consumidor está parado e entraria como zero. NaN quando o resultado não tem backlog.
export function drainRate(result) {
  return mean(drainCurve(result));
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

// O OMB amostra as taxas a cada 10 s.
export const SAMPLE_SECONDS = 10;

// As amostras de CPU e memória de uma célula (arquivo <célula>.stats, linhas "<epoch>;<contêiner>;<núcleos>;<mb>"), somadas
// por instante sobre os contêineres do servidor e contadas em segundos desde o início da fase medida (<célula>.t0).
// Devolve { t: [], cpu: [], mem: [] } ou null.
export function readResources(base) {
  if (!existsSync(`${base}.stats`) || !existsSync(`${base}.t0`)) return null;
  const t0 = Number(readFileSync(`${base}.t0`, 'utf8').trim());
  if (!Number.isFinite(t0) || t0 <= 0) return null;
  const byTime = new Map();
  for (const line of readFileSync(`${base}.stats`, 'utf8').split('\n')) {
    const [epoch, , cpu, mem] = line.trim().split(';');
    if (!mem) continue;
    const e = Number(epoch);
    const acc = byTime.get(e) ?? { cpu: 0, mem: 0 };
    acc.cpu += Number(cpu) || 0;
    acc.mem += Number(mem) || 0;
    byTime.set(e, acc);
  }
  const times = [...byTime.keys()].sort((a, b) => a - b);
  return { t: times.map((e) => e - t0), cpu: times.map((e) => byTime.get(e).cpu), mem: times.map((e) => byTime.get(e).mem) };
}

// Os eventos de falha de uma célula (<célula>.events, linhas "<segundos>;<ação>;<serviço>").
export function readEvents(base) {
  if (!existsSync(`${base}.events`)) return [];
  return readFileSync(`${base}.events`, 'utf8').split('\n').map((l) => l.trim().split(';')).filter((p) => p.length === 3)
    .map(([at, action, service]) => ({ at: Number(at), action, service }));
}

// As medidas de um JSON do OMB (latências em ms, vazão em mensagens por segundo) e dos recursos medidos ao lado.
export function metrics(result, resources = null, events = []) {
  const publish = result.publishRate ?? [];
  const consume = result.consumeRate ?? [];
  const seconds = consume.length * SAMPLE_SECONDS;
  // CPU e memória só dentro da fase medida.
  const inside = resources ? resources.t.map((t, i) => [t, i]).filter(([t]) => t >= 0 && t <= seconds).map(([, i]) => i) : [];
  const cpu = mean(inside.map((i) => resources.cpu[i]));
  const consumeMean = mean(consume);
  return {
    drain: drainRate(result),
    curve: drainCurve(result),
    publish: mean(publish),
    consume: consumeMean,
    p50: result.aggregatedEndToEndLatency50pct,
    p99: result.aggregatedEndToEndLatency99pct,
    p999: result.aggregatedEndToEndLatency999pct,
    cpu,
    mem: inside.length ? Math.max(...inside.map((i) => resources.mem[i])) : NaN,
    perCore: cpu > 0 ? consumeMean / cpu : NaN,
    errors: (result.publishErrorRate ?? []).reduce((a, b) => a + (b || 0), 0) * SAMPLE_SECONDS,
    series: { publish, consume, backlog: result.backlog ?? [] },
    resources,
    events,
  };
}

const KEYS = ['drain', 'publish', 'consume', 'p50', 'p99', 'p999', 'cpu', 'mem', 'perCore', 'errors'];

// A mediana, amostra a amostra, de várias séries.
const medianSeries = (list) => {
  const longest = Math.max(0, ...list.map((s) => s.length));
  return Array.from({ length: longest }, (_, i) => median(list.map((s) => s[i]).filter(Number.isFinite)));
};

// Agrupa as rodadas: { workload -> { durability -> { chave do sistema -> { system, nodes, runs, stats, ... } } } }.
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
    const base = join(rawDir, file.slice(0, -5));
    const cell = ((groups[id.workload] ??= {})[id.durability] ??= {});
    const key = systemKey(id.system, id.nodes);
    (cell[key] ??= { system: id.system, nodes: id.nodes, runs: [] }).runs.push(metrics(result, readResources(base), readEvents(base)));
  }
  for (const byDurability of Object.values(groups)) {
    for (const bySystem of Object.values(byDurability)) {
      for (const entry of Object.values(bySystem)) {
        entry.stats = {};
        // A mediana das rodadas em cada amostra; a curva vai até onde alguma rodada ainda tinha backlog.
        entry.curve = medianSeries(entry.runs.map((r) => r.curve));
        entry.series = {
          publish: medianSeries(entry.runs.map((r) => r.series.publish)),
          consume: medianSeries(entry.runs.map((r) => r.series.consume)),
          backlog: medianSeries(entry.runs.map((r) => r.series.backlog)),
        };
        // Recursos e eventos: os da primeira rodada que os tem (as amostras de rodadas diferentes não caem no mesmo instante).
        entry.resources = entry.runs.find((r) => r.resources)?.resources ?? null;
        entry.events = entry.runs.find((r) => r.events.length)?.events ?? [];
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

// O tempo de subida e a memória parada (startup.csv do run.sh): { chave do sistema -> { seconds, idleMb } } (medianas).
export function readStartup(dir) {
  const file = join(dir, 'startup.csv');
  if (!existsSync(file)) return {};
  const acc = {};
  for (const line of readFileSync(file, 'utf8').split('\n').slice(1)) {
    const [system, nodes, , , seconds, idle] = line.trim().split(';');
    if (!idle) continue;
    const e = (acc[systemKey(system, Number(nodes))] ??= { system, nodes: Number(nodes), seconds: [], idleMb: [] });
    e.seconds.push(Number(seconds));
    e.idleMb.push(Number(idle));
  }
  for (const e of Object.values(acc)) { e.seconds = median(e.seconds); e.idleMb = median(e.idleMb); }
  return acc;
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

const RESOURCE_COLUMNS = [
  ['cpu', 'CPU média (núcleos)', 2],
  ['mem', 'memória máx. (MB)', 0],
  ['perCore', 'msg/s por núcleo', 0],
];

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
  const present = order.filter((s) => bySystem[s]);
  // As colunas de recursos só quando alguma linha as tem.
  const cols = present.some((s) => bySystem[s].stats.cpu) ? [...columns, ...RESOURCE_COLUMNS] : columns;
  const lines = ['| Sistema | Rodadas | ' + cols.map((c) => c[1]).join(' | ') + ' |', '|---|---|' + cols.map(() => '---').join('|') + '|'];
  for (const key of present) {
    const entry = bySystem[key];
    const runs = entry.runs.length;
    lines.push(`| ${key} | ${runs} | ` + cols.map(([k, , digits]) => cellText(entry.stats[k], digits, runs)).join(' | ') + ' |');
  }
  return lines.join('\n');
}

// A série de tempo de um sistema: consumido (cheio) e publicado (tracejado), na mesma cor.
export function timelineSeries(present, bySystem, labels = {}) {
  return present.flatMap((key) => {
    const e = bySystem[key];
    const label = labels.system?.(e) ?? systemLabel(e.system, e.nodes);
    return [
      { key: e.system, label: `${label} ${labels.published ?? 'publicado'}`, values: e.series.publish, step: SAMPLE_SECONDS, dash: true },
      { key: e.system, label: `${label} ${labels.consumed ?? 'consumido'}`, endLabel: label, values: e.series.consume, step: SAMPLE_SECONDS },
    ];
  });
}

const ORDER = ['taba', 'rabbitmq', 'nats', 'kafka', 'redpanda'];

// A ordem das chaves: por número de nós, depois a ordem fixa dos sistemas.
export function sortKeys(keys) {
  const parse = (k) => { const m = /^(.+?)(?:-x(\d+))?$/.exec(k); return [Number(m[2] ?? 1), ORDER.includes(m[1]) ? ORDER.indexOf(m[1]) : 99, m[1]]; };
  return [...keys].sort((a, b) => {
    const [na, oa, sa] = parse(a), [nb, ob, sb] = parse(b);
    return na - nb || oa - ob || sa.localeCompare(sb);
  });
}

export function generate(dir) {
  const rawDir = join(dir, 'raw');
  if (!existsSync(rawDir)) throw new Error(`não há ${rawDir}`);
  const groups = summarize(rawDir);
  const startup = readStartup(dir);
  const machine = existsSync(join(dir, 'machine.json')) ? JSON.parse(readFileSync(join(dir, 'machine.json'), 'utf8')) : {};
  const failed = existsSync(join(dir, 'failed.txt')) ? readFileSync(join(dir, 'failed.txt'), 'utf8').trim().split('\n').filter(Boolean) : [];
  const keys = new Set();
  for (const w of Object.values(groups)) for (const d of Object.values(w)) for (const s of Object.keys(d)) keys.add(s);
  const sorted = sortKeys(keys);

  mkdirSync(join(dir, 'charts'), { recursive: true });
  const chart = (name, svg) => writeFileSync(join(dir, 'charts', name), svg);
  const md = ['# Benchmark local', '', BANNER, ''];
  md.push('## Máquina e limites', '');
  md.push(`- Data: ${machine.date ?? 'n/d'}; commit do Taba: ${machine.taba_commit ?? 'n/d'}; commit do OMB: ${machine.omb_commit ?? 'n/d'}`);
  md.push(`- CPU: ${machine.cpu_model ?? 'n/d'}; o Docker enxerga ${machine.docker_cpus ?? '?'} núcleos e ${machine.docker_memory_gb ?? '?'} GB (${machine.docker_os ?? '?'}, Docker ${machine.docker_version ?? '?'})`);
  md.push(`- Servidor de cada sistema: ${machine.server_cpus ?? '?'} núcleos e ${machine.server_memory_gb ?? '?'} GB (igual para todos); executor do OMB: ${machine.runner_cpus ?? '?'} núcleos e ${machine.runner_memory_gb ?? '?'} GB`);
  if (String(machine.nodes ?? '').includes('3')) md.push(`- Cluster: 3 nós de ${machine.cluster_node_cpus ?? '?'} núcleos e ${machine.cluster_node_memory_mb ?? '?'} MB cada (igual para todos)`);
  md.push(`- Perfil ${machine.profile ?? '?'}: ${machine.rounds ?? '?'} rodada(s) de ${machine.duration_minutes ?? '?'} min, com ${machine.warmup_minutes ?? '?'} min de aquecimento que não entram nos percentis`);
  if (machine.images) md.push(`- Imagens: ${Object.entries(machine.images).filter(([, v]) => v).map(([k, v]) => `${k} \`${v}\``).join(', ')}`);
  md.push('');

  const startupKeys = sortKeys(Object.keys(startup));
  if (startupKeys.length) {
    md.push('## Subida e memória parada', '', '| Sistema | até ficar saudável (s) | memória parada (MB, soma dos contêineres) |', '|---|---|---|');
    for (const k of startupKeys) md.push(`| ${k} | ${startup[k].seconds} | ${startup[k].idleMb} |`);
    md.push('');
  }

  const footnotes = new Set();
  for (const workload of Object.keys(groups).sort()) {
    md.push(`## ${workload}`, '');
    for (const durability of ['default', 'fsync']) {
      const bySystem = groups[workload][durability];
      if (!bySystem) continue;
      md.push(`### Durabilidade: ${durability}`, '', table(bySystem, sorted, isBacklog(workload) ? BACKLOG_COLUMNS : COLUMNS), '');
      for (const s of Object.values(bySystem)) if (FOOTNOTE_SAME[s.system] && groups[workload].default && groups[workload].fsync) footnotes.add(`${s.system}: ${FOOTNOTE_SAME[s.system]}`);
      const present = sorted.filter((s) => bySystem[s]);
      const id = `${workload}-${durability}`;
      const images = [];
      if (isBacklog(workload)) {
        chart(`${id}-drain.svg`, barChart({
          title: `${workload}: drenagem do backlog (${durability})`, subtitle: 'local, não representativo', unit: 'msg/s',
          categories: ['drenagem'],
          series: present.map((k) => ({ key: bySystem[k].system, label: k, values: [bySystem[k].stats.drain?.median ?? NaN] })),
        }));
        chart(`${id}-curve.svg`, lineChart({
          title: `${workload}: consumo ao longo da drenagem (${durability})`, subtitle: 'local, não representativo', unit: 'msg/s',
          xLabel: 'segundos depois de soltar o consumidor (mediana das rodadas)',
          series: present.map((k) => ({ key: bySystem[k].system, label: k, values: bySystem[k].curve, step: SAMPLE_SECONDS, start: 0 })),
        }));
        images.push(`![drenagem](charts/${id}-drain.svg)`, `![curva](charts/${id}-curve.svg)`);
      } else {
        chart(`${id}-latency.svg`, barChart({
          title: `${workload}: latência ponta a ponta (${durability})`, subtitle: 'local, não representativo', unit: 'ms',
          categories: ['p50', 'p99', 'p99.9'],
          series: present.map((k) => ({ key: bySystem[k].system, label: k, values: [bySystem[k].stats.p50?.median, bySystem[k].stats.p99?.median, bySystem[k].stats.p999?.median].map((v) => v ?? NaN) })),
        }));
        chart(`${id}-throughput.svg`, barChart({
          title: `${workload}: vazão (${durability})`, subtitle: 'local, não representativo', unit: 'msg/s',
          categories: ['publicado', 'consumido'],
          series: present.map((k) => ({ key: bySystem[k].system, label: k, values: [bySystem[k].stats.publish?.median ?? NaN, bySystem[k].stats.consume?.median ?? NaN] })),
        }));
        images.push(`![latência](charts/${id}-latency.svg)`, `![vazão](charts/${id}-throughput.svg)`);
      }
      // Linha do tempo e mensagens acumuladas, um gráfico por tamanho de cluster (as cores seguem o sistema).
      for (const nodes of [...new Set(present.map((k) => bySystem[k].nodes))]) {
        const same = present.filter((k) => bySystem[k].nodes === nodes);
        const suffix = nodes > 1 ? `-x${nodes}` : '';
        const where = nodes > 1 ? `${nodes} nós` : '1 nó';
        const events = same.flatMap((k) => bySystem[k].events);
        const markers = [...new Map(events.map((e) => [`${e.action}-${e.service}`, e])).values()]
          .map((e) => ({ at: e.at, label: `${e.action === 'kill' ? 'queda' : 'volta'} de ${e.service}` }));
        chart(`${id}${suffix}-timeline.svg`, lineChart({
          title: `${workload}: mensagens por segundo no tempo, ${where} (${durability})`,
          subtitle: 'consumido (linha cheia) e publicado (tracejada) · local, não representativo', unit: 'msg/s',
          xLabel: 'segundos da fase medida', series: timelineSeries(same, bySystem), markers,
        }));
        chart(`${id}${suffix}-cumulative.svg`, lineChart({
          title: `${workload}: mensagens processadas acumuladas, ${where} (${durability})`,
          subtitle: 'consumidas e confirmadas desde o início da fase medida · local, não representativo', unit: 'mensagens',
          xLabel: 'segundos da fase medida', markers,
          series: same.map((k) => ({ key: bySystem[k].system, label: k, values: cumulative(bySystem[k].series.consume, SAMPLE_SECONDS), step: SAMPLE_SECONDS })),
        }));
        images.push(`![tempo](charts/${id}${suffix}-timeline.svg)`, `![acumulado](charts/${id}${suffix}-cumulative.svg)`);
        const withResources = same.filter((k) => bySystem[k].resources);
        if (withResources.length) {
          chart(`${id}${suffix}-cpu.svg`, lineChart({
            title: `${workload}: CPU do servidor, ${where} (${durability})`, subtitle: 'soma dos contêineres · local, não representativo',
            unit: 'núcleos', xLabel: 'segundos da fase medida', markers,
            series: withResources.map((k) => ({ key: bySystem[k].system, label: k, values: bySystem[k].resources.cpu, step: 1, pts: bySystem[k].resources.t })).map(toTimed),
          }));
          chart(`${id}${suffix}-memory.svg`, lineChart({
            title: `${workload}: memória do servidor, ${where} (${durability})`, subtitle: 'soma dos contêineres · local, não representativo',
            unit: 'MB', xLabel: 'segundos da fase medida', markers,
            series: withResources.map((k) => ({ key: bySystem[k].system, label: k, values: bySystem[k].resources.mem, pts: bySystem[k].resources.t })).map(toTimed),
          }));
          images.push(`![cpu](charts/${id}${suffix}-cpu.svg)`, `![memória](charts/${id}${suffix}-memory.svg)`);
        }
      }
      md.push(images.join(' '), '');
    }
  }
  if (failed.length) {
    md.push('## Células que falharam', '', ...failed.map((f) => `- ${f}`), '');
  }
  md.push('## Diferenças conhecidas', '');
  for (const s of [...new Set(sorted.map((k) => k.replace(/-x\d+$/, '')))]) if (SYSTEM_NOTES[s]) md.push(`- ${SYSTEM_NOTES[s]}`);
  for (const f of footnotes) md.push(`- ${f}`);
  if (Object.keys(groups).some(isBacklog)) md.push(...ACK_NOTES.map((n) => `- ${n}`));
  if (!sorted.some((k) => /-x\d+$/.test(k))) md.push('- Um nó por sistema: o cluster de três nós não foi medido nesta rodada.');
  md.push('- CPU e memória: amostras do `docker stats` a cada poucos segundos, somadas sobre os contêineres do servidor (o executor do OMB fica de fora); msg/s por núcleo é o consumido médio dividido pela CPU média.', '- A tabela mostra a mediana das rodadas e, entre parênteses, o mínimo e o máximo; os sistemas não são ordenados.', '');
  writeFileSync(join(dir, 'results.md'), md.join('\n'));
  writeFileSync(join(dir, 'summary.json'), JSON.stringify(summaryJson(groups, startup, machine), null, 1) + '\n');
  return join(dir, 'results.md');
}

// Uma série de recursos tem instantes irregulares: vira pontos com `times` no lugar de `step`.
function toTimed(s) {
  const pairs = s.pts.map((t, i) => [t, s.values[i]]).filter(([t]) => t >= 0);
  return { key: s.key, label: s.label, values: pairs.map(([, v]) => v), times: pairs.map(([t]) => t) };
}

// O resumo que a documentação lê: sem as rodadas cruas, com as séries e as medianas.
export function summaryJson(groups, startup, machine) {
  const round = (v, d = 1) => (Number.isFinite(v) ? Math.round(v * 10 ** d) / 10 ** d : null);
  const cells = {};
  for (const [workload, byDurability] of Object.entries(groups)) {
    for (const [durability, bySystem] of Object.entries(byDurability)) {
      for (const [key, e] of Object.entries(bySystem)) {
        ((cells[workload] ??= {})[durability] ??= {})[key] = {
          system: e.system, nodes: e.nodes, runs: e.runs.length,
          stats: Object.fromEntries(Object.entries(e.stats).map(([k, v]) => [k, v ? round(v.median, 2) : null])),
          series: { publish: e.series.publish.map((v) => round(v)), consume: e.series.consume.map((v) => round(v)), backlog: e.series.backlog.map((v) => round(v, 0)) },
          curve: e.curve.map((v) => round(v)),
          resources: e.resources ? { t: e.resources.t, cpu: e.resources.cpu.map((v) => round(v, 2)), mem: e.resources.mem.map((v) => round(v, 0)) } : null,
          events: e.events,
        };
      }
    }
  }
  return { sampleSeconds: SAMPLE_SECONDS, machine, startup, cells };
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
