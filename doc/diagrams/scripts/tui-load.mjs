#!/usr/bin/env node
// Gera carga num cluster Taba para as capturas da TUI (capture-tui.sh): cria alguns tópicos e
// publica/consome com ritmos que variam no tempo, para os gráficos da fila mostrarem a
// profundidade subindo e descendo. Não é benchmark: o ritmo é fixo de propósito.
//
//   node doc/diagrams/scripts/tui-load.mjs --url http://localhost:8080 --token-file ./root-token --seconds 300
//
// Sem dependências (Node 22: fetch nativo).

import { readFileSync } from 'node:fs';

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith('--') ? [...acc, [a.slice(2), all[i + 1]]] : acc), []),
);
const URL_BASE = args.url ?? process.env.TABA_URL ?? 'http://localhost:8080';
const TOKEN = args['token-file'] ? readFileSync(args['token-file'], 'utf8').trim() : process.env.TABA_TOKEN ?? '';
const SECONDS = Number(args.seconds ?? 300);
const TICK_MS = 100;

const headers = TOKEN ? { Authorization: `Bearer ${TOKEN}` } : {};

async function call(method, path, body) {
  const res = await fetch(URL_BASE + path, {
    method,
    headers: body === undefined ? headers : { ...headers, 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  return { status: res.status, json: text ? safeJson(text) : null };
}

function safeJson(text) {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

// Tópicos da demonstração. `pub` e `con` são mensagens por segundo em função do tempo t (s):
// produtor e consumidor fora de fase, para a fila acumular e depois ser drenada.
const QUEUES = [
  { name: 'orders', fifo: false, pub: (t) => 150 + 100 * Math.sin(t / 30), con: (t) => 150 + 100 * Math.sin(t / 30 - 1.6) },
  { name: 'payments', fifo: true, pub: (t) => 40 + 25 * Math.sin(t / 20), con: () => 40 },
  { name: 'invoices', fifo: false, pub: (t) => (Math.floor(t / 45) % 2 === 0 ? 80 : 5), con: () => 40 },
  { name: 'emails', fifo: false, pub: () => 20, con: () => 20 },
];
const DISTRIBUTOR = { name: 'events', prefixes: ['events-'] };
const FANOUT = ['events-audit', 'events-billing'];

async function create(body) {
  const r = await call('POST', '/topics', body);
  if (r.status !== 201 && r.status !== 409) throw new Error(`criar ${body.name}: ${r.status} ${JSON.stringify(r.json)}`);
}

async function setup() {
  for (const q of QUEUES) await create({ name: q.name, type: 'queue', fifo: q.fifo, tags: { team: 'demo' } });
  for (const name of FANOUT) await create({ name, type: 'queue' });
  await create({ name: DISTRIBUTOR.name, type: 'distributor', prefixes: DISTRIBUTOR.prefixes });
}

let seq = 0;
const counts = { published: 0, consumed: 0, failed: {} };
async function publish(q) {
  const n = ++seq;
  const query = q.fifo ? `?groupId=customer-${n % 7}` : '';
  const r = await call('POST', `/topics/${q.name}/messages${query}`, { id: n, kind: q.name, amount: (n * 37) % 1000 });
  if (r.status === 201 || r.status === 200) counts.published++; // 200: duplicata FIFO de uma execução anterior
  else counts.failed[r.status] = (counts.failed[r.status] ?? 0) + 1;
}

async function consume(name, max) {
  const r = await call('GET', `/topics/${name}/messages?maxMessages=${Math.min(10, Math.max(1, max))}&visibilityTimeout=30`);
  const messages = r.json?.messages ?? [];
  await Promise.all(
    messages.map((m) => call('DELETE', `/topics/${name}/messages/${m.id}?receiptToken=${encodeURIComponent(m.receiptToken)}`)),
  );
  counts.consumed += messages.length;
  return messages.length;
}

// Cada ritmo roda no seu próprio laço (um produtor lento não segura os outros): a cada tique
// acumula a fração de mensagens que o ritmo pede e envia as inteiras.
async function pace(rateAt, act, start) {
  let debt = 0;
  while ((Date.now() - start) / 1000 < SECONDS) {
    const tick = Date.now();
    debt += (rateAt((tick - start) / 1000) * TICK_MS) / 1000;
    const n = Math.floor(debt);
    debt -= n;
    if (n > 0) await act(n).catch((e) => console.error(e.message));
    const left = TICK_MS - (Date.now() - tick);
    if (left > 0) await new Promise((r) => setTimeout(r, left));
  }
}

async function main() {
  await setup();
  const start = Date.now();
  const many = (n, f) => Promise.all(Array.from({ length: n }, f));
  const drain = (name) => async (n) => {
    while (n > 0) n -= Math.max(1, await consume(name, n));
  };
  const workers = [];
  for (const q of QUEUES) {
    workers.push(pace(q.pub, (n) => many(n, () => publish(q)), start));
    workers.push(pace(q.con, drain(q.name), start));
  }
  workers.push(pace(() => 15, (n) => many(n, () => call('POST', `/topics/${DISTRIBUTOR.name}/messages`, { event: 'signup', n: ++seq })), start));
  for (const name of FANOUT) workers.push(pace(() => 14, drain(name), start));

  console.log(`carga em ${URL_BASE} por ${SECONDS}s`);
  const report = setInterval(() => {
    const t = (Date.now() - start) / 1000;
    console.log(`${t.toFixed(0)}s publicadas=${counts.published} consumidas=${counts.consumed} falhas=${JSON.stringify(counts.failed)}`);
  }, 10000);
  await Promise.all(workers);
  clearInterval(report);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
