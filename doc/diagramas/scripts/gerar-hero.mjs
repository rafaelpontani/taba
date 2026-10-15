#!/usr/bin/env node
// Generates by hand the root README's hero (it does not go through Archify): doc/diagramas/img/hero.svg (static
// frame, with the messages already replicated) and doc/diagramas/img/hero.gif (animation of the messages).
// This is the source: to change text, colors or timings, edit the constants below and run
//   node doc/diagramas/scripts/gerar-hero.mjs [--so-svg]
// The GIF is assembled with headless Chrome (one frame per capture) and ffmpeg. Requires Node >= 22.
// Chrome: the ARCHIFY_CHROME variable, or the usual Chrome/Edge paths. ffmpeg: on the PATH, or FFMPEG.
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const raiz = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');

// ---------- language ----------
// English is the source language. --lang=<code> uses doc/diagramas/i18n/<code>.json (flat table
// "English string": "translation"; a missing string stays in English) and writes to img/<code>/.
// Without --lang, every language is generated (one run of this script per language).
const langArg = process.argv.find((a) => a.startsWith('--lang='));
if (!langArg) {
  const langs = ['en', ...readdirSync(resolve(raiz, 'doc/diagramas/i18n')).filter((f) => f.endsWith('.json')).map((f) => f.slice(0, -5))];
  let status = 0;
  for (const l of langs) {
    const r = spawnSync(process.execPath, [fileURLToPath(import.meta.url), `--lang=${l}`, ...process.argv.slice(2)], { stdio: 'inherit' });
    status = status || r.status || 0;
  }
  process.exit(status);
}
const LANG = langArg.slice(7);
const SUF = '';
const dirImg = resolve(raiz, 'doc/diagramas/img', LANG);
mkdirSync(dirImg, { recursive: true });
const TABELA = LANG === 'en' ? {} : JSON.parse(readFileSync(resolve(raiz, 'doc/diagramas/i18n', LANG + '.json'), 'utf8'));
const traduzir = (v) => (Array.isArray(v) ? v.map(traduzir) : typeof v === 'string' ? TABELA[v] ?? v : v);
const TXT_EN = {
  en: {
    etiqueta: 'OPEN SOURCE MESSAGING · AGPLv3',
    promessa: ['Replace SQS, SNS, Kafka', 'and RabbitMQ with '],
    promessaDestaque: 'a single image.',
    apoio: ['Speed and availability at almost', 'zero cost. No cloud required.'],
    chips: ['1 Docker image', 'HTTP API', 'no coordinator'],
    centro: ['same image', 'on every node'],
    produtor: 'producer', consumidor: 'consumer', primario: 'primary', replica: 'replica',
    legenda: ['producers publish without waiting', 'each node is the primary of some messages', 'consumers read whenever they want', 'the ack removes the message from all 3 nodes', 'producers and consumers come and go'],
    titulo: 'Taba: open-source messaging that replaces SQS, SNS, Kafka and RabbitMQ with a single Docker image',
    desc: 'A 3-node cluster, every node running the same image, drawn as 3-story buildings. Several producers come and go and publish messages; each message has a primary node, which replicates it to the other two, and every node is the primary of some of them. Consumers arrive later, read asynchronously and confirm with an ack, and the message leaves all 3 nodes.',
    html: 'en',
  },
};
const TXT = Object.fromEntries(Object.entries(TXT_EN.en).map(([k, v]) => [k, k === 'html' ? LANG : traduzir(v)]));

// ---------- data ----------
const W = 1200, H = 540;
const SANS = "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif";
const MONO = "'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Consolas, 'DejaVu Sans Mono', monospace";
// dark theme, same color family as the Archify diagrams
const BG = '#020617', PAINEL = '#0b1220', TINTA = '#f8fafc', SUAVE = '#94a3b8', LINHA = '#334155';
const DESTAQUE = '#22d3ee';

const TEXTO = {
  etiqueta: TXT.etiqueta,
  titulo: 'Taba',
  promessa: TXT.promessa,
  promessaDestaque: TXT.promessaDestaque,
  apoio: TXT.apoio,
  chips: TXT.chips,
  centro: TXT.centro,
};

// the village's nodes: 3 nodes, the minimum for production (same color family as the hash diagrams)
// each node is a 3-story building: one floor (slot) per stored message, and each node is the primary of some
const CX = 930, CY = 300, R = 140;
const NOS = {
  A: { x: CX, y: CY - R, cor: '#22d3ee', tom: '#0b2a35' },
  B: { x: CX + Math.round(R * Math.cos(Math.PI / 6)), y: CY + R / 2, cor: '#a78bfa', tom: '#241a4a' },
  C: { x: CX - Math.round(R * Math.cos(Math.PI / 6)), y: CY + R / 2, cor: '#fbbf24', tom: '#33260a' },
};
const PRED = { larg: 108, andar: 28, folga: 4, telhado: 30 };

