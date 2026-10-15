#!/usr/bin/env node
// Generates by hand the root README's hero (it does not go through Archify): doc/diagrams/img/<lang>/hero.svg (static
// frame, with the messages already replicated) and doc/diagrams/img/<lang>/hero.gif (animation of the messages).
// This is the source: to change text, colors or timings, edit the constants below and run
//   node doc/diagrams/scripts/generate-hero.mjs [--svg-only]
// The GIF is assembled with headless Chrome (one frame per capture) and ffmpeg. Requires Node >= 22.
// Chrome: the ARCHIFY_CHROME variable, or the usual Chrome/Edge paths. ffmpeg: on the PATH, or FFMPEG.
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');

// ---------- language ----------
// English (en-US) is the source language. --lang=<code> uses doc/diagrams/i18n/<code>.json (flat table
// "English string": "translation"; a missing string stays in English) and writes to img/<code>/.
// Without --lang, every language is generated (one run of this script per language).
const langArg = process.argv.find((a) => a.startsWith('--lang='));
if (!langArg) {
  const langs = ['en-US', ...readdirSync(resolve(root, 'doc/diagrams/i18n')).filter((f) => f.endsWith('.json')).map((f) => f.slice(0, -5))];
  let status = 0;
  for (const l of langs) {
    const r = spawnSync(process.execPath, [fileURLToPath(import.meta.url), `--lang=${l}`, ...process.argv.slice(2)], { stdio: 'inherit' });
    status = status || r.status || 0;
  }
  process.exit(status);
}
const LANG = langArg.slice(7);
const SUFFIX = '';
const imgDir = resolve(root, 'doc/diagrams/img', LANG);
mkdirSync(imgDir, { recursive: true });
const TABLE = LANG === 'en-US' ? {} : JSON.parse(readFileSync(resolve(root, 'doc/diagrams/i18n', LANG + '.json'), 'utf8'));
const translate = (v) => (Array.isArray(v) ? v.map(translate) : typeof v === 'string' ? TABLE[v] ?? v : v);
const SOURCE_TEXT = {
  label: 'OPEN SOURCE MESSAGING · AGPLv3',
  promise: ['Replace SQS, SNS, Kafka', 'and RabbitMQ with '],
  promiseHighlight: 'a single image.',
  support: ['Speed and availability at almost', 'zero cost. No cloud required.'],
  chips: ['1 Docker image', 'HTTP API', 'no coordinator'],
  center: ['same image', 'on every node'],
  producer: 'producer', consumer: 'consumer', primary: 'primary', replica: 'replica',
  caption: ['producers publish without waiting', 'each node is the primary of some messages', 'consumers read whenever they want', 'the ack removes the message from all 3 nodes', 'producers and consumers come and go'],
  title: 'Taba: open-source messaging that replaces SQS, SNS, Kafka and RabbitMQ with a single Docker image',
  desc: 'A 3-node cluster, every node running the same image, drawn as 3-story buildings. Several producers come and go and publish messages; each message has a primary node, which replicates it to the other two, and every node is the primary of some of them. Consumers arrive later, read asynchronously and confirm with an ack, and the message leaves all 3 nodes.',
};
const TXT = Object.fromEntries(Object.entries(SOURCE_TEXT).map(([k, v]) => [k, translate(v)]));

// ---------- data ----------
const W = 1200, H = 540;
const SANS = "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif";
const MONO = "'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Consolas, 'DejaVu Sans Mono', monospace";
// dark theme, same color family as the Archify diagrams
const BG = '#020617', PANEL = '#0b1220', INK = '#f8fafc', MUTED = '#94a3b8', LINE = '#334155';
const HIGHLIGHT = '#22d3ee';

const COPY = {
  label: TXT.label,
  title: 'Taba',
  promise: TXT.promise,
  promiseHighlight: TXT.promiseHighlight,
  support: TXT.support,
  chips: TXT.chips,
  center: TXT.center,
};

// the village's nodes: 3 nodes, the minimum for production (same color family as the hash diagrams)
// each node is a 3-story building: one floor (slot) per stored message, and each node is the primary of some
const CX = 930, CY = 300, R = 140;
const NODES = {
  A: { x: CX, y: CY - R, color: '#22d3ee', tone: '#0b2a35' },
  B: { x: CX + Math.round(R * Math.cos(Math.PI / 6)), y: CY + R / 2, color: '#a78bfa', tone: '#241a4a' },
  C: { x: CX - Math.round(R * Math.cos(Math.PI / 6)), y: CY + R / 2, color: '#fbbf24', tone: '#33260a' },
};
const BUILDING = { width: 108, floor: 28, gap: 4, roof: 30 };

