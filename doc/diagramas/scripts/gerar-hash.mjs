#!/usr/bin/env node
// Gera à mão os três diagramas do hash circular (não passam pelo Archify, que não desenha anéis
// nem matrizes): doc/diagramas/img/anel-hash.svg, doc/diagramas/img/topico-espalhado.svg e
// doc/diagramas/img/fifo-grupos.svg.
// Esta é a fonte: para mudar o desenho, edite as constantes abaixo e rode
//   node doc/diagramas/scripts/gerar-hash.mjs
import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const raiz = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const saida = (nome) => resolve(raiz, 'doc/diagramas/img', nome);
mkdirSync(resolve(raiz, 'doc/diagramas/img'), { recursive: true });

// ---------- idioma ----------
// --lang=en gera anel-hash-en.svg e topico-espalhado-en.svg (README em inglês); sem a opção, o português.
const LANG = (process.argv.find((a) => a.startsWith('--lang=')) || '--lang=pt').slice(7);
const SUF = LANG === 'pt' ? '' : '-' + LANG;
const EN = {
  'Anel de hash circular: blocos e papéis': 'Circular hash ring: blocks and roles',
  'cluster de 4 nós físicos, cada um em 3 posições do anel (nós virtuais), fator de replicação 3': '4 physical nodes, each at 3 ring positions (virtual nodes), replication factor 3',
  'sentido horário': 'clockwise',
  'Como ler o anel': 'How to read the ring',
  'O ID da mensagem vira um hash: um ponto no anel (losango).': 'The message ID becomes a hash: a point on the ring (diamond).',
  'Andando no sentido horário, o primeiro nó virtual é o primário (P): o dono da entrega.': 'Walking clockwise, the first virtual node is the primary (P): the owner of delivery.',
  'Os próximos nós físicos ainda não usados são o secundário (S) e o terciário (T), até completar o fator de replicação.': 'The next physical nodes not used yet are the secondary (S) and the tertiary (T), until the replication factor is met.',
  'Nó virtual de um nó físico já escolhido é pulado: as réplicas ficam sempre em máquinas distintas.': 'A virtual node of an already chosen physical node is skipped: replicas always sit on different machines.',
  'Cada nó físico ocupa várias posições, então recebe blocos espalhados pelo anel, e não uma faixa contínua.': 'Each physical node holds several positions, so it gets blocks spread around the ring, not one continuous stretch.',
  'Resultado dos dois exemplos': 'Result of the two examples',
  'Blocos diferentes têm primários diferentes: m1 tem A como dono; m2 tem B.': 'Different blocks have different primaries: m1 is owned by A; m2 by B.',
  'Nós físicos': 'Physical nodes',
  'A1, A2, A3: nós virtuais do nó físico A': 'A1, A2, A3: virtual nodes of physical node A',
  'pulado': 'skipped',
  'Anel com 12 posições de 4 nós físicos. Duas mensagens caem em pontos do anel e, andando no sentido horário, o primeiro nó virtual é o primário, seguido do secundário e do terciário, pulando nós virtuais de um nó físico já escolhido.': 'A ring with 12 positions of 4 physical nodes. Two messages land on points of the ring and, walking clockwise, the first virtual node is the primary, followed by the secondary and the tertiary, skipping virtual nodes of an already chosen physical node.',
  'Um tópico espalhado e redundante': 'A spread, redundant topic',
  'tópico `pedidos`: 6 mensagens em 4 nós, fator de replicação 3 (m1 e m2 são as do diagrama do anel)': 'topic `orders`: 6 messages on 4 nodes, replication factor 3 (m1 and m2 are the ones in the ring diagram)',
  'mensagem': 'message',
  'cada nó guarda': 'each node stores',
  'de': 'of',
  'primário (dono)': 'primary (owner)',
  'secundário': 'secondary',
  'terciário': 'tertiary',
  'não tem a mensagem': 'does not have the message',
  'Espalhado': 'Spread',
  'O ID decide o bloco, não o tópico: as mensagens caem em todo o anel.': 'The ID decides the block, not the topic: messages land all around the ring.',
  'Nenhum nó guarda tudo, nem é dono do tópico. Cada um tem 4 ou 5 das 6.': 'No node stores everything or owns the topic. Each one has 4 or 5 of the 6.',
  'Os primários se revezam entre A, B, C e D.': 'The primaries take turns among A, B, C and D.',
  'Redundante': 'Redundant',
  'Cada mensagem existe em 3 nós físicos distintos.': 'Each message exists on 3 different physical nodes.',
  'Com quórum de 2 de 3, a escrita continua com uma réplica fora do ar.': 'With a quorum of 2 out of 3, writes continue with one replica down.',
  'Se o nó B cair': 'If node B goes down',
  'm1, m2, m5 e m6 continuam em 2 nós; nenhuma se perde.': 'm1, m2, m5 and m6 stay on 2 nodes; none is lost.',
  'Os blocos de B passam ao próximo nó do anel e as réplicas são refeitas pela sincronização (seção 7.3).': 'The blocks of B move to the next node on the ring and the replicas are rebuilt by synchronization.',
  'Matriz com as mensagens m1 a m6 do tópico pedidos nas linhas e os nós A, B, C e D nas colunas. Cada mensagem tem um primário e duas réplicas em nós distintos, e nenhum nó guarda todas as mensagens.': 'A matrix with messages m1 to m6 of topic orders in the rows and nodes A, B, C and D in the columns. Each message has a primary and two replicas on different nodes, and no node stores all the messages.',
};
const tr = (t) => (LANG === 'en' ? (EN[t] ?? t) : t);

