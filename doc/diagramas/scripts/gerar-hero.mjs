#!/usr/bin/env node
// Gera à mão o hero do README da raiz (não passa pelo Archify): doc/diagramas/img/hero.svg (quadro
// estático, com a mensagem já replicada) e doc/diagramas/img/hero.gif (animação da mensagem).
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
const MSG = '#34d399', DESTAQUE = '#22d3ee';

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
const CX = 930, CY = 275, R = 140;
const NOS = {
  A: { x: CX, y: CY - R, cor: '#22d3ee', tom: '#0b2a35' },
  B: { x: CX + Math.round(R * Math.cos(Math.PI / 6)), y: CY + R / 2, cor: '#a78bfa', tom: '#241a4a' },
  C: { x: CX - Math.round(R * Math.cos(Math.PI / 6)), y: CY + R / 2, cor: '#fbbf24', tom: '#33260a' },
};
const RAIO_NO = 34;
const PRODUTOR = { x: 740, y: 26, w: 150, h: 44, texto: 'produtor' };
const CONSUMIDOR = { x: 1010, y: 452, w: 170, h: 44, texto: 'consumidor' };

// linha do tempo (s) da animação; o quadro estático é o instante ESTATICO
const DURACAO = 6, FPS = 20, ESTATICO = 2.7;
const T = {
  envia: [0.5, 1.3],        // produtor → nó A (primário)
  replica: [1.4, 2.2],      // A → B e A → C
  confirma: [2.3, 2.6],     // "201" volta ao produtor
  entrega: [3.3, 4.3],      // A → consumidor
  ack: [4.4, 4.9],          // cópias somem
};
// legenda que acompanha cada fase: [início, fim, texto]
const LEGENDA = [
  [0.4, 1.4, 'produtor publica: POST'],
  [1.4, 2.4, 'replica nos 3 nós, em memória'],
  [2.4, 3.3, 'confirmado: 201 ao produtor'],
  [3.3, 4.4, 'consumidor lê: GET'],
  [4.4, 5.2, 'ack: a mensagem sai do tópico'],
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

function texto(x, y, s, { tam = 20, peso = 400, cor = TINTA, ancora = 'start', fonte = MONO, op = 1 } = {}) {
  const o = op < 1 ? ` opacity="${n(op)}"` : '';
  return `<text x="${n(x)}" y="${n(y)}" font-family="${fonte}" font-size="${tam}" font-weight="${peso}" fill="${cor}" text-anchor="${ancora}"${o}>${esc(s)}</text>`;
}

// ponto ao longo de um segmento ou de uma curva quadrática
const seg = (p, [x1, y1], [x2, y2]) => [mistura(x1, x2, p), mistura(y1, y2, p)];
const quad = (p, [x1, y1], [cx, cy], [x2, y2]) => [
  (1 - p) * (1 - p) * x1 + 2 * (1 - p) * p * cx + p * p * x2,
  (1 - p) * (1 - p) * y1 + 2 * (1 - p) * p * cy + p * p * y2,
];

// bolinha de mensagem (com brilho leve) numa posição
function bola([x, y], { cor = MSG, r = 9, op = 1 } = {}) {
  if (op <= 0.01) return '';
  return `<circle cx="${n(x)}" cy="${n(y)}" r="${r + 5}" fill="${cor}" opacity="${n(0.18 * op)}"/>` +
    `<circle cx="${n(x)}" cy="${n(y)}" r="${r}" fill="${cor}" opacity="${n(op)}"/>`;
}

// oca (tenda): triângulo com a porta, o motivo do projeto
function oca(x, y, larg, cor, peso = 3) {
  const h = larg * 0.95;
  return `<path d="M${n(x)} ${n(y - h / 2)} L${n(x + larg / 2)} ${n(y + h / 2)} L${n(x - larg / 2)} ${n(y + h / 2)} Z" fill="none" stroke="${cor}" stroke-width="${peso}" stroke-linejoin="round"/>`;
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

function caixa(c) {
  return `<rect x="${c.x}" y="${c.y}" width="${c.w}" height="${c.h}" rx="22" fill="${PAINEL}" stroke="${LINHA}" stroke-width="1.5"/>` +
    texto(c.x + c.w / 2, c.y + 29, c.texto, { tam: 20, ancora: 'middle', cor: SUAVE });
}

// t = instante da animação em segundos
function direita(t) {
  const p = [];
  p.push(`<circle cx="${CX}" cy="${CY}" r="${R}" fill="none" stroke="${LINHA}" stroke-width="2" stroke-dasharray="4 10" stroke-linecap="round"/>`);
  p.push(caixa(PRODUTOR), caixa(CONSUMIDOR));
  TEXTO.centro.forEach((l, i) => p.push(texto(CX, CY + 8 + i * 24, l, { tam: 18, cor: SUAVE, ancora: 'middle' })));

  const pEnvia = prog(t, T.envia), pRep = prog(t, T.replica), pEntrega = prog(t, T.entrega);
  const somem = 1 - prog(t, T.ack);
  const temCopia = t >= T.replica[0]; // A guarda a mensagem desde a chegada
  const chegouA = t >= T.envia[1];
  const ackFeito = t >= T.ack[1];

  // papéis e cópias em cada nó, enquanto a mensagem existe no tópico
  const existe = chegouA && !ackFeito;
  const papeis = { A: 'primário', B: 'réplica', C: 'réplica' };
  for (const [id, no] of Object.entries(NOS)) {
    const ativo = existe && (id === 'A' || (temCopia && pRep >= 1 && (id === 'B' || id === 'C')));
    const op = ativo ? somem : 0;
    p.push(`<circle cx="${no.x}" cy="${no.y}" r="${RAIO_NO + 6}" fill="none" stroke="${no.cor}" stroke-width="2" opacity="${n(0.35 * op)}"/>`);
    p.push(`<circle cx="${no.x}" cy="${no.y}" r="${RAIO_NO}" fill="${no.tom}" stroke="${no.cor}" stroke-width="2.5"/>`);
    p.push(oca(no.x, no.y - 2, 34, no.cor, 2.5));
    p.push(texto(no.x, no.y + 12, id, { tam: 18, peso: 700, ancora: 'middle' }));
    if (op > 0.01 && papeis[id]) {
      const [dx, dy, anc] = id === 'A' ? [RAIO_NO + 14, 6, 'start'] : id === 'B' ? [0, RAIO_NO + 30, 'middle'] : [0, RAIO_NO + 30, 'middle'];
      p.push(texto(no.x + dx, no.y + dy, papeis[id], { tam: 18, cor: no.cor, ancora: anc, op }));
    }
  }

  // mensagem: produtor → A
  const pIni = [PRODUTOR.x + PRODUTOR.w - 6, PRODUTOR.y + PRODUTOR.h - 2];
  const aTopo = [NOS.A.x - 22, NOS.A.y - RAIO_NO + 4];
  if (t >= T.envia[0] && t < T.envia[1] + 0.02) p.push(bola(seg(pEnvia, pIni, aTopo), { op: 1 }));

  // réplicas: A → B e A → C
  if (t >= T.replica[0] && pRep < 1) {
    p.push(bola(seg(pRep, [NOS.A.x + 20, NOS.A.y + 28], [NOS.B.x - 10, NOS.B.y - 32]), { cor: NOS.B.cor }));
    p.push(bola(seg(pRep, [NOS.A.x - 20, NOS.A.y + 28], [NOS.C.x + 10, NOS.C.y - 32]), { cor: NOS.C.cor }));
  }
  // a mensagem em repouso dentro de A
  if (existe && pEntrega === 0) p.push(bola([NOS.A.x + 4, NOS.A.y + 24], { r: 6, op: somem }));

  // confirmação 201 sobe ao produtor
  const opOk = janela(t, T.confirma[0], T.entrega[0] + 0.3, 0.2);
  if (opOk > 0.01) p.push(texto(PRODUTOR.x + PRODUTOR.w + 12, PRODUTOR.y + 29, '201 ✓', { tam: 20, peso: 700, cor: MSG, op: opOk }));

  // entrega: A → consumidor, por fora do anel
  if (t >= T.entrega[0] && pEntrega < 1) {
    const ini = [NOS.A.x + 26, NOS.A.y + 24], ctl = [1170, 190], fim = [CONSUMIDOR.x + CONSUMIDOR.w - 36, CONSUMIDOR.y];
    p.push(bola(quad(pEntrega, ini, ctl, fim)));
  }
  const opRecebeu = janela(t, T.entrega[1], T.ack[1], 0.15);
  if (opRecebeu > 0.01) p.push(bola([CONSUMIDOR.x + CONSUMIDOR.w - 30, CONSUMIDOR.y + CONSUMIDOR.h / 2], { r: 7, op: opRecebeu }));

  // legenda da fase atual
  for (const [a, b, s] of LEGENDA) {
    const op = janela(t, a, b, 0.2);
    if (op > 0.01) p.push(texto(CX, 522, s, { tam: 20, cor: SUAVE, ancora: 'middle', op }));
  }
  return p.join('');
}

function svg(t, estatico = false) {
  const titulo = 'Taba: mensageria open source que substitui SQS, SNS, Kafka e RabbitMQ em um único container';
  const desc = 'Cluster de 3 nós, todos com a mesma imagem. O produtor publica uma mensagem por POST, o nó primário a replica em mais dois nós, em memória, e o consumidor a recebe por GET e confirma com ack.';
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