// producers (on top) and consumers (below) come and go: time windows (s) in which they appear
const PRODUCERS = [
  { x: 715, y: 22, w: 130, h: 44, text: TXT.producer, windows: [[0.2, 2.8], [4.6, 8.8]] },
  { x: 865, y: 22, w: 130, h: 44, text: TXT.producer, windows: [[3.0, 6.2], [8.0, 10.2]] },
  { x: 1015, y: 22, w: 130, h: 44, text: TXT.producer, windows: [[1.6, 3.6]] },
];
const CONSUMERS = [
  { x: 715, y: 458, w: 130, h: 40, text: TXT.consumer, windows: [[3.6, 6.6], [9.0, 11.1]] },
  { x: 865, y: 458, w: 130, h: 40, text: TXT.consumer, windows: [[4.6, 8.4], [10.0, 12.2]] },
  { x: 1015, y: 458, w: 130, h: 40, text: TXT.consumer, windows: [[7.0, 9.6]] },
];

// the example messages. `slot` is the floor it occupies in the 3 buildings; `send` takes it from the producer to the
// primary, `replicate` from the primary to the other two, `deliver` from the primary to the consumer (asynchronous,
// well after the send) and `ack` removes the message from the 3 nodes. `via*` are the curves' control points.
const MSGS = [
  { color: '#34d399', primary: 'A', slot: 0, prod: 0, cons: 0, send: [0.6, 1.2], replicate: [1.2, 1.8], deliver: [5.0, 5.8], ack: [5.8, 6.3], viaDeliver: [930, 470] },
  { color: '#60a5fa', primary: 'B', slot: 1, prod: 2, cons: 1, send: [2.0, 2.6], replicate: [2.6, 3.2], deliver: [6.6, 7.4], ack: [7.4, 7.9], viaDeliver: [1051, 480] },
  { color: '#f472b6', primary: 'C', slot: 2, prod: 1, cons: 2, send: [3.4, 4.0], replicate: [4.0, 4.6], deliver: [7.8, 8.6], ack: [8.6, 9.1], viaSend: [700, 100], viaDeliver: [930, 520] },
  { color: '#fbbf24', primary: 'B', slot: 0, prod: 0, cons: 0, send: [6.6, 7.2], replicate: [7.2, 7.8], deliver: [9.4, 10.2], ack: [10.2, 10.7], viaSend: [1100, 70] },
  { color: '#a78bfa', primary: 'C', slot: 1, prod: 1, cons: 1, send: [8.4, 9.0], replicate: [9.0, 9.6], deliver: [10.4, 11.2], ack: [11.2, 11.7], viaSend: [700, 100], viaDeliver: [809, 480] },
];
// the animation's timeline (s); the static frame is the instant STATIC_AT
const DURATION = 12.5, FPS = 20, STATIC_AT = 4.9;
// caption that goes with each phase: [start, end, text]
const CAPTIONS = [[0.3, 2.0], [2.0, 4.8], [4.8, 7.0], [7.0, 9.6], [9.6, 12.2]].map((f, i) => [...f, TXT.caption[i]]);

// ---------- utilities ----------
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const n = (v) => Math.round(v * 100) / 100;
const clamp = (v) => Math.min(1, Math.max(0, v));
const ease = (p) => (p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2);
const progress = (t, [a, b]) => ease(clamp((t - a) / (b - a)));
const mix = (a, b, p) => a + (b - a) * p;
// opacity of a [a,b] window with short fade-in and fade-out
const fade = (t, a, b, f = 0.15) => clamp(Math.min((t - a) / f, (b - t) / f));
const visible = (t, windows) => Math.max(...windows.map(([a, b]) => fade(t, a, b, 0.3)));

function text(x, y, s, { size = 20, weight = 400, color = INK, anchor = 'start', font = MONO, opacity = 1 } = {}) {
  const o = opacity < 1 ? ` opacity="${n(opacity)}"` : '';
  return `<text x="${n(x)}" y="${n(y)}" font-family="${font}" font-size="${size}" font-weight="${weight}" fill="${color}" text-anchor="${anchor}"${o}>${esc(s)}</text>`;
}

