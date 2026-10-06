#!/usr/bin/env node
// Gera à mão o hero do README da raiz (não passa pelo Archify): doc/diagramas/img/hero.svg (quadro
// estático, com as mensagens já replicadas) e doc/diagramas/img/hero.gif (animação das mensagens).
// Esta é a fonte: para mudar texto, cores ou tempos, edite as constantes abaixo e rode
//   node doc/diagramas/scripts/gerar-hero.mjs
// O GIF é montado com o Chrome headless (um quadro por captura) e o ffmpeg. Requer Node >= 22.
// Chrome: variável ARCHIFY_CHROME, ou os caminhos comuns do Chrome/Edge. ffmpeg: no PATH, ou FFMPEG.
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const raiz = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const dirImg = resolve(raiz, 'doc/diagramas/img');
mkdirSync(dirImg, { recursive: true });

// ---------- dados ----------
const W = 1200, H = 540;
const SANS = "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif";
const MONO = "'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Consolas, 'DejaVu Sans Mono', monospace";
// tema escuro, mesma família de cores dos diagramas do Archify
const BG = '#020617', PAINEL = '#0b1220', TINTA = '#f8fafc', SUAVE = '#94a3b8', LINHA = '#334155';
const DESTAQUE = '#22d3ee';

const TEXTO = {
  etiqueta: 'MENSAGERIA OPEN SOURCE · AGPLv3',
  titulo: 'Taba',
  promessa: ['Substitua SQS, SNS, Kafka', 'e RabbitMQ com '],
  promessaDestaque: 'uma só imagem.',
  apoio: ['Velocidade e disponibilidade, com custo', 'quase zero. Sem nuvem obrigatória.'],
  chips: ['1 imagem Docker', 'API HTTP', 'sem coordenador'],
  centro: ['mesma imagem', 'em cada nó'],
};

// nós da aldeia: 3 nós, o mínimo para produção (mesma família de cores dos diagramas de hash)
// cada nó é um prédio de 3 andares: um andar (slot) por mensagem guardada, e cada nó é o primário de algumas
const CX = 930, CY = 300, R = 140;
const NOS = {
  A: { x: CX, y: CY - R, cor: '#22d3ee', tom: '#0b2a35' },
  B: { x: CX + Math.round(R * Math.cos(Math.PI / 6)), y: CY + R / 2, cor: '#a78bfa', tom: '#241a4a' },
  C: { x: CX - Math.round(R * Math.cos(Math.PI / 6)), y: CY + R / 2, cor: '#fbbf24', tom: '#33260a' },
};
const PRED = { larg: 108, andar: 28, folga: 4, telhado: 30 };

// produtores (em cima) e consumidores (embaixo) entram e saem: janelas de tempo (s) em que aparecem
const PRODUTORES = [
  { x: 715, y: 22, w: 130, h: 44, texto: 'produtor', janelas: [[0.2, 2.8], [4.6, 8.8]] },
  { x: 865, y: 22, w: 130, h: 44, texto: 'produtor', janelas: [[3.0, 6.2], [8.0, 10.2]] },
  { x: 1015, y: 22, w: 130, h: 44, texto: 'produtor', janelas: [[1.6, 3.6]] },
];
const CONSUMIDORES = [
  { x: 715, y: 458, w: 130, h: 40, texto: 'consumidor', janelas: [[3.6, 6.6], [9.0, 11.1]] },
  { x: 865, y: 458, w: 130, h: 40, texto: 'consumidor', janelas: [[4.6, 8.4], [10.0, 12.2]] },
  { x: 1015, y: 458, w: 130, h: 40, texto: 'consumidor', janelas: [[7.0, 9.6]] },
];

// mensagens do exemplo. `slot` é o andar que ela ocupa nos 3 prédios; `envia` leva do produtor ao
// primário, `replica` do primário aos outros dois, `entrega` do primário ao consumidor (assíncrono,
// bem depois do envio) e `ack` remove a mensagem dos 3 nós. `via*` são pontos de controle das curvas.
const MSGS = [
  { cor: '#34d399', primario: 'A', slot: 0, prod: 0, cons: 0, envia: [0.6, 1.2], replica: [1.2, 1.8], entrega: [5.0, 5.8], ack: [5.8, 6.3], viaEntrega: [930, 470] },
  { cor: '#60a5fa', primario: 'B', slot: 1, prod: 2, cons: 1, envia: [2.0, 2.6], replica: [2.6, 3.2], entrega: [6.6, 7.4], ack: [7.4, 7.9], viaEntrega: [1051, 480] },
  { cor: '#f472b6', primario: 'C', slot: 2, prod: 1, cons: 2, envia: [3.4, 4.0], replica: [4.0, 4.6], entrega: [7.8, 8.6], ack: [8.6, 9.1], viaEnvio: [700, 100], viaEntrega: [930, 520] },
  { cor: '#fbbf24', primario: 'B', slot: 0, prod: 0, cons: 0, envia: [6.6, 7.2], replica: [7.2, 7.8], entrega: [9.4, 10.2], ack: [10.2, 10.7], viaEnvio: [1100, 70] },
  { cor: '#a78bfa', primario: 'C', slot: 1, prod: 1, cons: 1, envia: [8.4, 9.0], replica: [9.0, 9.6], entrega: [10.4, 11.2], ack: [11.2, 11.7], viaEnvio: [700, 100], viaEntrega: [809, 480] },
];
// linha do tempo (s) da animação; o quadro estático é o instante ESTATICO
const DURACAO = 12.5, FPS = 20, ESTATICO = 4.9;
// legenda que acompanha cada fase: [início, fim, texto]
const LEGENDA = [
  [0.3, 2.0, 'produtores publicam, sem esperar'],
  [2.0, 4.8, 'cada nó é o primário de parte das mensagens'],
  [4.8, 7.0, 'consumidores leem quando quiserem'],
  [7.0, 9.6, 'o ack remove a mensagem dos 3 nós'],
  [9.6, 12.2, 'produtores e consumidores vão e vêm'],
];