// ---------- dados ----------
const FONTE = "'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Consolas, 'DejaVu Sans Mono', monospace";
// tema escuro, mesma família de cores dos diagramas do Archify
const BG = '#020617', PAINEL = '#0b1220', TINTA = '#f8fafc', SUAVE = '#94a3b8', LINHA = '#334155';

// nós físicos: cor de contorno e de preenchimento
const NOS = {
  A: { cor: '#22d3ee', tom: '#0b2a35' },
  B: { cor: '#a78bfa', tom: '#241a4a' },
  C: { cor: '#fbbf24', tom: '#33260a' },
  D: { cor: '#fb7185', tom: '#3a121e' },
};

// posições do anel no sentido horário, a partir do topo: cada nó físico aparece em 3 posições (nós virtuais)
const ANEL = ['A1', 'C1', 'B1', 'D1', 'A2', 'A3', 'C2', 'B2', 'D2', 'B3', 'C3', 'D3'];

// mensagens do exemplo: posição do anel onde o hash cai (o bloco começa no primeiro nó virtual à frente)
// papéis por posição do anel; "pulado" = nó virtual de um nó físico já escolhido
const MSGS = [
  { id: 'm1', cor: '#34d399', gap: 4, papeis: { 4: 'P', 5: 'pulado', 6: 'S', 7: 'T' } },
  { id: 'm2', cor: '#60a5fa', gap: 9, papeis: { 9: 'P', 10: 'S', 11: 'T' } },
];

// matriz do tópico `pedidos` (m1 e m2 iguais às do anel): papel de cada nó em cada mensagem
const TOPICO = [
  { id: 'm1', A: 'P', C: 'S', B: 'T' },
  { id: 'm2', B: 'P', C: 'S', D: 'T' },
  { id: 'm3', C: 'P', D: 'S', A: 'T' },
  { id: 'm4', D: 'P', A: 'S', C: 'T' },
  { id: 'm5', A: 'P', B: 'S', D: 'T' },
  { id: 'm6', B: 'P', D: 'S', C: 'T' },
];

// ---------- utilitários ----------
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const n = (v) => Math.round(v * 100) / 100;

function texto(x, y, s, { tam = 12, peso = 400, cor = TINTA, ancora = 'start' } = {}) {
  return `<text x="${n(x)}" y="${n(y)}" font-size="${tam}" font-weight="${peso}" fill="${cor}" text-anchor="${ancora}">${esc(tr(s))}</text>`;
}