// point along a segment or a quadratic curve (with control point `via`, if any)
const pathAt = (p, start, end, via) => (via
  ? [(1 - p) * (1 - p) * start[0] + 2 * (1 - p) * p * via[0] + p * p * end[0],
     (1 - p) * (1 - p) * start[1] + 2 * (1 - p) * p * via[1] + p * p * end[1]]
  : [mix(start[0], end[0], p), mix(start[1], end[1], p)]);

// message dot (with a light glow) at a position
function dot([x, y], { color = '#34d399', r = 9, opacity = 1 } = {}) {
  if (opacity <= 0.01) return '';
  return `<circle cx="${n(x)}" cy="${n(y)}" r="${r + 5}" fill="${color}" opacity="${n(0.18 * opacity)}"/>` +
    `<circle cx="${n(x)}" cy="${n(y)}" r="${r}" fill="${color}" opacity="${n(opacity)}"/>`;
}

// hut (tent): triangle, the project's motif
function hut(x, y, width, color, weight = 3) {
  const h = width * 0.95;
  return `<path d="M${n(x)} ${n(y - h / 2)} L${n(x + width / 2)} ${n(y + h / 2)} L${n(x - width / 2)} ${n(y + h / 2)} Z" fill="none" stroke="${color}" stroke-width="${weight}" stroke-linejoin="round"/>`;
}

// geometry of a node's building: base, top of the floors, top of the roof and center of each floor (0 = ground floor)
function building(node) {
  const height = 3 * BUILDING.floor + 2 * BUILDING.gap;
  const base = node.y + (height + BUILDING.roof) / 2;
  const floorsTop = base - height;
  const floorCenter = (i) => base - (i + 1) * BUILDING.floor - i * BUILDING.gap + BUILDING.floor / 2;
  return { base, floorsTop, roofTop: floorsTop - BUILDING.roof, floorCenter };
}

// ---------- scene ----------
function leftSide() {
  const p = [];
  p.push(text(60, 84, COPY.label, { size: 20, weight: 700, color: HIGHLIGHT }));
  p.push(hut(96, 152, 66, HIGHLIGHT, 5));
  p.push(text(150, 190, COPY.title, { size: 112, weight: 800, font: SANS }));
  const [l1, l2] = COPY.promise;
  p.push(text(60, 262, l1, { size: 32, weight: 700, font: SANS }));
  p.push(`<text x="60" y="302" font-family="${SANS}" font-size="32" font-weight="700" fill="${INK}">${esc(l2)}<tspan fill="${HIGHLIGHT}">${esc(COPY.promiseHighlight)}</tspan></text>`);
  COPY.support.forEach((l, i) => p.push(text(60, 358 + i * 30, l, { size: 22, color: MUTED, font: SANS })));
  let x = 60;
  for (const c of COPY.chips) {
    const w = c.length * 12 + 32;
    p.push(`<rect x="${x}" y="430" width="${w}" height="42" rx="21" fill="${PANEL}" stroke="${LINE}" stroke-width="1.5"/>`);
    p.push(text(x + w / 2, 457, c, { size: 20, anchor: 'middle' }));
    x += w + 16;
  }
  return p.join('');
}

// producer or consumer: fades in and out with `opacity`; `mark` (color) highlights the outline while it holds a message
function box(c, opacity, mark) {
  if (opacity <= 0.01) return '';
  const radius = c.h / 2;
  return `<g opacity="${n(opacity)}"><rect x="${c.x}" y="${c.y}" width="${c.w}" height="${c.h}" rx="${radius}" fill="${PANEL}" stroke="${LINE}" stroke-width="1.5"/>` +
    (mark ? `<rect x="${c.x}" y="${c.y}" width="${c.w}" height="${c.h}" rx="${radius}" fill="${mark.color}" fill-opacity="${n(0.16 * mark.opacity)}" stroke="${mark.color}" stroke-width="3" opacity="${n(mark.opacity)}"/>` : '') +
    text(c.x + c.w / 2, c.y + c.h / 2 + 6, c.text, { size: 18, anchor: 'middle', color: MUTED }) + '</g>';
}