// ---------- utilitários ----------
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const n = (v) => Math.round(v * 100) / 100;
const clamp = (v) => Math.min(1, Math.max(0, v));
const suave = (p) => (p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2);
const prog = (t, [a, b]) => suave(clamp((t - a) / (b - a)));
const mistura = (a, b, p) => a + (b - a) * p;
// opacidade de uma janela [a,b] com entrada e saída curtas
const janela = (t, a, b, f = 0.15) => clamp(Math.min((t - a) / f, (b - t) / f));
const visivel = (t, janelas) => Math.max(...janelas.map(([a, b]) => janela(t, a, b, 0.3)));

function texto(x, y, s, { tam = 20, peso = 400, cor = TINTA, ancora = 'start', fonte = MONO, op = 1 } = {}) {
  const o = op < 1 ? ` opacity="${n(op)}"` : '';
  return `<text x="${n(x)}" y="${n(y)}" font-family="${fonte}" font-size="${tam}" font-weight="${peso}" fill="${cor}" text-anchor="${ancora}"${o}>${esc(s)}</text>`;
}

// ponto ao longo de um segmento ou de uma curva quadrática (com ponto de controle `via`, se houver)
const caminho = (p, ini, fim, via) => (via
  ? [(1 - p) * (1 - p) * ini[0] + 2 * (1 - p) * p * via[0] + p * p * fim[0],
     (1 - p) * (1 - p) * ini[1] + 2 * (1 - p) * p * via[1] + p * p * fim[1]]
  : [mistura(ini[0], fim[0], p), mistura(ini[1], fim[1], p)]);

// bolinha de mensagem (com brilho leve) numa posição
function bola([x, y], { cor = '#34d399', r = 9, op = 1 } = {}) {
  if (op <= 0.01) return '';
  return `<circle cx="${n(x)}" cy="${n(y)}" r="${r + 5}" fill="${cor}" opacity="${n(0.18 * op)}"/>` +
    `<circle cx="${n(x)}" cy="${n(y)}" r="${r}" fill="${cor}" opacity="${n(op)}"/>`;
}

// oca (tenda): triângulo, o motivo do projeto
function oca(x, y, larg, cor, peso = 3) {
  const h = larg * 0.95;
  return `<path d="M${n(x)} ${n(y - h / 2)} L${n(x + larg / 2)} ${n(y + h / 2)} L${n(x - larg / 2)} ${n(y + h / 2)} Z" fill="none" stroke="${cor}" stroke-width="${peso}" stroke-linejoin="round"/>`;
}

// geometria do prédio de um nó: base, topo dos andares, topo do telhado e centro de cada andar (0 = térreo)
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

// produtor ou consumidor: aparece e some com `op`; `marca` (cor) destaca o contorno enquanto ele segura uma mensagem
function caixa(c, op, marca) {
  if (op <= 0.01) return '';
  const raio = c.h / 2;
  return `<g opacity="${n(op)}"><rect x="${c.x}" y="${c.y}" width="${c.w}" height="${c.h}" rx="${raio}" fill="${PAINEL}" stroke="${LINHA}" stroke-width="1.5"/>` +
    (marca ? `<rect x="${c.x}" y="${c.y}" width="${c.w}" height="${c.h}" rx="${raio}" fill="${marca.cor}" fill-opacity="${n(0.16 * marca.op)}" stroke="${marca.cor}" stroke-width="3" opacity="${n(marca.op)}"/>` : '') +
    texto(c.x + c.w / 2, c.y + c.h / 2 + 6, c.texto, { tam: 18, ancora: 'middle', cor: SUAVE }) + '</g>';
}