// quebra em linhas de até `max` caracteres (fonte monoespaçada)
function quebrar(s, max) {
  s = tr(s);
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
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${largura} ${altura}" width="${largura}" height="${altura}" role="img" lang="${LANG === 'en' ? 'en' : 'pt-BR'}" aria-labelledby="t d" font-family="${FONTE}">
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
  const ang = (k) => k * 30; // graus, no sentido horário a partir do topo
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
  s += texto(40, 44, 'Anel de hash circular: blocos e papéis', { tam: 20, peso: 700 });
  s += texto(40, 66, 'cluster de 4 nós físicos, cada um em 3 posições do anel (nós virtuais), fator de replicação 3', { tam: 12, cor: SUAVE });

  // anel
  s += `<circle cx="${cx}" cy="${cy}" r="${R}" fill="none" stroke="${LINHA}" stroke-width="3"/>`;
  s += texto(cx, cy - 6, 'sentido horário', { tam: 13, peso: 700, cor: SUAVE, ancora: 'middle' });
  s += texto(cx, cy + 14, '↻', { tam: 26, cor: SUAVE, ancora: 'middle' });

  // trechos percorridos por cada mensagem
  for (const m of MSGS) {
    const ks = Object.keys(m.papeis).map(Number);
    s += arco(ang(m.gap) - 15, ang(Math.max(...ks)), m.cor);
  }

  // nós virtuais
  ANEL.forEach((nome, k) => {
    const [x, y] = pt(ang(k), R);
    const { cor, tom } = NOS[nome[0]];
    const pulado = MSGS.some((m) => m.papeis[k] === 'pulado');
    s += `<circle cx="${n(x)}" cy="${n(y)}" r="${r}" fill="${tom}" stroke="${cor}" stroke-width="2.5"${pulado ? ' stroke-dasharray="4 3"' : ''}/>`;
    s += texto(x, y + 5, nome, { tam: 14, peso: 700, cor: TINTA, ancora: 'middle' });
    if (pulado) s += `<line x1="${n(x - 17)}" y1="${n(y + 17)}" x2="${n(x + 17)}" y2="${n(y - 17)}" stroke="${cor}" stroke-width="2.5"/>`;
  });

  // marcadores das mensagens (no intervalo antes do bloco) e selos de papel
  for (const m of MSGS) {
    const a = ang(m.gap) - 15;
    const [mx, my] = pt(a, R);
    s += `<path d="M${n(mx)} ${n(my - 9)} L${n(mx + 9)} ${n(my)} L${n(mx)} ${n(my + 9)} L${n(mx - 9)} ${n(my)} Z" fill="${m.cor}" stroke="${BG}" stroke-width="2"/>`;
    const [lx, ly] = pt(a, R - 42);
    s += texto(lx, ly - 2, m.id, { tam: 15, peso: 700, cor: m.cor, ancora: 'middle' });
    s += texto(lx, ly + 13, 'hash(ID)', { tam: 10, cor: SUAVE, ancora: 'middle' });

    for (const [k, papel] of Object.entries(m.papeis)) {
      const [bx, by] = pt(ang(Number(k)), R + 56);
      const rotulo = papel === 'pulado' ? `${m.id} ${tr('pulado')}` : `${m.id} ${papel}`;
      const w = papel === 'pulado' ? 92 : 56;
      const cheio = papel === 'P';
      const fundo = papel === 'pulado' ? '#1e293b' : cheio ? m.cor : BG;
      const tracoCor = papel === 'pulado' ? SUAVE : m.cor;
      const letra = papel === 'pulado' ? SUAVE : cheio ? BG : m.cor;
      s += `<rect x="${n(bx - w / 2)}" y="${n(by - 12)}" width="${w}" height="24" rx="12" fill="${fundo}" stroke="${tracoCor}" stroke-width="2"/>`;
      s += texto(bx, by + 4.5, rotulo, { tam: 12, peso: 700, cor: letra, ancora: 'middle' });
    }
  }

  // painel à direita
  const px = 700;
  s += `<line x1="${px - 24}" y1="96" x2="${px - 24}" y2="${H - 40}" stroke="${LINHA}" stroke-width="1.5"/>`;
  s += texto(px, 112, 'Como ler o anel', { tam: 15, peso: 700 });
  const passos = [
    'O ID da mensagem vira um hash: um ponto no anel (losango).',
    'Andando no sentido horário, o primeiro nó virtual é o primário (P): o dono da entrega.',
    'Os próximos nós físicos ainda não usados são o secundário (S) e o terciário (T), até completar o fator de replicação.',
    'Nó virtual de um nó físico já escolhido é pulado: as réplicas ficam sempre em máquinas distintas.',
    'Cada nó físico ocupa várias posições, então recebe blocos espalhados pelo anel, e não uma faixa contínua.',
  ];
  let y = 138;
  passos.forEach((p, i) => {
    s += `<circle cx="${px + 10}" cy="${y - 4}" r="10" fill="${TINTA}"/>`;
    s += texto(px + 10, y, String(i + 1), { tam: 12, peso: 700, cor: BG, ancora: 'middle' });
    const par = paragrafo(px + 32, y, p, 40, { tam: 12 });
    s += par.svg;
    y = par.fim + 12;
  });

  // resultado
  y += 6;
  s += texto(px, y, 'Resultado dos dois exemplos', { tam: 15, peso: 700 });
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
  const nota = paragrafo(px, y, 'Blocos diferentes têm primários diferentes: m1 tem A como dono; m2 tem B.', 44, { tam: 12, cor: SUAVE });
  s += nota.svg;

  // legenda dos nós
  const ly = H - 34;
  s += texto(40, ly - 4, 'Nós físicos', { tam: 12, peso: 700, cor: SUAVE });
  Object.entries(NOS).forEach(([nome, c], i) => {
    const x = 150 + i * 70;
    s += `<circle cx="${x}" cy="${ly - 8}" r="11" fill="${c.tom}" stroke="${c.cor}" stroke-width="2"/>`;
    s += texto(x, ly - 3.5, nome, { tam: 12, peso: 700, ancora: 'middle' });
  });
  s += texto(450, ly - 4, 'A1, A2, A3: nós virtuais do nó físico A', { tam: 12, cor: SUAVE });

  return envolver(W, H, 'Anel de hash circular: blocos e papéis',
    'Anel com 12 posições de 4 nós físicos. Duas mensagens caem em pontos do anel e, andando no sentido horário, o primeiro nó virtual é o primário, seguido do secundário e do terciário, pulando nós virtuais de um nó físico já escolhido.', s);
}