// producers (on top) and consumers (below) come and go: time windows (s) in which they appear
const PRODUTORES = [
  { x: 715, y: 22, w: 130, h: 44, texto: TXT.produtor, janelas: [[0.2, 2.8], [4.6, 8.8]] },
  { x: 865, y: 22, w: 130, h: 44, texto: TXT.produtor, janelas: [[3.0, 6.2], [8.0, 10.2]] },
  { x: 1015, y: 22, w: 130, h: 44, texto: TXT.produtor, janelas: [[1.6, 3.6]] },
];
const CONSUMIDORES = [
  { x: 715, y: 458, w: 130, h: 40, texto: TXT.consumidor, janelas: [[3.6, 6.6], [9.0, 11.1]] },
  { x: 865, y: 458, w: 130, h: 40, texto: TXT.consumidor, janelas: [[4.6, 8.4], [10.0, 12.2]] },
  { x: 1015, y: 458, w: 130, h: 40, texto: TXT.consumidor, janelas: [[7.0, 9.6]] },
];

// the example messages. `slot` is the floor it occupies in the 3 buildings; `envia` takes it from the producer to the
// primary, `replica` from the primary to the other two, `entrega` from the primary to the consumer (asynchronous,
// well after the send) and `ack` removes the message from the 3 nodes. `via*` are the curves' control points.
const MSGS = [
  { cor: '#34d399', primario: 'A', slot: 0, prod: 0, cons: 0, envia: [0.6, 1.2], replica: [1.2, 1.8], entrega: [5.0, 5.8], ack: [5.8, 6.3], viaEntrega: [930, 470] },
  { cor: '#60a5fa', primario: 'B', slot: 1, prod: 2, cons: 1, envia: [2.0, 2.6], replica: [2.6, 3.2], entrega: [6.6, 7.4], ack: [7.4, 7.9], viaEntrega: [1051, 480] },
  { cor: '#f472b6', primario: 'C', slot: 2, prod: 1, cons: 2, envia: [3.4, 4.0], replica: [4.0, 4.6], entrega: [7.8, 8.6], ack: [8.6, 9.1], viaEnvio: [700, 100], viaEntrega: [930, 520] },
  { cor: '#fbbf24', primario: 'B', slot: 0, prod: 0, cons: 0, envia: [6.6, 7.2], replica: [7.2, 7.8], entrega: [9.4, 10.2], ack: [10.2, 10.7], viaEnvio: [1100, 70] },
  { cor: '#a78bfa', primario: 'C', slot: 1, prod: 1, cons: 1, envia: [8.4, 9.0], replica: [9.0, 9.6], entrega: [10.4, 11.2], ack: [11.2, 11.7], viaEnvio: [700, 100], viaEntrega: [809, 480] },
];
// the animation's timeline (s); the static frame is the instant ESTATICO
const DURACAO = 12.5, FPS = 20, ESTATICO = 4.9;
// caption that goes with each phase: [start, end, text]
const LEGENDA = [[0.3, 2.0], [2.0, 4.8], [4.8, 7.0], [7.0, 9.6], [9.6, 12.2]].map((f, i) => [...f, TXT.legenda[i]]);

// ---------- utilities ----------
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const n = (v) => Math.round(v * 100) / 100;
const clamp = (v) => Math.min(1, Math.max(0, v));
const suave = (p) => (p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2);
const prog = (t, [a, b]) => suave(clamp((t - a) / (b - a)));
const mistura = (a, b, p) => a + (b - a) * p;
// opacity of a [a,b] window with short fade-in and fade-out
const janela = (t, a, b, f = 0.15) => clamp(Math.min((t - a) / f, (b - t) / f));
const visivel = (t, janelas) => Math.max(...janelas.map(([a, b]) => janela(t, a, b, 0.3)));

function texto(x, y, s, { tam = 20, peso = 400, cor = TINTA, ancora = 'start', fonte = MONO, op = 1 } = {}) {
  const o = op < 1 ? ` opacity="${n(op)}"` : '';
  return `<text x="${n(x)}" y="${n(y)}" font-family="${fonte}" font-size="${tam}" font-weight="${peso}" fill="${cor}" text-anchor="${ancora}"${o}>${esc(s)}</text>`;
}

