// Reespaça as mensagens de um diagrama de sequência do Archify.
// Uso: node doc/diagramas/scripts/reespacar.mjs <fonte.json> [...]
//
// A ORDEM das mensagens no array "messages" é a ordem do diagrama; o "y" é recalculado
// aqui (passo fixo). Para inserir uma mensagem, coloque-a na posição certa do array com
// qualquer "y"; se houver "segments", use um y igual ao de uma mensagem vizinha da fase
// à qual ela pertence. O script também refaz as faixas de "segments" e o "meta.viewBox".
import { readFileSync, writeFileSync } from 'node:fs';

const PASSO = 28;        // mínimo do Archify: 28 px entre mensagens que dividem espaço
const FOLGA_FASE = 8;    // respiro extra entre fases (segments)
const TOPO = 172;        // primeiro y (mínimo do Archify: 160)
const LARGURA = 1080;    // largura do viewBox (proporção mínima de tela: ~1,55)

for (const arquivo of process.argv.slice(2)) {
  const j = JSON.parse(readFileSync(arquivo, 'utf8'));
  if (j.diagram_type !== 'sequence') { console.log(`${arquivo}: ignorado (não é sequence)`); continue; }

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
    ? '  AVISO: diagrama alto demais para a proporção mínima do Archify; divida em dois diagramas.'
    : '';
  console.log(`${arquivo}: ${j.messages.length} mensagens, viewBox ${LARGURA}x${altura}${aviso}`);
}