// ---------- diagrama B: o tópico espalhado ----------
function topico() {
  const W = 1080, H = 640;
  const nos = Object.keys(NOS);
  const x0 = 200, larg = 118, y0 = 150, alt = 54, cw = 92, ch = 38;

  let s = '';
  s += texto(40, 44, 'Um tópico espalhado e redundante', { tam: 20, peso: 700 });
  s += texto(40, 66, 'tópico `pedidos`: 6 mensagens em 4 nós, fator de replicação 3 (m1 e m2 são as do diagrama do anel)', { tam: 12, cor: SUAVE });

  // cabeçalho das colunas
  s += texto(60, y0 - 26, 'mensagem', { tam: 12, peso: 700, cor: SUAVE });
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

  // totais por nó
  const yt = y0 + TOPICO.length * alt + 6;
  s += `<line x1="40" y1="${yt}" x2="${x0 + nos.length * larg + 10}" y2="${yt}" stroke="${LINHA}" stroke-width="1.5"/>`;
  s += texto(60, yt + 24, 'cada nó guarda', { tam: 12, peso: 700, cor: SUAVE });
  nos.forEach((no, j) => {
    const t = totais[no];
    const cx = x0 + j * larg + larg / 2;
    s += texto(cx, yt + 22, `${t.P + t.S + t.T} ${tr('de')} ${TOPICO.length}`, { tam: 14, peso: 700, ancora: 'middle' });
    s += texto(cx, yt + 40, `${t.P}P ${t.S}S ${t.T}T`, { tam: 11, cor: SUAVE, ancora: 'middle' });
  });

  // legenda
  const yl = yt + 78;
  const leg = [
    ['P', 'primário (dono)', 'cheio', 60],
    ['S', 'secundário', 'meio', 240],
    ['T', 'terciário', 'vazio', 380],
  ];
  leg.forEach(([l, d, tipo, x]) => {
    s += `<rect x="${x}" y="${yl - 14}" width="30" height="22" rx="6" fill="${tipo === 'cheio' ? '#94a3b8' : tipo === 'meio' ? '#1e293b' : BG}" stroke="#94a3b8" stroke-width="2"/>`;
    s += texto(x + 15, yl + 2, l, { tam: 12, peso: 700, cor: tipo === 'cheio' ? BG : TINTA, ancora: 'middle' });
    s += texto(x + 40, yl + 2, d, { tam: 11, cor: SUAVE });
  });
  s += `<rect x="510" y="${yl - 14}" width="30" height="22" rx="6" fill="none" stroke="${LINHA}" stroke-dasharray="3 4"/>`;
  s += texto(550, yl + 2, 'não tem a mensagem', { tam: 11, cor: SUAVE });

  // cartões à direita
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
  const c1 = cartao(112, 'Espalhado', '#22d3ee', [
    'O ID decide o bloco, não o tópico: as mensagens caem em todo o anel.',
    'Nenhum nó guarda tudo, nem é dono do tópico. Cada um tem 4 ou 5 das 6.',
    'Os primários se revezam entre A, B, C e D.',
  ]);
  const c2 = cartao(c1.fim + 16, 'Redundante', '#34d399', [
    'Cada mensagem existe em 3 nós físicos distintos.',
    'Com quórum de 2 de 3, a escrita continua com uma réplica fora do ar.',
  ]);
  const c3 = cartao(c2.fim + 16, 'Se o nó B cair', '#fb7185', [
    'm1, m2, m5 e m6 continuam em 2 nós; nenhuma se perde.',
    'Os blocos de B passam ao próximo nó do anel e as réplicas são refeitas pela sincronização (seção 7.3).',
  ]);
  s += c1.g + c2.g + c3.g;

  return envolver(W, Math.max(H, c3.fim + 30), 'Um tópico espalhado e redundante',
    'Matriz com as mensagens m1 a m6 do tópico pedidos nas linhas e os nós A, B, C e D nas colunas. Cada mensagem tem um primário e duas réplicas em nós distintos, e nenhum nó guarda todas as mensagens.', s);
}