// t = the animation's instant in seconds
function rightSide(t) {
  const p = [];
  const ids = Object.keys(NODES);
  p.push(`<circle cx="${CX}" cy="${CY}" r="${R}" fill="none" stroke="${LINE}" stroke-width="2" stroke-dasharray="4 10" stroke-linecap="round"/>`);
  COPY.center.forEach((l, i) => p.push(text(CX, CY - 38 + i * 24, l, { size: 18, color: MUTED, anchor: 'middle' })));

  // producers and consumers; the consumer highlights the outline with the color of the message it just received
  PRODUCERS.forEach((c) => p.push(box(c, visible(t, c.windows))));
  CONSUMERS.forEach((c, k) => {
    const m = MSGS.find((x) => x.cons === k && t >= x.deliver[1] && t < x.ack[1] + 0.15);
    p.push(box(c, visible(t, c.windows), m && { color: m.color, opacity: fade(t, m.deliver[1], m.ack[1] + 0.15, 0.15) }));
  });

  // buildings: the 3 empty floors, then each message lights up its floor when the node receives it and
  // goes dark on the ack
  const g = {};
  for (const id of ids) g[id] = building(NODES[id]);
  for (const id of ids) {
    const node = NODES[id], b = g[id], x0 = node.x - BUILDING.width / 2;
    const floorY = (s) => b.floorsTop + (2 - s) * (BUILDING.floor + BUILDING.gap);
    for (let s = 0; s < 3; s++) {
      p.push(`<rect x="${x0}" y="${n(floorY(s))}" width="${BUILDING.width}" height="${BUILDING.floor}" rx="5" fill="${node.tone}" stroke="${node.color}" stroke-width="1.5" opacity="0.55"/>`);
    }
    for (const m of MSGS) {
      const isPrimary = m.primary === id;
      const arrival = isPrimary ? m.send[1] : m.replicate[1];
      const opacity = clamp((t - arrival) / 0.2) * (1 - progress(t, m.ack));
      if (opacity <= 0.01) continue;
      const y0 = floorY(m.slot);
      p.push(`<rect x="${x0}" y="${n(y0)}" width="${BUILDING.width}" height="${BUILDING.floor}" rx="5" fill="${m.color}" fill-opacity="${isPrimary ? 0.35 : 0.16}" stroke="${m.color}" stroke-width="${isPrimary ? 3 : 1.5}" opacity="${n(opacity)}"/>`);
      p.push(text(node.x, y0 + 20, isPrimary ? TXT.primary : TXT.replica, { size: 18, weight: isPrimary ? 700 : 400, color: isPrimary ? INK : MUTED, anchor: 'middle', opacity }));
    }
    // roof (hut) with the node's letter
    p.push(`<path d="M${node.x} ${n(b.roofTop)} L${node.x + BUILDING.width / 2 - 6} ${n(b.floorsTop - 3)} L${node.x - BUILDING.width / 2 + 6} ${n(b.floorsTop - 3)} Z" fill="${node.tone}" stroke="${node.color}" stroke-width="3" stroke-linejoin="round"/>`);
    p.push(text(node.x, b.floorsTop - 9, id, { size: 18, weight: 700, anchor: 'middle' }));
  }

  // messages in transit: producer → primary, primary → replicas (on the message's floor) and
  // primary → consumer
  for (const m of MSGS) {
    const prod = PRODUCERS[m.prod], cons = CONSUMERS[m.cons];
    const prim = NODES[m.primary], gp = g[m.primary];
    if (t >= m.send[0] && t < m.send[1] + 0.02) {
      const start = [prod.x + prod.w / 2, prod.y + prod.h], end = [prim.x, gp.roofTop + 2];
      p.push(dot(pathAt(progress(t, m.send), start, end, m.viaSend), { color: m.color }));
    }
    if (t >= m.replicate[0] && t < m.replicate[1]) {
      const from = [prim.x, gp.floorCenter(m.slot)];
      for (const id of ids.filter((k) => k !== m.primary)) {
        p.push(dot(pathAt(progress(t, m.replicate), from, [NODES[id].x, g[id].floorCenter(m.slot)]), { color: m.color, r: 7 }));
      }
    }
    if (t >= m.deliver[0] && t < m.deliver[1]) {
      const start = [prim.x, gp.floorCenter(m.slot)], end = [cons.x + cons.w / 2, cons.y];
      p.push(dot(pathAt(progress(t, m.deliver), start, end, m.viaDeliver), { color: m.color }));
    }
  }

  // caption of the current phase
  for (const [a, b, s] of CAPTIONS) {
    const opacity = fade(t, a, b, 0.2);
    if (opacity > 0.01) p.push(text(CX, 522, s, { size: 20, color: MUTED, anchor: 'middle', opacity }));
  }
  return p.join('');
}

