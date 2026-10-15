// Re-spaces the messages of an Archify sequence diagram.
// Usage: node doc/diagramas/scripts/reespacar.mjs <fonte.json> [...]
//
// The ORDER of the messages in the "messages" array is the diagram's order; the "y" is recalculated
// here (fixed step). To insert a message, put it in the right position of the array with
// any "y"; if there are "segments", use a y equal to a neighboring message's in the phase
// it belongs to. The script also rebuilds the "segments" bands and the "meta.viewBox".
import { readFileSync, writeFileSync } from 'node:fs';

const PASSO = 28;        // Archify's minimum: 28 px between messages that share space
const FOLGA_FASE = 8;    // extra breathing room between phases (segments)
const TOPO = 172;        // first y (Archify's minimum: 160)
const LARGURA = 1080;    // viewBox width (minimum screen ratio: ~1.55)

for (const arquivo of process.argv.slice(2)) {
  const j = JSON.parse(readFileSync(arquivo, 'utf8'));
  if (j.diagram_type !== 'sequence') { console.log(`${arquivo}: skipped (not a sequence)`); continue; }

  const segs = j.segments || [];
  let fase = 0;
  const fases = j.messages.map((m) => {
    const i = segs.findIndex((s) => m.y >= s.from && m.y <= s.to);
    if (i >= 0) fase = i;
    return fase;
  });

  j.messages.forEach((m, i) => { m.y = TOPO + i * PASSO + fases[i] * FOLGA_FASE; });
  segs.forEach((s, k) => {
    const ys = j.messages.filter((_, i) => fases[i] === k).map((m) => m.y);
    if (ys.length) { s.from = Math.min(...ys) - 14; s.to = Math.max(...ys) + 14; }
  });

  const ultimo = j.messages[j.messages.length - 1].y;
  const altura = Math.max(480, ultimo + (segs.length ? 14 + 130 : 120));
  j.meta.viewBox = [LARGURA, altura];
  writeFileSync(arquivo, JSON.stringify(j, null, 2) + '\n');

  const aviso = altura > LARGURA / 1.55
    ? '  WARNING: diagram too tall for Archify\'s minimum ratio; split it in two diagrams.'
    : '';
  console.log(`${arquivo}: ${j.messages.length} messages, viewBox ${LARGURA}x${altura}${aviso}`);
}