// t = instante da animação em segundos
function direita(t) {
  const p = [];
  const ids = Object.keys(NOS);
  p.push(`<circle cx="${CX}" cy="${CY}" r="${R}" fill="none" stroke="${LINHA}" stroke-width="2" stroke-dasharray="4 10" stroke-linecap="round"/>`);
  TEXTO.centro.forEach((l, i) => p.push(texto(CX, CY - 38 + i * 24, l, { tam: 18, cor: SUAVE, ancora: 'middle' })));

  // produtores e consumidores; o consumidor destaca o contorno com a cor da mensagem que acabou de receber
  PRODUTORES.forEach((c) => p.push(caixa(c, visivel(t, c.janelas))));
  CONSUMIDORES.forEach((c, k) => {
    const m = MSGS.find((x) => x.cons === k && t >= x.entrega[1] && t < x.ack[1] + 0.15);
    p.push(caixa(c, visivel(t, c.janelas), m && { cor: m.cor, op: janela(t, m.entrega[1], m.ack[1] + 0.15, 0.15) }));
  });

  // prédios: os 3 andares vazios, depois cada mensagem acende o seu andar quando o nó a recebe e
  // apaga no ack
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
      p.push(texto(no.x, y0 + 20, primario ? 'primário' : 'réplica', { tam: 18, peso: primario ? 700 : 400, cor: primario ? TINTA : SUAVE, ancora: 'middle', op }));
    }
    // telhado (oca) com a letra do nó
    p.push(`<path d="M${no.x} ${n(b.topoTelhado)} L${no.x + PRED.larg / 2 - 6} ${n(b.topoAndares - 3)} L${no.x - PRED.larg / 2 + 6} ${n(b.topoAndares - 3)} Z" fill="${no.tom}" stroke="${no.cor}" stroke-width="3" stroke-linejoin="round"/>`);
    p.push(texto(no.x, b.topoAndares - 9, id, { tam: 18, peso: 700, ancora: 'middle' }));
  }

  // mensagens em trânsito: produtor → primário, primário → réplicas (no andar da mensagem) e
  // primário → consumidor
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

  // legenda da fase atual
  for (const [a, b, s] of LEGENDA) {
    const op = janela(t, a, b, 0.2);
    if (op > 0.01) p.push(texto(CX, 522, s, { tam: 20, cor: SUAVE, ancora: 'middle', op }));
  }
  return p.join('');
}

function svg(t, estatico = false) {
  const titulo = 'Taba: mensageria open source que substitui SQS, SNS, Kafka e RabbitMQ com uma só imagem Docker';
  const desc = 'Cluster de 3 nós, todos com a mesma imagem, desenhados como prédios de 3 andares. Vários produtores entram e saem e publicam mensagens; cada mensagem tem um nó primário, que a replica nos outros dois, e todo nó é o primário de parte delas. Consumidores chegam depois, leem as mensagens de forma assíncrona e confirmam com ack, e a mensagem sai dos 3 nós.';
  return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" lang="pt-BR" aria-labelledby="t d">
<title id="t">${esc(titulo)}</title>
<desc id="d">${esc(desc)}</desc>
<rect width="${W}" height="${H}" fill="${BG}"/>${esquerda()}${direita(estatico ? ESTATICO : t)}
</svg>
`;
}

// ---------- saída ----------
writeFileSync(resolve(dirImg, 'hero.svg'), svg(0, true));
console.log('SVG: doc/diagramas/img/hero.svg');

if (process.argv.includes('--so-svg')) process.exit(0);

// GIF: uma captura do Chrome por quadro, depois ffmpeg com paleta própria
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
if (!chrome) { console.error('Chrome/Edge não encontrado. Defina ARCHIFY_CHROME.'); process.exit(2); }
const ffmpeg = process.env.FFMPEG || 'ffmpeg';
if (spawnSync(ffmpeg, ['-version'], { stdio: 'ignore' }).status !== 0) {
  console.error('ffmpeg não encontrado. Instale-o ou defina FFMPEG.'); process.exit(2);
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
    const t = setTimeout(() => erro(new Error('Chrome não iniciou a tempo')), 30000);
    proc.stderr.on('data', (d) => {
      buf += d;
      const m = buf.match(/DevTools listening on ws:\/\/127\.0\.0\.1:(\d+)\//);
      if (m) { clearTimeout(t); ok(m[1]); }
    });
    proc.on('exit', () => erro(new Error('Chrome encerrou antes do esperado')));
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
    await cdp('Page.navigate', { url: 'data:text/html;charset=utf-8,' + encodeURIComponent(html) });
    await new Promise((ok) => setTimeout(ok, i === 0 ? 400 : 60));
    const r = await cdp('Page.captureScreenshot', { format: 'png', clip: { x: 0, y: 0, width: W, height: H, scale: 1 } });
    writeFileSync(join(tmp, `q${String(i).padStart(3, '0')}.png`), Buffer.from(r.result.data, 'base64'));
  }
  ws.close();

  const saida = resolve(dirImg, 'hero.gif');
  const r = spawnSync(ffmpeg, [
    '-y', '-loglevel', 'error', '-framerate', String(FPS), '-i', join(tmp, 'q%03d.png'),
    '-vf', 'split[a][b];[a]palettegen=max_colors=64:stats_mode=full[p];[b][p]paletteuse=dither=none',
    '-loop', '0', saida,
  ], { stdio: 'inherit' });
  if (r.status !== 0) { console.error('ffmpeg falhou'); process.exitCode = 1; }
  else console.log(`GIF: doc/diagramas/img/hero.gif (${quadros} quadros, ${DURACAO}s, ${FPS} fps)`);
} finally {
  encerrar();
}
