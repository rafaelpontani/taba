#!/usr/bin/env node
// Generates by hand the three circular hash diagrams (they do not go through Archify, which does not draw rings
// or matrices): doc/diagramas/img/anel-hash.svg, doc/diagramas/img/topico-espalhado.svg and
// doc/diagramas/img/fifo-grupos.svg.
// This is the source: to change the drawing, edit the constants below and run
//   node doc/diagramas/scripts/gerar-hash.mjs
import { writeFileSync, mkdirSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const raiz = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
// ---------- language ----------
// English is the source language. --lang=<code> uses doc/diagramas/i18n/<code>.json (flat table
// "English string": "translation"; a missing string stays in English) and writes to img/<code>/.
// Without --lang, every language is generated.
const arg = process.argv.find((a) => a.startsWith('--lang='));
const IDIOMAS = arg ? [arg.slice(7)] : ['en', ...readdirSync(resolve(raiz, 'doc/diagramas/i18n')).filter((f) => f.endsWith('.json')).map((f) => f.slice(0, -5))];
let LANG = 'en';
let TABELA = {};
const tr = (t) => TABELA[t] ?? t;
const saida = (nome) => resolve(raiz, 'doc/diagramas/img', LANG, nome);


// ---------- data ----------
const FONTE = "'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Consolas, 'DejaVu Sans Mono', monospace";
// dark theme, same color family as the Archify diagrams
const BG = '#020617', PAINEL = '#0b1220', TINTA = '#f8fafc', SUAVE = '#94a3b8', LINHA = '#334155';

// physical nodes: outline and fill color
const NOS = {
  A: { cor: '#22d3ee', tom: '#0b2a35' },
  B: { cor: '#a78bfa', tom: '#241a4a' },
  C: { cor: '#fbbf24', tom: '#33260a' },
  D: { cor: '#fb7185', tom: '#3a121e' },
};

// ring positions clockwise, from the top: each physical node appears in 3 positions (virtual nodes)
const ANEL = ['A1', 'C1', 'B1', 'D1', 'A2', 'A3', 'C2', 'B2', 'D2', 'B3', 'C3', 'D3'];

// the example messages: ring position where the hash lands (the block starts at the first virtual node ahead)
// roles by ring position; "skipped" = virtual node of an already chosen physical node
const MSGS = [
  { id: 'm1', cor: '#34d399', gap: 4, papeis: { 4: 'P', 5: 'skipped', 6: 'S', 7: 'T' } },
  { id: 'm2', cor: '#60a5fa', gap: 9, papeis: { 9: 'P', 10: 'S', 11: 'T' } },
];

// the `orders` topic matrix (m1 and m2 equal to the ring's): each node's role in each message
const TOPICO = [
  { id: 'm1', A: 'P', C: 'S', B: 'T' },
  { id: 'm2', B: 'P', C: 'S', D: 'T' },
  { id: 'm3', C: 'P', D: 'S', A: 'T' },
  { id: 'm4', D: 'P', A: 'S', C: 'T' },
  { id: 'm5', A: 'P', B: 'S', D: 'T' },
  { id: 'm6', B: 'P', D: 'S', C: 'T' },
];

// ---------- utilities ----------
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const n = (v) => Math.round(v * 100) / 100;

function texto(x, y, s, { tam = 12, peso = 400, cor = TINTA, ancora = 'start' } = {}) {
  return `<text x="${n(x)}" y="${n(y)}" font-size="${tam}" font-weight="${peso}" fill="${cor}" text-anchor="${ancora}">${esc(tr(s))}</text>`;
}

// breaks into lines of up to `max` characters (monospaced font)
function quebrar(s, max) {

  const linhas = [];
  let atual = '';
  for (const p of s.split(' ')) {
    if ((atual + ' ' + p).trim().length > max) { linhas.push(atual); atual = p; }
    else atual = (atual + ' ' + p).trim();
  }
  if (atual) linhas.push(atual);
  return linhas;
}

function paragrafo(x, y, s, max, { tam = 12, cor = TINTA, passo = 17 } = {}) {
  const linhas = quebrar(s, max);
  return { svg: linhas.map((l, i) => texto(x, y + i * passo, l, { tam, cor })).join(''), fim: y + linhas.length * passo };
}

function envolver(largura, altura, titulo, descricao, corpo) {
  return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${largura} ${altura}" width="${largura}" height="${altura}" role="img" lang="${LANG}" aria-labelledby="t d" font-family="${FONTE}">
<title id="t">${esc(tr(titulo))}</title>
<desc id="d">${esc(tr(descricao))}</desc>
<rect width="${largura}" height="${altura}" fill="${BG}"/>
${corpo}
</svg>
`;
}

// ---------- diagrama A: o anel ----------
function anel() {
  const W = 1080, H = 700, cx = 350, cy = 336, R = 200, r = 24;
  const ang = (k) => k * 30; // graus, no clockwise a partir do topo
  const pt = (graus, raio) => {
    const t = (graus * Math.PI) / 180;
    return [cx + raio * Math.sin(t), cy - raio * Math.cos(t)];
  };
  const arco = (de, ate, cor) => {
    const [x1, y1] = pt(de, R), [x2, y2] = pt(ate, R);
    const grande = ate - de > 180 ? 1 : 0;
    return `<path d="M${n(x1)} ${n(y1)} A${R} ${R} 0 ${grande} 1 ${n(x2)} ${n(y2)}" fill="none" stroke="${cor}" stroke-width="9" stroke-linecap="round" opacity="0.4"/>`;
  };

  let s = '';
  s += texto(40, 44, 'Circular hash ring: blocks and roles', { tam: 20, peso: 700 });
  s += texto(40, 66, '4 physical nodes, each at 3 ring positions (virtual nodes), replication factor 3', { tam: 12, cor: SUAVE });

  // anel
  s += `<circle cx="${cx}" cy="${cy}" r="${R}" fill="none" stroke="${LINHA}" stroke-width="3"/>`;
  s += texto(cx, cy - 6, 'clockwise', { tam: 13, peso: 700, cor: SUAVE, ancora: 'middle' });
  s += texto(cx, cy + 14, '↻', { tam: 26, cor: SUAVE, ancora: 'middle' });

  // stretches walked by each message
  for (const m of MSGS) {
    const ks = Object.keys(m.papeis).map(Number);
    s += arco(ang(m.gap) - 15, ang(Math.max(...ks)), m.cor);
  }

  // virtual nodes
  ANEL.forEach((nome, k) => {
    const [x, y] = pt(ang(k), R);
    const { cor, tom } = NOS[nome[0]];
    const skipped = MSGS.some((m) => m.papeis[k] === 'skipped');
    s += `<circle cx="${n(x)}" cy="${n(y)}" r="${r}" fill="${tom}" stroke="${cor}" stroke-width="2.5"${skipped ? ' stroke-dasharray="4 3"' : ''}/>`;
    s += texto(x, y + 5, nome, { tam: 14, peso: 700, cor: TINTA, ancora: 'middle' });
    if (skipped) s += `<line x1="${n(x - 17)}" y1="${n(y + 17)}" x2="${n(x + 17)}" y2="${n(y - 17)}" stroke="${cor}" stroke-width="2.5"/>`;
  });

  // message markers (in the gap before the block) and role badges
  for (const m of MSGS) {
    const a = ang(m.gap) - 15;
    const [mx, my] = pt(a, R);
    s += `<path d="M${n(mx)} ${n(my - 9)} L${n(mx + 9)} ${n(my)} L${n(mx)} ${n(my + 9)} L${n(mx - 9)} ${n(my)} Z" fill="${m.cor}" stroke="${BG}" stroke-width="2"/>`;
    const [lx, ly] = pt(a, R - 42);
    s += texto(lx, ly - 2, m.id, { tam: 15, peso: 700, cor: m.cor, ancora: 'middle' });
    s += texto(lx, ly + 13, 'hash(ID)', { tam: 10, cor: SUAVE, ancora: 'middle' });

    for (const [k, papel] of Object.entries(m.papeis)) {
      const [bx, by] = pt(ang(Number(k)), R + 56);
      const rotulo = papel === 'skipped' ? `${m.id} ${tr('skipped')}` : `${m.id} ${papel}`;
      const w = papel === 'skipped' ? 92 : 56;
      const cheio = papel === 'P';
      const fundo = papel === 'skipped' ? '#1e293b' : cheio ? m.cor : BG;
      const tracoCor = papel === 'skipped' ? SUAVE : m.cor;
      const letra = papel === 'skipped' ? SUAVE : cheio ? BG : m.cor;
      s += `<rect x="${n(bx - w / 2)}" y="${n(by - 12)}" width="${w}" height="24" rx="12" fill="${fundo}" stroke="${tracoCor}" stroke-width="2"/>`;
      s += texto(bx, by + 4.5, rotulo, { tam: 12, peso: 700, cor: letra, ancora: 'middle' });
    }
  }

  // right panel
  const px = 700;
  s += `<line x1="${px - 24}" y1="96" x2="${px - 24}" y2="${H - 40}" stroke="${LINHA}" stroke-width="1.5"/>`;
  s += texto(px, 112, 'How to read the ring', { tam: 15, peso: 700 });
  const passos = [
    'The message ID becomes a hash: a point on the ring (diamond).',
    'Walking clockwise, the first virtual node is the primary (P): the owner of delivery.',
    'The next physical nodes not used yet are the secondary (S) and the tertiary (T), until the replication factor is met.',
    'A virtual node of an already chosen physical node is skipped: replicas always sit on different machines.',
    'Each physical node holds several positions, so it gets blocks spread around the ring, not one continuous stretch.',
  ];
  let y = 138;
  passos.forEach((p, i) => {
    s += `<circle cx="${px + 10}" cy="${y - 4}" r="10" fill="${TINTA}"/>`;
    s += texto(px + 10, y, String(i + 1), { tam: 12, peso: 700, cor: BG, ancora: 'middle' });
    const par = paragrafo(px + 32, y, p, 40, { tam: 12 });
    s += par.svg;
    y = par.fim + 12;
  });

  // result
  y += 6;
  s += texto(px, y, 'Result of the two examples', { tam: 15, peso: 700 });
  y += 14;
  const cab = ['', 'P', 'S', 'T'];
  const colX = [px, px + 70, px + 160, px + 250];
  cab.forEach((c, i) => { s += texto(colX[i] + (i ? 26 : 0), y + 14, c, { tam: 12, peso: 700, cor: SUAVE, ancora: i ? 'middle' : 'start' }); });
  y += 22;
  const donos = { m1: ['A', 'C', 'B'], m2: ['B', 'C', 'D'] };
  MSGS.forEach((m) => {
    s += `<rect x="${px - 6}" y="${y}" width="352" height="34" rx="8" fill="${PAINEL}" stroke="${LINHA}"/>`;
    s += texto(px + 6, y + 22, m.id, { tam: 13, peso: 700, cor: m.cor });
    donos[m.id].forEach((no, i) => {
      const cxn = colX[i + 1] + 26;
      s += `<circle cx="${cxn}" cy="${y + 17}" r="12" fill="${NOS[no].tom}" stroke="${NOS[no].cor}" stroke-width="2"/>`;
      s += texto(cxn, y + 21.5, no, { tam: 13, peso: 700, ancora: 'middle' });
    });
    y += 42;
  });
  y += 14;
  const nota = paragrafo(px, y, 'Different blocks have different primaries: m1 is owned by A; m2 by B.', 44, { tam: 12, cor: SUAVE });
  s += nota.svg;

  // node legend
  const ly = H - 34;
  s += texto(40, ly - 4, 'Physical nodes', { tam: 12, peso: 700, cor: SUAVE });
  Object.entries(NOS).forEach(([nome, c], i) => {
    const x = 150 + i * 70;
    s += `<circle cx="${x}" cy="${ly - 8}" r="11" fill="${c.tom}" stroke="${c.cor}" stroke-width="2"/>`;
    s += texto(x, ly - 3.5, nome, { tam: 12, peso: 700, ancora: 'middle' });
  });
  s += texto(450, ly - 4, 'A1, A2, A3: virtual nodes of physical node A', { tam: 12, cor: SUAVE });

  return envolver(W, H, 'Circular hash ring: blocks and roles',
    'A ring with 12 positions of 4 physical nodes. Two messages land on points of the ring and, walking clockwise, the first virtual node is the primary, followed by the secondary and the tertiary, skipping virtual nodes of an already chosen physical node.', s);
}

// ---------- diagram B: the spread topic ----------
function topico() {
  const W = 1080, H = 640;
  const nos = Object.keys(NOS);
  const x0 = 200, larg = 118, y0 = 150, alt = 54, cw = 92, ch = 38;

  let s = '';
  s += texto(40, 44, 'A spread, redundant topic', { tam: 20, peso: 700 });
  s += texto(40, 66, 'topic `orders`: 6 messages on 4 nodes, replication factor 3 (m1 and m2 are the ones in the ring diagram)', { tam: 12, cor: SUAVE });

  // column headers
  s += texto(60, y0 - 26, 'message', { tam: 12, peso: 700, cor: SUAVE });
  nos.forEach((no, j) => {
    const cx = x0 + j * larg + larg / 2;
    s += `<circle cx="${cx}" cy="${y0 - 34}" r="15" fill="${NOS[no].tom}" stroke="${NOS[no].cor}" stroke-width="2.5"/>`;
    s += texto(cx, y0 - 29, no, { tam: 15, peso: 700, ancora: 'middle' });
  });

  const totais = Object.fromEntries(nos.map((no) => [no, { P: 0, S: 0, T: 0 }]));

  TOPICO.forEach((lin, i) => {
    const y = y0 + i * alt;
    s += `<rect x="40" y="${y - 6}" width="${x0 - 40 + nos.length * larg + 10}" height="${alt - 6}" rx="10" fill="${i % 2 ? 'none' : PAINEL}"/>`;
    s += texto(60, y + 24, lin.id, { tam: 15, peso: 700 });
    nos.forEach((no, j) => {
      const papel = lin[no];
      const cx = x0 + j * larg + (larg - cw) / 2;
      if (!papel) {
        s += `<rect x="${cx}" y="${y}" width="${cw}" height="${ch}" rx="9" fill="none" stroke="${LINHA}" stroke-dasharray="3 4"/>`;
        return;
      }
      totais[no][papel]++;
      const cor = NOS[no].cor;
      const cheio = papel === 'P';
      s += `<rect x="${cx}" y="${y}" width="${cw}" height="${ch}" rx="9" fill="${cheio ? cor : papel === 'S' ? NOS[no].tom : BG}" stroke="${cor}" stroke-width="2.5"/>`;
      s += texto(cx + cw / 2, y + 24, papel, { tam: 12, peso: 700, cor: cheio ? BG : TINTA, ancora: 'middle' });
    });
  });

  // totals per node
  const yt = y0 + TOPICO.length * alt + 6;
  s += `<line x1="40" y1="${yt}" x2="${x0 + nos.length * larg + 10}" y2="${yt}" stroke="${LINHA}" stroke-width="1.5"/>`;
  s += texto(60, yt + 24, 'each node stores', { tam: 12, peso: 700, cor: SUAVE });
  nos.forEach((no, j) => {
    const t = totais[no];
    const cx = x0 + j * larg + larg / 2;
    s += texto(cx, yt + 22, `${t.P + t.S + t.T} ${tr('of')} ${TOPICO.length}`, { tam: 14, peso: 700, ancora: 'middle' });
    s += texto(cx, yt + 40, `${t.P}P ${t.S}S ${t.T}T`, { tam: 11, cor: SUAVE, ancora: 'middle' });
  });

  // legenda
  const yl = yt + 78;
  const leg = [
    ['P', 'primary (owner)', 'cheio', 60],
    ['S', 'secondary', 'meio', 240],
    ['T', 'tertiary', 'vazio', 380],
  ];
  leg.forEach(([l, d, tipo, x]) => {
    s += `<rect x="${x}" y="${yl - 14}" width="30" height="22" rx="6" fill="${tipo === 'cheio' ? '#94a3b8' : tipo === 'meio' ? '#1e293b' : BG}" stroke="#94a3b8" stroke-width="2"/>`;
    s += texto(x + 15, yl + 2, l, { tam: 12, peso: 700, cor: tipo === 'cheio' ? BG : TINTA, ancora: 'middle' });
    s += texto(x + 40, yl + 2, d, { tam: 11, cor: SUAVE });
  });
  s += `<rect x="510" y="${yl - 14}" width="30" height="22" rx="6" fill="none" stroke="${LINHA}" stroke-dasharray="3 4"/>`;
  s += texto(550, yl + 2, 'does not have the message', { tam: 11, cor: SUAVE });

  // cards on the right
  const px = 770, cw2 = 270;
  const cartao = (y, titulo, cor, itens) => {
    let g = '';
    const linhas = itens.map((t) => quebrar(t, 33));
    const altura = 44 + linhas.reduce((a, l) => a + l.length * 17 + 8, 0);
    g += `<rect x="${px}" y="${y}" width="${cw2}" height="${altura}" rx="12" fill="${PAINEL}" stroke="${LINHA}"/>`;
    g += `<circle cx="${px + 20}" cy="${y + 24}" r="6" fill="${cor}"/>`;
    g += texto(px + 34, y + 29, titulo, { tam: 14, peso: 700 });
    let yy = y + 52;
    linhas.forEach((ls) => {
      ls.forEach((l, i) => { g += texto(px + 20, yy + i * 17, (i === 0 ? '• ' : '  ') + l, { tam: 12 }); });
      yy += ls.length * 17 + 8;
    });
    return { g, fim: y + altura };
  };
  const c1 = cartao(112, 'Spread', '#22d3ee', [
    'The ID decides the block, not the topic: messages land all around the ring.',
    'No node stores everything or owns the topic. Each one has 4 or 5 of the 6.',
    'The primaries take turns among A, B, C and D.',
  ]);
  const c2 = cartao(c1.fim + 16, 'Redundant', '#34d399', [
    'Each message exists on 3 different physical nodes.',
    'With a quorum of 2 out of 3, writes continue with one replica down.',
  ]);
  const c3 = cartao(c2.fim + 16, 'If node B goes down', '#fb7185', [
    'm1, m2, m5 and m6 stay on 2 nodes; none is lost.',
    'The blocks of B move to the next node on the ring and the replicas are rebuilt by synchronization.',
  ]);
  s += c1.g + c2.g + c3.g;

  return envolver(W, Math.max(H, c3.fim + 30), 'A spread, redundant topic',
    'A matrix with messages m1 to m6 of topic orders in the rows and nodes A, B, C and D in the columns. Each message has a primary and two replicas on different nodes, and no node stores all the messages.', s);
}

// ---------- diagram C: the groups of a FIFO queue ----------
// FIFO queue `orders-fifo`: each group has a position on the ring (hash of the topic id + groupId),
// so a single block; the group's messages (in publish order) all live in that block
const GRUPOS = [
  { id: 'g1', msgs: ['m1', 'm3', 'm5'], A: 'P', C: 'S', B: 'T' },
  { id: 'g2', msgs: ['m2', 'm4'], B: 'P', C: 'S', D: 'T' },
  { id: 'g3', msgs: ['m6'], C: 'P', D: 'S', A: 'T' },
  { id: 'g4', msgs: ['m7', 'm8'], D: 'P', A: 'S', C: 'T' },
];

function fifo() {
  const W = 1080, H = 640;
  const nos = Object.keys(NOS);
  const x0 = 200, larg = 118, y0 = 150, alt = 54, cw = 92, ch = 38;

  let s = '';
  s += texto(40, 44, 'The groups of a FIFO queue on the ring', { tam: 20, peso: 700 });
  s += texto(40, 66, 'queue `orders-fifo`: 4 groups on 4 nodes, replication factor 3; the group decides the block, not the message ID', { tam: 12, cor: SUAVE });

  s += texto(60, y0 - 26, 'group', { tam: 12, peso: 700, cor: SUAVE });
  nos.forEach((no, j) => {
    const cx = x0 + j * larg + larg / 2;
    s += `<circle cx="${cx}" cy="${y0 - 34}" r="15" fill="${NOS[no].tom}" stroke="${NOS[no].cor}" stroke-width="2.5"/>`;
    s += texto(cx, y0 - 29, no, { tam: 15, peso: 700, ancora: 'middle' });
  });

  const totais = Object.fromEntries(nos.map((no) => [no, { P: 0, S: 0, T: 0 }]));

  GRUPOS.forEach((lin, i) => {
    const y = y0 + i * alt;
    s += `<rect x="40" y="${y - 6}" width="${x0 - 40 + nos.length * larg + 10}" height="${alt - 6}" rx="10" fill="${i % 2 ? 'none' : PAINEL}"/>`;
    s += texto(60, y + 18, lin.id, { tam: 15, peso: 700 });
    s += texto(60, y + 34, lin.msgs.join(' → '), { tam: 11, cor: SUAVE });
    nos.forEach((no, j) => {
      const papel = lin[no];
      const cx = x0 + j * larg + (larg - cw) / 2;
      if (!papel) {
        s += `<rect x="${cx}" y="${y}" width="${cw}" height="${ch}" rx="9" fill="none" stroke="${LINHA}" stroke-dasharray="3 4"/>`;
        return;
      }
      totais[no][papel]++;
      const cor = NOS[no].cor;
      const cheio = papel === 'P';
      s += `<rect x="${cx}" y="${y}" width="${cw}" height="${ch}" rx="9" fill="${cheio ? cor : papel === 'S' ? NOS[no].tom : BG}" stroke="${cor}" stroke-width="2.5"/>`;
      s += texto(cx + cw / 2, y + 24, papel, { tam: 12, peso: 700, cor: cheio ? BG : TINTA, ancora: 'middle' });
    });
  });

  const yt = y0 + GRUPOS.length * alt + 6;
  s += `<line x1="40" y1="${yt}" x2="${x0 + nos.length * larg + 10}" y2="${yt}" stroke="${LINHA}" stroke-width="1.5"/>`;
  s += texto(60, yt + 24, 'each node is primary of', { tam: 12, peso: 700, cor: SUAVE });
  nos.forEach((no, j) => {
    const t = totais[no];
    const cx = x0 + j * larg + larg / 2;
    s += texto(cx, yt + 22, `${t.P} ${tr(t.P === 1 ? 'group' : 'groups')}`, { tam: 14, peso: 700, ancora: 'middle' });
    s += texto(cx, yt + 40, `${t.P}P ${t.S}S ${t.T}T`, { tam: 11, cor: SUAVE, ancora: 'middle' });
  });

  const yl = yt + 78;
  const leg = [
    ['P', 'primary: orders and delivers', 'cheio', 60],
    ['S', 'secondary: in the quorum', 'meio', 290],
    ['T', 'tertiary', 'vazio', 510],
  ];
  leg.forEach(([l, d, tipo, x]) => {
    s += `<rect x="${x}" y="${yl - 14}" width="30" height="22" rx="6" fill="${tipo === 'cheio' ? '#94a3b8' : tipo === 'meio' ? '#1e293b' : BG}" stroke="#94a3b8" stroke-width="2"/>`;
    s += texto(x + 15, yl + 2, l, { tam: 12, peso: 700, cor: tipo === 'cheio' ? BG : TINTA, ancora: 'middle' });
    s += texto(x + 40, yl + 2, d, { tam: 11, cor: SUAVE });
  });

  const px = 770, cw2 = 270;
  const cartao = (y, titulo, cor, itens) => {
    let g = '';
    const linhas = itens.map((t) => quebrar(t, 33));
    const altura = 44 + linhas.reduce((a, l) => a + l.length * 17 + 8, 0);
    g += `<rect x="${px}" y="${y}" width="${cw2}" height="${altura}" rx="12" fill="${PAINEL}" stroke="${LINHA}"/>`;
    g += `<circle cx="${px + 20}" cy="${y + 24}" r="6" fill="${cor}"/>`;
    g += texto(px + 34, y + 29, titulo, { tam: 14, peso: 700 });
    let yy = y + 52;
    linhas.forEach((ls) => {
      ls.forEach((l, i) => { g += texto(px + 20, yy + i * 17, (i === 0 ? '• ' : '  ') + l, { tam: 12 }); });
      yy += ls.length * 17 + 8;
    });
    return { g, fim: y + altura };
  };
  const c1 = cartao(112, 'One group, one block', '#22d3ee', [
    'The hash of the topic id plus the groupId decides the block: all messages of the group stay on the same 3 nodes.',
    'The primary generates the IDs in increasing order and delivers one message of the group at a time.',
  ]);
  const c2 = cartao(c1.fim + 16, 'Spread across groups', '#34d399', [
    'Different groups land in different blocks: the primaries take turns.',
    'A big group never goes beyond one primary (hot group).',
  ]);
  const c3 = cartao(c2.fim + 16, 'If the primary of g1 goes down', '#fb7185', [
    'Secondary C takes over g1 already up to date, since the quorum always included it.',
    'The order of g1 and the deduplication carry on without loss.',
  ]);
  s += c1.g + c2.g + c3.g;

  return envolver(W, Math.max(H, c3.fim + 30), 'The groups of a FIFO queue on the ring',
    'A matrix with groups g1 to g4 of queue orders-fifo in the rows and nodes A, B, C and D in the columns. Each group has a primary, a secondary and a tertiary, and all the messages of a group stay on the same nodes.', s);
}

for (const lang of IDIOMAS) {
  LANG = lang;
  TABELA = lang === 'en' ? {} : JSON.parse(readFileSync(resolve(raiz, 'doc/diagramas/i18n', lang + '.json'), 'utf8'));
  mkdirSync(resolve(raiz, 'doc/diagramas/img', lang), { recursive: true });
  writeFileSync(saida('anel-hash.svg'), anel());
  writeFileSync(saida('topico-espalhado.svg'), topico());
  writeFileSync(saida('fifo-grupos.svg'), fifo());
  console.log(`generated (${lang}): doc/diagramas/img/${lang}/anel-hash.svg, topico-espalhado.svg, fifo-grupos.svg`);
}
