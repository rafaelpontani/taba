// Gráficos em SVG do benchmark, sem dependências (Node 22). Usado pelo relatório local (bench/report.mjs) e pelos gráficos
// da documentação (doc/diagrams/scripts/generate-bench.mjs). Cada SVG traz os dois temas: claro por padrão e escuro com
// prefers-color-scheme, que também vale num <img> do GitHub.
// A cor segue o sistema, nunca a posição: SYSTEM_SLOTS fixa a cor de cada um (paleta categórica validada para daltonismo,
// os três primeiros também em todos os pares). Valores e rótulos ficam na cor do texto; a cor só marca a série.

// Ordem categórica fixa (claro, escuro).
const SLOTS = [
  ['#2a78d6', '#3987e5'], // azul
  ['#eb6834', '#d95926'], // laranja
  ['#1baf7a', '#199e70'], // água
  ['#eda100', '#c98500'], // amarelo
  ['#e87ba4', '#d55181'], // magenta
];
export const SYSTEM_SLOTS = { taba: 0, rabbitmq: 1, nats: 2, kafka: 3, redpanda: 4 };
export const SYSTEM_NAMES = { taba: 'Taba', rabbitmq: 'RabbitMQ', nats: 'NATS JetStream', kafka: 'Kafka', redpanda: 'Redpanda' };

const W = 760;
const FONT = "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif";

export const esc = (t) => String(t).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

// 12345 -> "12.3k"; 0.5 -> "0.5". `unit` só entra no texto de ajuda.
export function short(v) {
  if (!Number.isFinite(v)) return 'n/d';
  const a = Math.abs(v);
  if (a >= 1e6) return `${+(v / 1e6).toFixed(a >= 1e7 ? 0 : 1)}M`;
  if (a >= 1e3) return `${+(v / 1e3).toFixed(a >= 1e4 ? 0 : 1)}k`;
  if (a >= 10) return String(Math.round(v));
  return String(+v.toFixed(1));
}

// Um teto "redondo" para o eixo e o passo das 4 linhas de grade.
function niceMax(max) {
  if (!(max > 0)) return 1;
  const raw = max / 4;
  const pow = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * pow).find((s) => s >= raw);
  return step * 4;
}

function style(slots) {
  const light = slots.map((s, i) => `--s${i}:${SLOTS[s][0]}`).join(';');
  const dark = slots.map((s, i) => `--s${i}:${SLOTS[s][1]}`).join(';');
  return `<style>
svg{--bg:#fcfcfb;--ink:#0b0b0b;--ink2:#52514e;--muted:#898781;--grid:#e1e0d9;--axis:#c3c2b7;${light}}
@media (prefers-color-scheme: dark){svg{--bg:#1a1a19;--ink:#f0efec;--ink2:#c3c2b7;--muted:#898781;--grid:#2c2c2a;--axis:#383835;${dark}}}
text{font-family:${FONT};fill:var(--ink2);font-size:12px}
.t{fill:var(--ink);font-size:16px;font-weight:600}.st{font-size:12.5px}.v{fill:var(--ink);font-size:11.5px;font-weight:600}
.lg{fill:var(--ink);font-size:12.5px}.ax{fill:var(--muted);font-size:11px}.mk{fill:var(--ink2);font-size:11px}
</style>`;
}

// Cabeçalho comum: título, subtítulo e legenda numa linha. Devolve [partes, y onde o gráfico começa].
function header({ title, subtitle, legend }) {
  const p = [`<text class="t" x="20" y="28">${esc(title)}</text>`];
  let y = 28;
  if (subtitle) { y += 20; p.push(`<text class="st" x="20" y="${y}">${esc(subtitle)}</text>`); }
  if (legend?.length) {
    y += 24;
    let x = 20;
    for (const item of legend) {
      const swatch = item.dash
        ? `<line x1="${x}" y1="${y - 4}" x2="${x + 18}" y2="${y - 4}" stroke="${item.color}" stroke-width="2" stroke-dasharray="4 3"/>`
        : item.line
          ? `<line x1="${x}" y1="${y - 4}" x2="${x + 18}" y2="${y - 4}" stroke="${item.color}" stroke-width="2.5"/>`
          : `<rect x="${x}" y="${y - 10}" width="12" height="12" rx="3" fill="${item.color}"/>`;
      const sw = item.dash || item.line ? 24 : 18;
      p.push(swatch, `<text class="lg" x="${x + sw}" y="${y}">${esc(item.label)}</text>`);
      x += sw + item.label.length * 7 + 22;
    }
  }
  return [p, y + 18];
}