function svg(t, isStatic = false) {
  return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" lang="${LANG}" aria-labelledby="t d">
<title id="t">${esc(TXT.title)}</title>
<desc id="d">${esc(TXT.desc)}</desc>
<rect width="${W}" height="${H}" fill="${BG}"/>${leftSide()}${rightSide(isStatic ? STATIC_AT : t)}
</svg>
`;
}

// ---------- output ----------
writeFileSync(resolve(imgDir, `hero${SUFFIX}.svg`), svg(0, true));
console.log(`SVG: doc/diagrams/img/${LANG}/hero${SUFFIX}.svg`);

if (process.argv.includes('--svg-only')) process.exit(0);

// GIF: one Chrome capture per frame, then ffmpeg with its own palette
const candidates = [
  process.env.ARCHIFY_CHROME,
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium',
  '/usr/bin/chromium-browser',
].filter(Boolean);
const chrome = candidates.find((c) => existsSync(c));
if (!chrome) { console.error('Chrome/Edge not found. Set ARCHIFY_CHROME.'); process.exit(2); }
const ffmpeg = process.env.FFMPEG || 'ffmpeg';
if (spawnSync(ffmpeg, ['-version'], { stdio: 'ignore' }).status !== 0) {
  console.error('ffmpeg not found. Install it or set FFMPEG.'); process.exit(2);
}

const tmp = mkdtempSync(join(tmpdir(), 'taba-hero-'));
const profile = join(tmp, 'profile');
const proc = spawn(chrome, [
  '--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check', '--hide-scrollbars',
  `--user-data-dir=${profile}`, '--remote-debugging-port=0', `--window-size=${W},${H}`, 'about:blank',
], { stdio: ['ignore', 'ignore', 'pipe'] });
const shutdown = () => {
  try { proc.kill(); } catch {}
  setTimeout(() => { try { rmSync(tmp, { recursive: true, force: true }); } catch {} }, 800);
};

try {
  const port = await new Promise((ok, fail) => {
    let buf = '';
    const t = setTimeout(() => fail(new Error('Chrome did not start in time')), 30000);
    proc.stderr.on('data', (d) => {
      buf += d;
      const m = buf.match(/DevTools listening on ws:\/\/127\.0\.0\.1:(\d+)\//);
      if (m) { clearTimeout(t); ok(m[1]); }
    });
    proc.on('exit', () => fail(new Error('Chrome exited earlier than expected')));
  });
  const target = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: 'PUT' })).json();
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((ok, fail) => { ws.onopen = ok; ws.onerror = fail; });
  let seq = 0;
  const pending = new Map();
  ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
  };
  const cdp = (method, params = {}) => new Promise((ok) => {
    const id = ++seq; pending.set(id, ok); ws.send(JSON.stringify({ id, method, params }));
  });

  await cdp('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false });
  const frames = DURATION * FPS;
  for (let i = 0; i < frames; i++) {
    const html = `<!doctype html><meta charset="utf-8"><body style="margin:0;background:${BG}">${svg(i / FPS).replace(/^<\?xml[^>]*\?>\s*/, '')}`;
    let r;
    for (let attempt = 0; attempt < 4; attempt++) {
      await cdp('Page.navigate', { url: 'data:text/html;charset=utf-8,' + encodeURIComponent(html) });
      await new Promise((ok) => setTimeout(ok, i === 0 || attempt ? 400 : 60));
      r = await cdp('Page.captureScreenshot', { format: 'png', clip: { x: 0, y: 0, width: W, height: H, scale: 1 } });
      if (r.result) break;
    }
    if (!r.result) throw new Error('capture failed at frame ' + i + ': ' + JSON.stringify(r).slice(0, 300));
    writeFileSync(join(tmp, `f${String(i).padStart(3, '0')}.png`), Buffer.from(r.result.data, 'base64'));
  }
  ws.close();

  const outFile = resolve(imgDir, `hero${SUFFIX}.gif`);
  const r = spawnSync(ffmpeg, [
    '-y', '-loglevel', 'error', '-framerate', String(FPS), '-i', join(tmp, 'f%03d.png'),
    '-vf', 'split[a][b];[a]palettegen=max_colors=64:stats_mode=full[p];[b][p]paletteuse=dither=none',
    '-loop', '0', outFile,
  ], { stdio: 'inherit' });
  if (r.status !== 0) { console.error('ffmpeg failed'); process.exitCode = 1; }
  else console.log(`GIF: doc/diagrams/img/${LANG}/hero${SUFFIX}.gif (${frames} frames, ${DURATION}s, ${FPS} fps)`);
} finally {
  shutdown();
}
