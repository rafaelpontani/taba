#!/usr/bin/env node
// Generates by hand the three circular hash diagrams (they do not go through Archify, which does not draw rings
// or matrices): doc/diagrams/img/<lang>/ring-hash.svg, spread-topic.svg and fifo-groups.svg.
// This is the source: to change the drawing, edit the constants below and run
//   node doc/diagrams/scripts/generate-hash.mjs
import { writeFileSync, mkdirSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
// ---------- language ----------
// English (en-US) is the source language. --lang=<code> uses doc/diagrams/i18n/<code>.json (flat table
// "English string": "translation"; a missing string stays in English) and writes to img/<code>/.
// Without --lang, every language is generated.
const arg = process.argv.find((a) => a.startsWith('--lang='));
const LANGUAGES = arg ? [arg.slice(7)] : ['en-US', ...readdirSync(resolve(root, 'doc/diagrams/i18n')).filter((f) => f.endsWith('.json')).map((f) => f.slice(0, -5))];
let LANG = 'en-US';
let TABLE = {};
const tr = (t) => TABLE[t] ?? t;
const output = (name) => resolve(root, 'doc/diagrams/img', LANG, name);


// ---------- data ----------
const FONT = "'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Consolas, 'DejaVu Sans Mono', monospace";
// dark theme, same color family as the Archify diagrams
const BG = '#020617', PANEL = '#0b1220', INK = '#f8fafc', MUTED = '#94a3b8', LINE = '#334155';

// physical nodes: outline and fill color
const NODES = {
  A: { color: '#22d3ee', tone: '#0b2a35' },
  B: { color: '#a78bfa', tone: '#241a4a' },
  C: { color: '#fbbf24', tone: '#33260a' },
  D: { color: '#fb7185', tone: '#3a121e' },
};

// ring positions clockwise, from the top: each physical node appears in 3 positions (virtual nodes)
const RING = ['A1', 'C1', 'B1', 'D1', 'A2', 'A3', 'C2', 'B2', 'D2', 'B3', 'C3', 'D3'];

// the example messages: ring position where the hash lands (the block starts at the first virtual node ahead)
// roles by ring position; "skipped" = virtual node of an already chosen physical node
const MSGS = [
  { id: 'm1', color: '#34d399', gap: 4, roles: { 4: 'P', 5: 'skipped', 6: 'S', 7: 'T' } },
  { id: 'm2', color: '#60a5fa', gap: 9, roles: { 9: 'P', 10: 'S', 11: 'T' } },
];

// the `orders` topic matrix (m1 and m2 equal to the ring's): each node's role in each message
const TOPIC = [
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

function text(x, y, s, { size = 12, weight = 400, color = INK, anchor = 'start' } = {}) {
  return `<text x="${n(x)}" y="${n(y)}" font-size="${size}" font-weight="${weight}" fill="${color}" text-anchor="${anchor}">${esc(tr(s))}</text>`;
}

// breaks into lines of up to `max` characters (monospaced font)
function wrap(s, max) {
  const lines = [];
  let current = '';
  for (const p of s.split(' ')) {
    if ((current + ' ' + p).trim().length > max) { lines.push(current); current = p; }
    else current = (current + ' ' + p).trim();
  }
  if (current) lines.push(current);
  return lines;
}

function paragraph(x, y, s, max, { size = 12, color = INK, step = 17 } = {}) {
  const lines = wrap(s, max);
  return { svg: lines.map((l, i) => text(x, y + i * step, l, { size, color })).join(''), end: y + lines.length * step };
}

function frame(width, height, title, description, body) {
  return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}" width="${width}" height="${height}" role="img" lang="${LANG}" aria-labelledby="t d" font-family="${FONT}">
<title id="t">${esc(tr(title))}</title>
<desc id="d">${esc(tr(description))}</desc>
<rect width="${width}" height="${height}" fill="${BG}"/>
${body}
</svg>
`;
}

// ---------- diagram A: the ring ----------
function ringDiagram() {
  const W = 1080, H = 700, cx = 350, cy = 336, R = 200, r = 24;
  const angle = (k) => k * 30; // degrees, clockwise from the top
  const pt = (degrees, radius) => {
    const t = (degrees * Math.PI) / 180;
    return [cx + radius * Math.sin(t), cy - radius * Math.cos(t)];
  };
  const arc = (from, to, color) => {
    const [x1, y1] = pt(from, R), [x2, y2] = pt(to, R);
    const large = to - from > 180 ? 1 : 0;
    return `<path d="M${n(x1)} ${n(y1)} A${R} ${R} 0 ${large} 1 ${n(x2)} ${n(y2)}" fill="none" stroke="${color}" stroke-width="9" stroke-linecap="round" opacity="0.4"/>`;
  };

  let s = '';
  s += text(40, 44, 'Circular hash ring: blocks and roles', { size: 20, weight: 700 });
  s += text(40, 66, '4 physical nodes, each at 3 ring positions (virtual nodes), replication factor 3', { size: 12, color: MUTED });

  // ring
  s += `<circle cx="${cx}" cy="${cy}" r="${R}" fill="none" stroke="${LINE}" stroke-width="3"/>`;
  s += text(cx, cy - 6, 'clockwise', { size: 13, weight: 700, color: MUTED, anchor: 'middle' });
  s += text(cx, cy + 14, '↻', { size: 26, color: MUTED, anchor: 'middle' });

  // stretches walked by each message
  for (const m of MSGS) {
    const ks = Object.keys(m.roles).map(Number);
    s += arc(angle(m.gap) - 15, angle(Math.max(...ks)), m.color);
  }

  // virtual nodes
  RING.forEach((name, k) => {
    const [x, y] = pt(angle(k), R);
    const { color, tone } = NODES[name[0]];
    const skipped = MSGS.some((m) => m.roles[k] === 'skipped');
    s += `<circle cx="${n(x)}" cy="${n(y)}" r="${r}" fill="${tone}" stroke="${color}" stroke-width="2.5"${skipped ? ' stroke-dasharray="4 3"' : ''}/>`;
    s += text(x, y + 5, name, { size: 14, weight: 700, color: INK, anchor: 'middle' });
    if (skipped) s += `<line x1="${n(x - 17)}" y1="${n(y + 17)}" x2="${n(x + 17)}" y2="${n(y - 17)}" stroke="${color}" stroke-width="2.5"/>`;
  });

  // message markers (in the gap before the block) and role badges
  for (const m of MSGS) {
    const a = angle(m.gap) - 15;
    const [mx, my] = pt(a, R);
    s += `<path d="M${n(mx)} ${n(my - 9)} L${n(mx + 9)} ${n(my)} L${n(mx)} ${n(my + 9)} L${n(mx - 9)} ${n(my)} Z" fill="${m.color}" stroke="${BG}" stroke-width="2"/>`;
    const [lx, ly] = pt(a, R - 42);
    s += text(lx, ly - 2, m.id, { size: 15, weight: 700, color: m.color, anchor: 'middle' });
    s += text(lx, ly + 13, 'hash(ID)', { size: 10, color: MUTED, anchor: 'middle' });

    for (const [k, role] of Object.entries(m.roles)) {
      const [bx, by] = pt(angle(Number(k)), R + 56);
      const label = role === 'skipped' ? `${m.id} ${tr('skipped')}` : `${m.id} ${role}`;
      const w = role === 'skipped' ? 92 : 56;
      const filled = role === 'P';
      const fill = role === 'skipped' ? '#1e293b' : filled ? m.color : BG;
      const strokeColor = role === 'skipped' ? MUTED : m.color;
      const letter = role === 'skipped' ? MUTED : filled ? BG : m.color;
      s += `<rect x="${n(bx - w / 2)}" y="${n(by - 12)}" width="${w}" height="24" rx="12" fill="${fill}" stroke="${strokeColor}" stroke-width="2"/>`;
      s += text(bx, by + 4.5, label, { size: 12, weight: 700, color: letter, anchor: 'middle' });
    }
  }

  // right panel
  const px = 700;
  s += `<line x1="${px - 24}" y1="96" x2="${px - 24}" y2="${H - 40}" stroke="${LINE}" stroke-width="1.5"/>`;
  s += text(px, 112, 'How to read the ring', { size: 15, weight: 700 });
  const steps = [
    'The message ID becomes a hash: a point on the ring (diamond).',
    'Walking clockwise, the first virtual node is the primary (P): the owner of delivery.',
    'The next physical nodes not used yet are the secondary (S) and the tertiary (T), until the replication factor is met.',
    'A virtual node of an already chosen physical node is skipped: replicas always sit on different machines.',
    'Each physical node holds several positions, so it gets blocks spread around the ring, not one continuous stretch.',
  ];
  let y = 138;
  steps.forEach((p, i) => {
    s += `<circle cx="${px + 10}" cy="${y - 4}" r="10" fill="${INK}"/>`;
    s += text(px + 10, y, String(i + 1), { size: 12, weight: 700, color: BG, anchor: 'middle' });
    const para = paragraph(px + 32, y, p, 40, { size: 12 });
    s += para.svg;
    y = para.end + 12;
  });

  // result
  y += 6;
  s += text(px, y, 'Result of the two examples', { size: 15, weight: 700 });
  y += 14;
  const head = ['', 'P', 'S', 'T'];
  const colX = [px, px + 70, px + 160, px + 250];
  head.forEach((c, i) => { s += text(colX[i] + (i ? 26 : 0), y + 14, c, { size: 12, weight: 700, color: MUTED, anchor: i ? 'middle' : 'start' }); });
  y += 22;
  const owners = { m1: ['A', 'C', 'B'], m2: ['B', 'C', 'D'] };
  MSGS.forEach((m) => {
    s += `<rect x="${px - 6}" y="${y}" width="352" height="34" rx="8" fill="${PANEL}" stroke="${LINE}"/>`;
    s += text(px + 6, y + 22, m.id, { size: 13, weight: 700, color: m.color });
    owners[m.id].forEach((node, i) => {
      const cxn = colX[i + 1] + 26;
      s += `<circle cx="${cxn}" cy="${y + 17}" r="12" fill="${NODES[node].tone}" stroke="${NODES[node].color}" stroke-width="2"/>`;
      s += text(cxn, y + 21.5, node, { size: 13, weight: 700, anchor: 'middle' });
    });
    y += 42;
  });
  y += 14;
  const note = paragraph(px, y, 'Different blocks have different primaries: m1 is owned by A; m2 by B.', 44, { size: 12, color: MUTED });
  s += note.svg;

  // node legend
  const ly = H - 34;
  s += text(40, ly - 4, 'Physical nodes', { size: 12, weight: 700, color: MUTED });
  Object.entries(NODES).forEach(([name, c], i) => {
    const x = 150 + i * 70;
    s += `<circle cx="${x}" cy="${ly - 8}" r="11" fill="${c.tone}" stroke="${c.color}" stroke-width="2"/>`;
    s += text(x, ly - 3.5, name, { size: 12, weight: 700, anchor: 'middle' });
  });
  s += text(450, ly - 4, 'A1, A2, A3: virtual nodes of physical node A', { size: 12, color: MUTED });

  return frame(W, H, 'Circular hash ring: blocks and roles',
    'A ring with 12 positions of 4 physical nodes. Two messages land on points of the ring and, walking clockwise, the first virtual node is the primary, followed by the secondary and the tertiary, skipping virtual nodes of an already chosen physical node.', s);
}

// ---------- diagram B: the spread topic ----------
function spreadTopicDiagram() {
  const W = 1080, H = 640;
  const nodes = Object.keys(NODES);
  const x0 = 200, colWidth = 118, y0 = 150, rowHeight = 54, cw = 92, ch = 38;

  let s = '';
  s += text(40, 44, 'A spread, redundant topic', { size: 20, weight: 700 });
  s += text(40, 66, 'topic `orders`: 6 messages on 4 nodes, replication factor 3 (m1 and m2 are the ones in the ring diagram)', { size: 12, color: MUTED });

  // column headers
  s += text(60, y0 - 26, 'message', { size: 12, weight: 700, color: MUTED });
  nodes.forEach((node, j) => {
    const cx = x0 + j * colWidth + colWidth / 2;
    s += `<circle cx="${cx}" cy="${y0 - 34}" r="15" fill="${NODES[node].tone}" stroke="${NODES[node].color}" stroke-width="2.5"/>`;
    s += text(cx, y0 - 29, node, { size: 15, weight: 700, anchor: 'middle' });
  });

  const totals = Object.fromEntries(nodes.map((node) => [node, { P: 0, S: 0, T: 0 }]));

  TOPIC.forEach((row, i) => {
    const y = y0 + i * rowHeight;
    s += `<rect x="40" y="${y - 6}" width="${x0 - 40 + nodes.length * colWidth + 10}" height="${rowHeight - 6}" rx="10" fill="${i % 2 ? 'none' : PANEL}"/>`;
    s += text(60, y + 24, row.id, { size: 15, weight: 700 });
    nodes.forEach((node, j) => {
      const role = row[node];
      const cx = x0 + j * colWidth + (colWidth - cw) / 2;
      if (!role) {
        s += `<rect x="${cx}" y="${y}" width="${cw}" height="${ch}" rx="9" fill="none" stroke="${LINE}" stroke-dasharray="3 4"/>`;
        return;
      }
      totals[node][role]++;
      const color = NODES[node].color;
      const filled = role === 'P';
      s += `<rect x="${cx}" y="${y}" width="${cw}" height="${ch}" rx="9" fill="${filled ? color : role === 'S' ? NODES[node].tone : BG}" stroke="${color}" stroke-width="2.5"/>`;
      s += text(cx + cw / 2, y + 24, role, { size: 12, weight: 700, color: filled ? BG : INK, anchor: 'middle' });
    });
  });

  // totals per node
  const yt = y0 + TOPIC.length * rowHeight + 6;
  s += `<line x1="40" y1="${yt}" x2="${x0 + nodes.length * colWidth + 10}" y2="${yt}" stroke="${LINE}" stroke-width="1.5"/>`;
  s += text(60, yt + 24, 'each node stores', { size: 12, weight: 700, color: MUTED });
  nodes.forEach((node, j) => {
    const t = totals[node];
    const cx = x0 + j * colWidth + colWidth / 2;
    s += text(cx, yt + 22, `${t.P + t.S + t.T} ${tr('of')} ${TOPIC.length}`, { size: 14, weight: 700, anchor: 'middle' });
    s += text(cx, yt + 40, `${t.P}P ${t.S}S ${t.T}T`, { size: 11, color: MUTED, anchor: 'middle' });
  });

  // legend
  const yl = yt + 78;
  const legend = [
    ['P', 'primary (owner)', 'full', 60],
    ['S', 'secondary', 'half', 240],
    ['T', 'tertiary', 'empty', 380],
  ];
  legend.forEach(([l, d, kind, x]) => {
    s += `<rect x="${x}" y="${yl - 14}" width="30" height="22" rx="6" fill="${kind === 'full' ? '#94a3b8' : kind === 'half' ? '#1e293b' : BG}" stroke="#94a3b8" stroke-width="2"/>`;
    s += text(x + 15, yl + 2, l, { size: 12, weight: 700, color: kind === 'full' ? BG : INK, anchor: 'middle' });
    s += text(x + 40, yl + 2, d, { size: 11, color: MUTED });
  });
  s += `<rect x="510" y="${yl - 14}" width="30" height="22" rx="6" fill="none" stroke="${LINE}" stroke-dasharray="3 4"/>`;
  s += text(550, yl + 2, 'does not have the message', { size: 11, color: MUTED });

  // cards on the right
  const px = 770, cw2 = 270;
  const card = (y, title, color, items) => {
    let g = '';
    const lines = items.map((t) => wrap(t, 33));
    const height = 44 + lines.reduce((a, l) => a + l.length * 17 + 8, 0);
    g += `<rect x="${px}" y="${y}" width="${cw2}" height="${height}" rx="12" fill="${PANEL}" stroke="${LINE}"/>`;
    g += `<circle cx="${px + 20}" cy="${y + 24}" r="6" fill="${color}"/>`;
    g += text(px + 34, y + 29, title, { size: 14, weight: 700 });
    let yy = y + 52;
    lines.forEach((ls) => {
      ls.forEach((l, i) => { g += text(px + 20, yy + i * 17, (i === 0 ? '• ' : '  ') + l, { size: 12 }); });
      yy += ls.length * 17 + 8;
    });
    return { g, end: y + height };
  };
  const c1 = card(112, 'Spread', '#22d3ee', [
    'The ID decides the block, not the topic: messages land all around the ring.',
    'No node stores everything or owns the topic. Each one has 4 or 5 of the 6.',
    'The primaries take turns among A, B, C and D.',
  ]);
  const c2 = card(c1.end + 16, 'Redundant', '#34d399', [
    'Each message exists on 3 different physical nodes.',
    'With a quorum of 2 out of 3, writes continue with one replica down.',
  ]);
  const c3 = card(c2.end + 16, 'If node B goes down', '#fb7185', [
    'm1, m2, m5 and m6 stay on 2 nodes; none is lost.',
    'The blocks of B move to the next node on the ring and the replicas are rebuilt by synchronization.',
  ]);
  s += c1.g + c2.g + c3.g;

  return frame(W, Math.max(H, c3.end + 30), 'A spread, redundant topic',
    'A matrix with messages m1 to m6 of topic orders in the rows and nodes A, B, C and D in the columns. Each message has a primary and two replicas on different nodes, and no node stores all the messages.', s);
}

// ---------- diagram C: the groups of a FIFO queue ----------
// FIFO queue `orders-fifo`: each group has a position on the ring (hash of the topic id + groupId),
// so a single block; the group's messages (in publish order) all live in that block
const GROUPS = [
  { id: 'g1', msgs: ['m1', 'm3', 'm5'], A: 'P', C: 'S', B: 'T' },
  { id: 'g2', msgs: ['m2', 'm4'], B: 'P', C: 'S', D: 'T' },
  { id: 'g3', msgs: ['m6'], C: 'P', D: 'S', A: 'T' },
  { id: 'g4', msgs: ['m7', 'm8'], D: 'P', A: 'S', C: 'T' },
];

function fifoDiagram() {
  const W = 1080, H = 640;
  const nodes = Object.keys(NODES);
  const x0 = 200, colWidth = 118, y0 = 150, rowHeight = 54, cw = 92, ch = 38;

  let s = '';
  s += text(40, 44, 'The groups of a FIFO queue on the ring', { size: 20, weight: 700 });
  s += text(40, 66, 'queue `orders-fifo`: 4 groups on 4 nodes, replication factor 3; the group decides the block, not the message ID', { size: 12, color: MUTED });

  s += text(60, y0 - 26, 'group', { size: 12, weight: 700, color: MUTED });
  nodes.forEach((node, j) => {
    const cx = x0 + j * colWidth + colWidth / 2;
    s += `<circle cx="${cx}" cy="${y0 - 34}" r="15" fill="${NODES[node].tone}" stroke="${NODES[node].color}" stroke-width="2.5"/>`;
    s += text(cx, y0 - 29, node, { size: 15, weight: 700, anchor: 'middle' });
  });

  const totals = Object.fromEntries(nodes.map((node) => [node, { P: 0, S: 0, T: 0 }]));

  GROUPS.forEach((row, i) => {
    const y = y0 + i * rowHeight;
    s += `<rect x="40" y="${y - 6}" width="${x0 - 40 + nodes.length * colWidth + 10}" height="${rowHeight - 6}" rx="10" fill="${i % 2 ? 'none' : PANEL}"/>`;
    s += text(60, y + 18, row.id, { size: 15, weight: 700 });
    s += text(60, y + 34, row.msgs.join(' → '), { size: 11, color: MUTED });
    nodes.forEach((node, j) => {
      const role = row[node];
      const cx = x0 + j * colWidth + (colWidth - cw) / 2;
      if (!role) {
        s += `<rect x="${cx}" y="${y}" width="${cw}" height="${ch}" rx="9" fill="none" stroke="${LINE}" stroke-dasharray="3 4"/>`;
        return;
      }
      totals[node][role]++;
      const color = NODES[node].color;
      const filled = role === 'P';
      s += `<rect x="${cx}" y="${y}" width="${cw}" height="${ch}" rx="9" fill="${filled ? color : role === 'S' ? NODES[node].tone : BG}" stroke="${color}" stroke-width="2.5"/>`;
      s += text(cx + cw / 2, y + 24, role, { size: 12, weight: 700, color: filled ? BG : INK, anchor: 'middle' });
    });
  });

  const yt = y0 + GROUPS.length * rowHeight + 6;
  s += `<line x1="40" y1="${yt}" x2="${x0 + nodes.length * colWidth + 10}" y2="${yt}" stroke="${LINE}" stroke-width="1.5"/>`;
  s += text(60, yt + 24, 'each node is primary of', { size: 12, weight: 700, color: MUTED });
  nodes.forEach((node, j) => {
    const t = totals[node];
    const cx = x0 + j * colWidth + colWidth / 2;
    s += text(cx, yt + 22, `${t.P} ${tr(t.P === 1 ? 'group' : 'groups')}`, { size: 14, weight: 700, anchor: 'middle' });
    s += text(cx, yt + 40, `${t.P}P ${t.S}S ${t.T}T`, { size: 11, color: MUTED, anchor: 'middle' });
  });

  const yl = yt + 78;
  const legend = [
    ['P', 'primary: orders and delivers', 'full', 60],
    ['S', 'secondary: in the quorum', 'half', 290],
    ['T', 'tertiary', 'empty', 510],
  ];
  legend.forEach(([l, d, kind, x]) => {
    s += `<rect x="${x}" y="${yl - 14}" width="30" height="22" rx="6" fill="${kind === 'full' ? '#94a3b8' : kind === 'half' ? '#1e293b' : BG}" stroke="#94a3b8" stroke-width="2"/>`;
    s += text(x + 15, yl + 2, l, { size: 12, weight: 700, color: kind === 'full' ? BG : INK, anchor: 'middle' });
    s += text(x + 40, yl + 2, d, { size: 11, color: MUTED });
  });

  const px = 770, cw2 = 270;
  const card = (y, title, color, items) => {
    let g = '';
    const lines = items.map((t) => wrap(t, 33));
    const height = 44 + lines.reduce((a, l) => a + l.length * 17 + 8, 0);
    g += `<rect x="${px}" y="${y}" width="${cw2}" height="${height}" rx="12" fill="${PANEL}" stroke="${LINE}"/>`;
    g += `<circle cx="${px + 20}" cy="${y + 24}" r="6" fill="${color}"/>`;
    g += text(px + 34, y + 29, title, { size: 14, weight: 700 });
    let yy = y + 52;
    lines.forEach((ls) => {
      ls.forEach((l, i) => { g += text(px + 20, yy + i * 17, (i === 0 ? '• ' : '  ') + l, { size: 12 }); });
      yy += ls.length * 17 + 8;
    });
    return { g, end: y + height };
  };
  const c1 = card(112, 'One group, one block', '#22d3ee', [
    'The hash of the topic id plus the groupId decides the block: all messages of the group stay on the same 3 nodes.',
    'The primary generates the IDs in increasing order and delivers one message of the group at a time.',
  ]);
  const c2 = card(c1.end + 16, 'Spread across groups', '#34d399', [
    'Different groups land in different blocks: the primaries take turns.',
    'A big group never goes beyond one primary (hot group).',
  ]);
  const c3 = card(c2.end + 16, 'If the primary of g1 goes down', '#fb7185', [
    'Secondary C takes over g1 already up to date, since the quorum always included it.',
    'The order of g1 and the deduplication carry on without loss.',
  ]);
  s += c1.g + c2.g + c3.g;

  return frame(W, Math.max(H, c3.end + 30), 'The groups of a FIFO queue on the ring',
    'A matrix with groups g1 to g4 of queue orders-fifo in the rows and nodes A, B, C and D in the columns. Each group has a primary, a secondary and a tertiary, and all the messages of a group stay on the same nodes.', s);
}

for (const lang of LANGUAGES) {
  LANG = lang;
  TABLE = lang === 'en-US' ? {} : JSON.parse(readFileSync(resolve(root, 'doc/diagrams/i18n', lang + '.json'), 'utf8'));
  mkdirSync(resolve(root, 'doc/diagrams/img', lang), { recursive: true });
  writeFileSync(output('ring-hash.svg'), ringDiagram());
  writeFileSync(output('spread-topic.svg'), spreadTopicDiagram());
  writeFileSync(output('fifo-groups.svg'), fifoDiagram());
  console.log(`generated (${lang}): doc/diagrams/img/${lang}/ring-hash.svg, spread-topic.svg, fifo-groups.svg`);
}