function frame(height, slots, body, { title, desc }) {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${height}" width="${W}" height="${height}" role="img" aria-labelledby="t d">
<title id="t">${esc(title)}</title><desc id="d">${esc(desc ?? title)}</desc>
${style(slots)}
<rect width="${W}" height="${height}" rx="10" fill="var(--bg)"/>
${body.join('\n')}
</svg>
`;
}

// Barras agrupadas. series: [{ key (sistema), label, values: [...] }], uma por sistema; categories: rótulos dos grupos.
// O valor fica escrito em cima de cada barra.
export function barChart({ title, subtitle, unit, categories, series, note }) {
  const slots = series.map((s) => SYSTEM_SLOTS[s.key] ?? 4);
  const legend = series.map((s, i) => ({ label: s.label, color: `var(--s${i})` }));
  const [head, top] = header({ title, subtitle, legend });
  const height = top + 250 + (note ? 18 : 0);
  const left = 64, right = 20, plotH = 200, bottom = top + plotH;
  const plotW = W - left - right;
  const max = niceMax(Math.max(0, ...series.flatMap((s) => s.values.filter(Number.isFinite))));
  const groupW = plotW / Math.max(1, categories.length);
  const barW = Math.min(56, (groupW * 0.72) / Math.max(1, series.length));
  const p = [...head];
  for (let i = 0; i <= 4; i++) {
    const y = bottom - (plotH * i) / 4;
    p.push(`<line x1="${left}" y1="${y}" x2="${W - right}" y2="${y}" stroke="var(${i ? '--grid' : '--axis'})" stroke-width="1"/>`);
    p.push(`<text class="ax" x="${left - 8}" y="${y + 4}" text-anchor="end">${short((max * i) / 4)}</text>`);
  }
  p.push(`<text class="ax" x="${left - 8}" y="${top - 6}" text-anchor="end">${esc(unit)}</text>`);
  categories.forEach((cat, ci) => {
    const gx = left + ci * groupW + (groupW - barW * series.length) / 2;
    series.forEach((s, si) => {
      const v = s.values[ci];
      const x = gx + si * barW + 1;
      if (!Number.isFinite(v)) {
        p.push(`<text class="ax" x="${x + (barW - 2) / 2}" y="${bottom - 6}" text-anchor="middle">n/d</text>`);
        return;
      }
      const h = Math.max(1, (plotH * v) / max);
      const r = Math.min(4, h / 2, (barW - 2) / 2);
      // Topo arredondado, base reta na linha de base.
      const w = barW - 2, y = bottom - h;
      p.push(`<path d="M${x} ${bottom}V${y + r}Q${x} ${y} ${x + r} ${y}H${x + w - r}Q${x + w} ${y} ${x + w} ${y + r}V${bottom}Z" fill="var(--s${si})"><title>${esc(s.label)} · ${esc(cat)}: ${short(v)} ${esc(unit)}</title></path>`);
      p.push(`<text class="v" x="${x + w / 2}" y="${y - 5}" text-anchor="middle">${short(v)}</text>`);
    });
    p.push(`<text class="lg" x="${left + ci * groupW + groupW / 2}" y="${bottom + 20}" text-anchor="middle">${esc(cat)}</text>`);
  });
  if (note) p.push(`<text class="ax" x="20" y="${height - 12}">${esc(note)}</text>`);
  return frame(height, slots, p, { title, desc: subtitle });
}

// Linhas no tempo. series: [{ key, label, values: [...], step (s por ponto), start (s do primeiro ponto, padrão step),
// ou times: [...] (o instante de cada ponto), dash }]. markers: [{ at (s), label }] viram linhas verticais (a queda de um nó, por exemplo).
// Séries com o mesmo `key` têm a mesma cor (publicado tracejado, consumido cheio).
export function lineChart({ title, subtitle, unit, xLabel, series, markers = [], legend, note, endLabels = true }) {
  const keys = [...new Set(series.map((s) => s.key))];
  const slots = keys.map((k) => SYSTEM_SLOTS[k] ?? 4);
  const color = (s) => `var(--s${keys.indexOf(s.key)})`;
  const lg = legend ?? keys.map((k) => ({ label: series.find((s) => s.key === k).label, color: `var(--s${keys.indexOf(k)})`, line: true }));
  const [head, top] = header({ title, subtitle, legend: lg.map((l) => ({ ...l, color: l.color ?? (l.key ? `var(--s${keys.indexOf(l.key)})` : 'var(--ink2)') })) });
  const height = top + 262 + (note ? 18 : 0);
  const left = 64, right = endLabels ? 96 : 24, plotH = 200, bottom = top + plotH;
  const plotW = W - left - right;
  const pts = (s) => s.values.map((v, i) => [s.times ? s.times[i] : (s.start ?? s.step) + i * s.step, v]);
  const tMax = Math.max(1, ...series.flatMap((s) => pts(s).map(([t]) => t)), ...markers.map((m) => m.at));
  const max = niceMax(Math.max(0, ...series.flatMap((s) => s.values.filter(Number.isFinite))));
  const X = (t) => left + (plotW * t) / tMax;
  const Y = (v) => bottom - (plotH * v) / max;
  const p = [...head];
  for (let i = 0; i <= 4; i++) {
    const y = bottom - (plotH * i) / 4;
    p.push(`<line x1="${left}" y1="${y}" x2="${left + plotW}" y2="${y}" stroke="var(${i ? '--grid' : '--axis'})" stroke-width="1"/>`);
    p.push(`<text class="ax" x="${left - 8}" y="${y + 4}" text-anchor="end">${short((max * i) / 4)}</text>`);
  }
  p.push(`<text class="ax" x="${left - 8}" y="${top - 6}" text-anchor="end">${esc(unit)}</text>`);
  const tickStep = [10, 20, 30, 60, 120, 300].find((s) => tMax / s <= 8) ?? 600;
  for (let t = 0; t <= tMax + 0.1; t += tickStep) {
    p.push(`<text class="ax" x="${X(t)}" y="${bottom + 16}" text-anchor="middle">${t}</text>`);
  }
  p.push(`<text class="ax" x="${left + plotW / 2}" y="${bottom + 34}" text-anchor="middle">${esc(xLabel)}</text>`);
  for (const m of markers) {
    p.push(`<line x1="${X(m.at)}" y1="${top}" x2="${X(m.at)}" y2="${bottom}" stroke="var(--ink2)" stroke-width="1" stroke-dasharray="2 3"/>`);
    p.push(`<text class="mk" x="${X(m.at) + 4}" y="${top + 10}">${esc(m.label)}</text>`);
  }
  // Rótulos no fim das linhas cheias, afastados para não se sobreporem.
  const ends = [];
  for (const s of series) {
    const valid = pts(s).filter(([, v]) => Number.isFinite(v));
    if (valid.length < 1) continue;
    const d = valid.map(([t, v], i) => `${i ? 'L' : 'M'}${X(t).toFixed(1)} ${Y(v).toFixed(1)}`).join('');
    p.push(`<path d="${d}" fill="none" stroke="${color(s)}" stroke-width="${s.dash ? 1.5 : 2}" stroke-linejoin="round" stroke-linecap="round"${s.dash ? ' stroke-dasharray="5 4" opacity="0.85"' : ''}><title>${esc(s.label)}</title></path>`);
    if (!s.dash) {
      for (const [t, v] of valid) p.push(`<circle cx="${X(t).toFixed(1)}" cy="${Y(v).toFixed(1)}" r="7" fill="transparent"><title>${esc(s.label)} · ${t} s: ${short(v)} ${esc(unit)}</title></circle>`);
      const [lt, lv] = valid[valid.length - 1];
      ends.push({ x: X(lt), y: Y(lv), text: `${s.endLabel ?? s.label} ${short(lv)}` });
    }
  }
  if (endLabels) {
    ends.sort((a, b) => a.y - b.y);
    for (let i = 1; i < ends.length; i++) if (ends[i].y - ends[i - 1].y < 14) ends[i].y = ends[i - 1].y + 14;
    for (const e of ends) p.push(`<text class="v" x="${left + plotW + 6}" y="${e.y + 4}">${esc(e.text)}</text>`);
  }
  if (note) p.push(`<text class="ax" x="20" y="${height - 12}">${esc(note)}</text>`);
  return frame(height, slots, p, { title, desc: subtitle });
}

// Soma acumulada de uma série de taxas (msg/s a cada `step` s): quantas mensagens até cada ponto.
export const cumulative = (rates, step) => {
  let total = 0;
  return rates.map((r) => (total += (Number.isFinite(r) ? r : 0) * step));
};