// point along a segment or a quadratic curve (with control point `via`, if any)
const caminho = (p, ini, fim, via) => (via
  ? [(1 - p) * (1 - p) * ini[0] + 2 * (1 - p) * p * via[0] + p * p * fim[0],
     (1 - p) * (1 - p) * ini[1] + 2 * (1 - p) * p * via[1] + p * p * fim[1]]
  : [mistura(ini[0], fim[0], p), mistura(ini[1], fim[1], p)]);

// message dot (with a light glow) at a position
function bola([x, y], { cor = '#34d399', r = 9, op = 1 } = {}) {
  if (op <= 0.01) return '';
  return `<circle cx="${n(x)}" cy="${n(y)}" r="${r + 5}" fill="${cor}" opacity="${n(0.18 * op)}"/>` +
    `<circle cx="${n(x)}" cy="${n(y)}" r="${r}" fill="${cor}" opacity="${n(op)}"/>`;
}

// hut (tent): triangle, the project's motif
function oca(x, y, larg, cor, peso = 3) {
  const h = larg * 0.95;
  return `<path d="M${n(x)} ${n(y - h / 2)} L${n(x + larg / 2)} ${n(y + h / 2)} L${n(x - larg / 2)} ${n(y + h / 2)} Z" fill="none" stroke="${cor}" stroke-width="${peso}" stroke-linejoin="round"/>`;
}

// geometry of a node's building: base, top of the floors, top of the roof and center of each floor (0 = ground floor)
function predio(no) {
  const alt = 3 * PRED.andar + 2 * PRED.folga;
  const base = no.y + (alt + PRED.telhado) / 2;
  const topoAndares = base - alt;
  const centroAndar = (i) => base - (i + 1) * PRED.andar - i * PRED.folga + PRED.andar / 2;
  return { base, topoAndares, topoTelhado: topoAndares - PRED.telhado, centroAndar };
}

// ---------- cena ----------
function esquerda() {
  const p = [];
  p.push(texto(60, 84, TEXTO.etiqueta, { tam: 20, peso: 700, cor: DESTAQUE }));
  p.push(oca(96, 152, 66, DESTAQUE, 5));
  p.push(texto(150, 190, TEXTO.titulo, { tam: 112, peso: 800, fonte: SANS }));
  const [l1, l2] = TEXTO.promessa;
  p.push(texto(60, 262, l1, { tam: 32, peso: 700, fonte: SANS }));
  p.push(`<text x="60" y="302" font-family="${SANS}" font-size="32" font-weight="700" fill="${TINTA}">${esc(l2)}<tspan fill="${DESTAQUE}">${esc(TEXTO.promessaDestaque)}</tspan></text>`);
  TEXTO.apoio.forEach((l, i) => p.push(texto(60, 358 + i * 30, l, { tam: 22, cor: SUAVE, fonte: SANS })));
  let x = 60;
  for (const c of TEXTO.chips) {
    const w = c.length * 12 + 32;
    p.push(`<rect x="${x}" y="430" width="${w}" height="42" rx="21" fill="${PAINEL}" stroke="${LINHA}" stroke-width="1.5"/>`);
    p.push(texto(x + w / 2, 457, c, { tam: 20, ancora: 'middle' }));
    x += w + 16;
  }
  return p.join('');
}

// producer or consumer: fades in and out with `op`; `marca` (color) highlights the outline while it holds a message
function caixa(c, op, marca) {
  if (op <= 0.01) return '';
  const raio = c.h / 2;
  return `<g opacity="${n(op)}"><rect x="${c.x}" y="${c.y}" width="${c.w}" height="${c.h}" rx="${raio}" fill="${PAINEL}" stroke="${LINHA}" stroke-width="1.5"/>` +
    (marca ? `<rect x="${c.x}" y="${c.y}" width="${c.w}" height="${c.h}" rx="${raio}" fill="${marca.cor}" fill-opacity="${n(0.16 * marca.op)}" stroke="${marca.cor}" stroke-width="3" opacity="${n(marca.op)}"/>` : '') +
    texto(c.x + c.w / 2, c.y + c.h / 2 + 6, c.texto, { tam: 18, ancora: 'middle', cor: SUAVE }) + '</g>';
}