// ---------- diagrama C: os grupos de uma fila FIFO ----------
// fila FIFO `pedidos-fifo`: cada grupo tem uma posição no anel (hash do id do tópico + groupId),
// logo um só bloco; as mensagens do grupo (na ordem de publicação) vivem todas nesse bloco
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
  s += texto(40, 44, 'Os grupos de uma fila FIFO no anel', { tam: 20, peso: 700 });
  s += texto(40, 66, 'fila `pedidos-fifo`: 4 grupos em 4 nós, fator de replicação 3; o grupo decide o bloco, e não o ID da mensagem', { tam: 12, cor: SUAVE });

  s += texto(60, y0 - 26, 'grupo', { tam: 12, peso: 700, cor: SUAVE });
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
  s += texto(60, yt + 24, 'cada nó é primário de', { tam: 12, peso: 700, cor: SUAVE });
  nos.forEach((no, j) => {
    const t = totais[no];
    const cx = x0 + j * larg + larg / 2;
    s += texto(cx, yt + 22, `${t.P} grupo${t.P === 1 ? '' : 's'}`, { tam: 14, peso: 700, ancora: 'middle' });
    s += texto(cx, yt + 40, `${t.P}P ${t.S}S ${t.T}T`, { tam: 11, cor: SUAVE, ancora: 'middle' });
  });

  const yl = yt + 78;
  const leg = [
    ['P', 'primário: ordena e entrega', 'cheio', 60],
    ['S', 'secundário: no quórum', 'meio', 290],
    ['T', 'terciário', 'vazio', 510],
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
  const c1 = cartao(112, 'Um grupo, um bloco', '#22d3ee', [
    'O hash do id do tópico mais o groupId decide o bloco: todas as mensagens do grupo ficam nos mesmos 3 nós.',
    'O primário gera os IDs em ordem crescente e entrega uma mensagem do grupo por vez.',
  ]);
  const c2 = cartao(c1.fim + 16, 'Espalhado entre grupos', '#34d399', [
    'Grupos diferentes caem em blocos diferentes: os primários se revezam.',
    'Um grupo grande não passa de um primário (grupo quente).',
  ]);
  const c3 = cartao(c2.fim + 16, 'Se o primário de g1 cair', '#fb7185', [
    'O secundário C assume g1 já em dia, pois o quórum sempre o incluiu.',
    'A ordem de g1 e a deduplicação seguem sem perda.',
  ]);
  s += c1.g + c2.g + c3.g;

  return envolver(W, Math.max(H, c3.fim + 30), 'Os grupos de uma fila FIFO no anel',
    'Matriz com os grupos g1 a g4 da fila pedidos-fifo nas linhas e os nós A, B, C e D nas colunas. Cada grupo tem um primário, um secundário e um terciário, e todas as mensagens de um grupo ficam nos mesmos nós.', s);
}

writeFileSync(saida(`anel-hash${SUF}.svg`), anel());
writeFileSync(saida(`topico-espalhado${SUF}.svg`), topico());
// o diagrama FIFO só existe em português (não está nos README da raiz)
if (LANG === 'pt') writeFileSync(saida('fifo-grupos.svg'), fifo());
console.log('gerados: doc/diagramas/img/anel-hash.svg, doc/diagramas/img/topico-espalhado.svg, doc/diagramas/img/fifo-grupos.svg');