// t = the animation's instant in seconds
function direita(t) {
  const p = [];
  const ids = Object.keys(NOS);
  p.push(`<circle cx="${CX}" cy="${CY}" r="${R}" fill="none" stroke="${LINHA}" stroke-width="2" stroke-dasharray="4 10" stroke-linecap="round"/>`);
  TEXTO.centro.forEach((l, i) => p.push(texto(CX, CY - 38 + i * 24, l, { tam: 18, cor: SUAVE, ancora: 'middle' })));

  // producers and consumers; the consumer highlights the outline with the color of the message it just received
  PRODUTORES.forEach((c) => p.push(caixa(c, visivel(t, c.janelas))));
  CONSUMIDORES.forEach((c, k) => {
    const m = MSGS.find((x) => x.cons === k && t >= x.entrega[1] && t < x.ack[1] + 0.15);
    p.push(caixa(c, visivel(t, c.janelas), m && { cor: m.cor, op: janela(t, m.entrega[1], m.ack[1] + 0.15, 0.15) }));
  });

  // buildings: the 3 empty floors, then each message lights up its floor when the node receives it and
  // goes dark on the ack
  const g = {};
  for (const id of ids) g[id] = predio(NOS[id]);
  for (const id of ids) {
    const no = NOS[id], b = g[id], x0 = no.x - PRED.larg / 2;
    const yAndar = (s) => b.topoAndares + (2 - s) * (PRED.andar + PRED.folga);
    for (let s = 0; s < 3; s++) {
      p.push(`<rect x="${x0}" y="${n(yAndar(s))}" width="${PRED.larg}" height="${PRED.andar}" rx="5" fill="${no.tom}" stroke="${no.cor}" stroke-width="1.5" opacity="0.55"/>`);
    }
    for (const m of MSGS) {
      const primario = m.primario === id;
      const chegada = primario ? m.envia[1] : m.replica[1];
      const op = clamp((t - chegada) / 0.2) * (1 - prog(t, m.ack));
      if (op <= 0.01) continue;
      const y0 = yAndar(m.slot);
      p.push(`<rect x="${x0}" y="${n(y0)}" width="${PRED.larg}" height="${PRED.andar}" rx="5" fill="${m.cor}" fill-opacity="${primario ? 0.35 : 0.16}" stroke="${m.cor}" stroke-width="${primario ? 3 : 1.5}" opacity="${n(op)}"/>`);
      p.push(texto(no.x, y0 + 20, primario ? TXT.primario : TXT.replica, { tam: 18, peso: primario ? 700 : 400, cor: primario ? TINTA : SUAVE, ancora: 'middle', op }));
    }
    // roof (hut) with the node's letter
    p.push(`<path d="M${no.x} ${n(b.topoTelhado)} L${no.x + PRED.larg / 2 - 6} ${n(b.topoAndares - 3)} L${no.x - PRED.larg / 2 + 6} ${n(b.topoAndares - 3)} Z" fill="${no.tom}" stroke="${no.cor}" stroke-width="3" stroke-linejoin="round"/>`);
    p.push(texto(no.x, b.topoAndares - 9, id, { tam: 18, peso: 700, ancora: 'middle' }));
  }

  // messages in transit: producer → primary, primary → replicas (on the message's floor) and
  // primary → consumer
  for (const m of MSGS) {
    const prod = PRODUTORES[m.prod], cons = CONSUMIDORES[m.cons];
    const prim = NOS[m.primario], gp = g[m.primario];
    if (t >= m.envia[0] && t < m.envia[1] + 0.02) {
      const ini = [prod.x + prod.w / 2, prod.y + prod.h], fim = [prim.x, gp.topoTelhado + 2];
      p.push(bola(caminho(prog(t, m.envia), ini, fim, m.viaEnvio), { cor: m.cor }));
    }
    if (t >= m.replica[0] && t < m.replica[1]) {
      const de = [prim.x, gp.centroAndar(m.slot)];
      for (const id of ids.filter((k) => k !== m.primario)) {
        p.push(bola(caminho(prog(t, m.replica), de, [NOS[id].x, g[id].centroAndar(m.slot)]), { cor: m.cor, r: 7 }));
      }
    }
    if (t >= m.entrega[0] && t < m.entrega[1]) {
      const ini = [prim.x, gp.centroAndar(m.slot)], fim = [cons.x + cons.w / 2, cons.y];
      p.push(bola(caminho(prog(t, m.entrega), ini, fim, m.viaEntrega), { cor: m.cor }));
    }
  }

  // caption of the current phase
  for (const [a, b, s] of LEGENDA) {
    const op = janela(t, a, b, 0.2);
    if (op > 0.01) p.push(texto(CX, 522, s, { tam: 20, cor: SUAVE, ancora: 'middle', op }));
  }
  return p.join('');
}

function svg(t, estatico = false) {
  const titulo = TXT.titulo;
  const desc = TXT.desc;
  return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" lang="${TXT.html}" aria-labelledby="t d">
<title id="t">${esc(titulo)}</title>
<desc id="d">${esc(desc)}</desc>
<rect width="${W}" height="${H}" fill="${BG}"/>${esquerda()}${direita(estatico ? ESTATICO : t)}
</svg>
`;
}

// ---------- output ----------
writeFileSync(resolve(dirImg, `hero${SUF}.svg`), svg(0, true));
console.log(`SVG: doc/diagramas/img/${LANG}/hero${SUF}.svg`);

if (process.argv.includes('--so-svg')) process.exit(0);

// GIF: one Chrome capture per frame, then ffmpeg with its own palette
const candidatos = [
  process.env.ARCHIFY_CHROME,
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium',
  '/usr/bin/chromium-browser',
].filter(Boolean);
const chrome = candidatos.find((c) => existsSync(c));
if (!chrome) { console.error('Chrome/Edge not found. Set ARCHIFY_CHROME.'); process.exit(2); }
const ffmpeg = process.env.FFMPEG || 'ffmpeg';
if (spawnSync(ffmpeg, ['-version'], { stdio: 'ignore' }).status !== 0) {
  console.error('ffmpeg not found. Install it or set FFMPEG.'); process.exit(2);
}

const tmp = mkdtempSync(join(tmpdir(), 'taba-hero-'));
const perfil = join(tmp, 'perfil');
const proc = spawn(chrome, [
  '--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check', '--hide-scrollbars',
  `--user-data-dir=${perfil}`, '--remote-debugging-port=0', `--window-size=${W},${H}`, 'about:blank',
], { stdio: ['ignore', 'ignore', 'pipe'] });
const encerrar = () => {
  try { proc.kill(); } catch {}
  setTimeout(() => { try { rmSync(tmp, { recursive: true, force: true }); } catch {} }, 800);
};

try {
  const porta = await new Promise((ok, erro) => {
    let buf = '';
    const t = setTimeout(() => erro(new Error('Chrome did not start in time')), 30000);
    proc.stderr.on('data', (d) => {
      buf += d;
      const m = buf.match(/DevTools listening on ws:\/\/127\.0\.0\.1:(\d+)\//);
      if (m) { clearTimeout(t); ok(m[1]); }
    });
    proc.on('exit', () => erro(new Error('Chrome exited earlier than expected')));
  });
  const alvo = await (await fetch(`http://127.0.0.1:${porta}/json/new?about:blank`, { method: 'PUT' })).json();
  const ws = new WebSocket(alvo.webSocketDebuggerUrl);
  await new Promise((ok, erro) => { ws.onopen = ok; ws.onerror = erro; });
  let seq = 0;
  const pend = new Map();
  ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.id && pend.has(m.id)) { pend.get(m.id)(m); pend.delete(m.id); }
  };
  const cdp = (method, params = {}) => new Promise((ok) => {
    const id = ++seq; pend.set(id, ok); ws.send(JSON.stringify({ id, method, params }));
  });

  await cdp('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false });
  const quadros = DURACAO * FPS;
  for (let i = 0; i < quadros; i++) {
    const html = `<!doctype html><meta charset="utf-8"><body style="margin:0;background:${BG}">${svg(i / FPS).replace(/^<\?xml[^>]*\?>\s*/, '')}`;
    let r;
    for (let tentativa = 0; tentativa < 4; tentativa++) {
      await cdp('Page.navigate', { url: 'data:text/html;charset=utf-8,' + encodeURIComponent(html) });
      await new Promise((ok) => setTimeout(ok, i === 0 || tentativa ? 400 : 60));
      r = await cdp('Page.captureScreenshot', { format: 'png', clip: { x: 0, y: 0, width: W, height: H, scale: 1 } });
      if (r.result) break;
    }
    if (!r.result) throw new Error('capture failed at frame ' + i + ': ' + JSON.stringify(r).slice(0, 300));
    writeFileSync(join(tmp, `q${String(i).padStart(3, '0')}.png`), Buffer.from(r.result.data, 'base64'));
  }
  ws.close();

  const saida = resolve(dirImg, `hero${SUF}.gif`);
  const r = spawnSync(ffmpeg, [
    '-y', '-loglevel', 'error', '-framerate', String(FPS), '-i', join(tmp, 'q%03d.png'),
    '-vf', 'split[a][b];[a]palettegen=max_colors=64:stats_mode=full[p];[b][p]paletteuse=dither=none',
    '-loop', '0', saida,
  ], { stdio: 'inherit' });
  if (r.status !== 0) { console.error('ffmpeg failed'); process.exitCode = 1; }
  else console.log(`GIF: doc/diagramas/img/${LANG}/hero${SUF}.gif (${quadros} frames, ${DURACAO}s, ${FPS} fps)`);
} finally {
  encerrar();
}
