#!/usr/bin/env node
// Gera doc/pt-BR/30-decisoes.md e doc/pt-BR/31-pendencias.md a partir de doc/08a, 08b e 08c.
//
// As decisões e as pendências têm código estável (D…, PRB-…), e o texto histórico é a fonte: este
// script só reformata (tabelas, cartões) e troca os links para os documentos da pasta pt-BR.
//
// Uso (da raiz do repositório):  node doc/pt-BR/scripts/gerar-historico.mjs

import fs from 'node:fs';
import path from 'node:path';
import { execSync } from 'node:child_process';

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')), '../../..');
const read = (f) => fs.readFileSync(path.join(root, "doc", f), "utf8").split(String.fromCharCode(13)).join("");
const commit = execSync('git rev-parse --short HEAD', { cwd: root }).toString().trim();
const today = new Date().toISOString().slice(0, 10);

// ---- troca de links: documento histórico -> documento novo ----
const MAP = {
  '01-visao-geral.md': '01-visao-geral.md', '02-arquitetura.md': '04-arquitetura.md', '03-escopo.md': '01-visao-geral.md',
  '04-cluster.md': '07-cluster.md', '05-particionamento-replicacao.md': '06-anel-e-replicacao.md', '06-api.md': '14-seguranca.md',
  '06a-api-cliente.md': '16-api-cliente.md', '06b-api-operacao.md': '17-api-operacao.md', '06c-api-cluster.md': '18-api-cluster.md',
  '06d-api-convencoes.md': '15-api-visao-geral.md', '07-principios-design.md': '01-visao-geral.md',
  '08a-decisoes-tomadas.md': '30-decisoes.md', '08b-pendencias.md': '31-pendencias.md', '08c-proximos-passos.md': '32-roadmap.md',
  '09-glossario.md': '28-glossario.md', '10-configuracao.md': '22-configuracao.md', '11-observabilidade.md': '13-observabilidade.md',
  '12-limites-api.md': '19-erros-e-limites.md', '13-entrega-de-mensagens.md': '12-entrega.md', '14-invisibilidade.md': '12-entrega.md',
  '15-topicos-distribuidor.md': '09-topicos.md', '16-mapa-mental.md': '29-mapa-mental.md', '17-topicos-fifo.md': '09-topicos.md',
  '18-estrutura-codigo.md': '24-convencoes.md', '19-conectores.md': '21-conectores.md', '19a-conectores-linguagens.md': '21-conectores.md',
  '19b-cli.md': '20-cli.md', '20-casos-de-uso.md': '26-casos-de-uso.md', '20a-casos-de-uso-cliente.md': '26a-casos-de-uso-implementados.md',
  '20b-casos-de-uso-no-dados.md': '26a-casos-de-uso-implementados.md', '20c-casos-de-uso-no-cluster.md': '26a-casos-de-uso-implementados.md',
  '21-pipeline-ci-cd.md': '27-ci-cd.md',
};
const label = (f) => f.replace(/\.md$/, '');
// o 05 se divide por seção: 7.1 anel, 7.2 publicação, 7.3 sincronização
function target05(trailing) {
  if (/7\.3/.test(trailing)) return '07-cluster.md';
  if (/7\.2/.test(trailing)) return '10-publicacao.md';
  return '06-anel-e-replicacao.md';
}
// o 06 vira segurança (8.0) ou a visão das APIs
function target06(trailing) { return /8\.0/.test(trailing) ? '14-seguranca.md' : '15-api-visao-geral.md'; }
function target18(trailing) { return /17\.5|17\.7|17\.3|17\.4/.test(trailing) ? '24-convencoes.md' : '24-convencoes.md'; }

function relink(text) {
  // caminhos para arquivos do repositório (../x) ganham mais um nível: doc/pt-BR fica um nível abaixo de doc/
  text = text.replace(/\]\(\.\.\/(?!\.\.)/g, '](../../');
  const link = /\[([^\]]*)\]\(([^)#\s]+\.md)(#[^)]*)?\)/g;
  return text.replace(link, (all, lbl, file, anchor, offset, whole) => {
    const base = path.basename(file);
    if (!(base in MAP)) return all;
    const trailing = whole.slice(offset + all.length, offset + all.length + 40).split('|')[0].split('\n')[0];
    let to = MAP[base];
    if (base === '05-particionamento-replicacao.md') to = target05(trailing + lbl);
    if (base === '06-api.md') to = target06(trailing + lbl);
    const shown = lbl.endsWith('.md') ? to : lbl;
    return '[' + shown + '](' + to + ')';
  });
}

// ---------------- 30: decisões ----------------
{
  const text = read('08a-decisoes-tomadas.md');
  const lines = text.split('\n');
  let out = `---
título: Decisões
resumo: O registro das decisões já tomadas, por tema, com o código estável de cada dúvida (D…) e onde está descrita, e os problemas já resolvidos (PRB-…) — o "porquê" por trás do que os outros documentos descrevem.
tipo: referência
status: estável
verificado-em: ${today} @ ${commit}
fontes: [doc/08a-decisoes-tomadas.md]
---

# Decisões

> **Como ler.** A documentação principal descreve **o resultado**; aqui está **o porquê** (no formato de um registro de decisões, ADR). Cada linha tem o **código da dúvida** (para citar: "D13"), a **decisão** e **onde ela está descrita** nos documentos desta pasta. Os códigos **nunca são reaproveitados**. Uma decisão **provisória** é uma que a implementação tomou e que ainda aguarda confirmação: ela fica aqui **e** a pergunta continua em [31-pendencias.md](31-pendencias.md).
>
> **Gerado.** Este arquivo é produzido por \`node doc/pt-BR/scripts/gerar-historico.mjs\` a partir de \`doc/08a-decisoes-tomadas.md\` (a fonte do registro); só os links e o formato mudam. **Regra do projeto:** ao fechar uma pendência, registre a decisão no documento técnico, o valor padrão em [22-configuracao.md](22-configuracao.md), a linha aqui, e **apague** a entrada de [31-pendencias.md](31-pendencias.md) e a referência em [32-roadmap.md](32-roadmap.md).

`;
  let count = 0;
  for (const raw of lines) {
    if (/^# /.test(raw) || /^## 10\.1 /.test(raw)) continue;
    if (/^## 10\.1\.1/.test(raw)) { out += '\n## Problemas resolvidos (PRB)\n'; continue; }
    if (/^### /.test(raw)) { out += `\n${raw.replace(/^### /, '## ')}\n`; continue; }
    if (/^\[← Índice\]/.test(raw)) continue;
    if (/^Parte da seção 10/.test(raw) || /^Problemas de lógica, risco ou limite/.test(raw)) continue;
    if (/^\| (D|PRB)/.test(raw)) count++;
    out += relink(raw) + '\n';
  }
  out += `
## Fontes e provas

- \`doc/08a-decisoes-tomadas.md\` (o registro histórico; ${count} entradas), reformatado por \`doc/pt-BR/scripts/gerar-historico.mjs\`.
- Cada decisão aponta para o documento técnico onde está descrita; as que dizem respeito ao código foram conferidas durante a escrita dos documentos de [04](04-arquitetura.md) a [27](27-ci-cd.md) (as divergências achadas estão em [31-pendencias.md](31-pendencias.md)).
`;
  fs.writeFileSync(path.join(root, 'doc/pt-BR/30-decisoes.md'), out);
  console.log(`30-decisoes.md: ${count} linhas de decisão/problema`);
}

// ---------------- 31: pendências ----------------
{
  const text = read('08b-pendencias.md');
  const lines = text.split('\n');
  let section = '';
  const items = [];
  for (const raw of lines) {
    if (/^## 1\./.test(raw)) section = 'Sem bloqueio';
    else if (/^## 2\./.test(raw)) section = 'Bloqueada por outra decisão ou etapa';
    const m = raw.match(/^- \*\*((?:D|PRB-)[0-9.]+)\.?\*\*\s*(.*)$/);
    if (m) items.push({ code: m[1].replace(/\.$/, ''), section, body: m[2] });
    else if (/^  - \*\*/.test(raw) && items.length) items[items.length - 1].body += '\n' + raw;
  }
  const date = (x) => x;
  let out = `---
título: Pendências
resumo: Tudo o que continua em aberto no projeto — dúvidas de design (D…) e problemas de lógica, risco ou limite (PRB-…) — mais as divergências e descobertas desta documentação, ainda sem código.
tipo: referência
status: parcial
verificado-em: ${today} @ ${commit}
fontes: [doc/08b-pendencias.md, doc/pt-BR/_divergencias.md]
---

# Pendências

> **Dois tipos de item, dois códigos.** **\`D…\`** é uma **dúvida de design** (o documento não define, ou há duas saídas), sempre com uma proposta; **\`PRB-…\`** é um **problema** que a implementação descobriu (o código se comporta de um jeito furado, injusto ou incompleto), com o **efeito** e a **proposta**. Os códigos **nunca são reaproveitados**; o que já foi decidido ou resolvido está em [30-decisoes.md](30-decisoes.md).
>
> **Legenda:** \`[ ]\` em aberto, sem decisão; \`[~]\` parcialmente resolvida (o texto traz **só as perguntas que faltam**, cada uma com seu sub-código); **Depende de** = a dúvida só se responde depois de outra.
>
> **Gerado** (a primeira parte) por \`node doc/pt-BR/scripts/gerar-historico.mjs\` a partir de \`doc/08b-pendencias.md\`, que é a fonte: reformatado em cartões, com os links trocados. A seção **"Descobertas desta documentação"** é escrita à mão e **ainda não está no \`08b\`**.

## Descobertas desta documentação (ainda sem código)

Achadas ao escrever os documentos desta pasta, **conferindo o código**. Cada uma tem a evidência. Para virar item oficial, entram em \`doc/08b-pendencias.md\` com o próximo \`PRB\` ou \`D\` livre (os mais altos hoje são **PRB-052** e **D128**).

| # | Tipo | O que se viu | Efeito | Proposta | Evidência |
|---|---|---|---|---|---|
| A | incompleto | **A agregação entre nós das métricas de tópico não existe.** \`GET /metrics/topics*\` com \`scope=cluster\` devolve a visão do nó que atende; \`partial\` é sempre \`false\`; a rota \`GET /cluster/metrics/topics\` não existe | \`depth\`/\`published\` do "cluster" são só os de um nó; num cluster de 3 nós com 12 mensagens, cada nó e o escopo \`cluster\` responderam 4 | Implementar a rota interna e o fan-out (com \`stats.topics.cache-ttl\`) ou documentar que só vale por nó | [13-observabilidade.md](13-observabilidade.md); \`http/topic_stats/mod.rs\`; execução num cluster de 3 nós |
| B | definição | **Sete parâmetros são aceitos mas não têm efeito:** \`delivery.ready-list.size\`, \`delivery.invisible-list.size\`, \`delivery.invisible.reload-threshold\`, \`cluster.sync.stream.max-line-bytes\`, \`cluster.sync.stream.idle-timeout\`, \`fifo.dedup.index.max-entries\`, \`stats.topics.cache-ttl\` | Quem os ajusta acredita que mudou algo; três deles se ligam a D82/D88/PRB-018/PRB-022 | Implementar, ou recusar a chave, ou tirá-los da tabela e de [22-configuracao.md](22-configuracao.md) | [22-configuracao.md](22-configuracao.md); script de uso dos campos de \`Config\` |
| C | risco | **Perder dois nós seguidos trava o cluster** (\`quorum-lost\`): sobra um nó, a exclusão do segundo não se efetiva, e os reiniciados não reentram (o registro exige todos da tabela). Não há exclusão forçada | Só se recupera parando tudo, **perdendo mensagens e tópicos** | A "exclusão forçada" já prevista no doc 04 (evolução futura): rota manual de exclusão, ou tabela por época | [07-cluster.md](07-cluster.md), [23-operacao.md](23-operacao.md); execução com \`docker compose kill\` |
| D | limite | **Parar o cluster inteiro em produção perde mensagens e definições de tópico** (todo reinício é de nó novo; o disco só devolve "perdas" listadas por nós vivos) | Uma manutenção que pare tudo, ou uma queda geral, zera o cluster | Retomar do disco quando o **primeiro nó** (\`first-node\`) sobe com disco anterior, como o modo desenvolvimento faz; ou documentar o limite como definitivo | [11-durabilidade.md](11-durabilidade.md); execução: \`docker compose down\` + \`up\` |
| E | processo | **O teste \`the_tests_column_of_the_use_cases_is_up_to_date\` falha no repositório** (a coluna "Testes" de \`doc/20*.md\` está velha) | O \`cargo test\` não passa limpo | \`TABA_UC_WRITE=1 cargo test --test uc_coverage\` e commitar | [25-testes.md](25-testes.md); execução em 2026-10-13 |
| F | documentação | **Comentários \`//!\` velhos** (\`cluster/mod.rs\`, \`cluster/drain/mod.rs\`, \`engine/mod.rs\`, \`node.rs\`): "por enquanto", "o \`Transport\` real ainda não existe" | Quem lê o código se confunde | Atualizar os comentários | \`_divergencias.md\` #10 |
| G | documentação | **\`doc/21-pipeline-ci-cd.md\` está parcialmente velho** ("não há \`.github/\`"; planeja o job \`integration\` e o GHCR, que não existem) | Plano e estado confundidos | Atualizar o doc histórico | [27-ci-cd.md](27-ci-cd.md) |
| H | documentação | **\`D79\`/\`PRB-013\` ainda constam em aberto, mas \`diskFreeBytes\` já é devolvido** (\`fs4\`); **\`D82\`** é confirmada pelo código (não há \`expiry\`) | O \`08b\` mente | Mover D79 e PRB-013 para "resolvidos"; decidir D82 | \`_divergencias.md\` #8, #9 |
| I | idioma | **\`README.md\` e \`AGENTS.md\` da raiz estão em português**; a regra do projeto é en-US | Quem chega vê o idioma errado | Traduzir (fase 8 do plano de documentação) | [00-plano-de-documentacao.md](00-plano-de-documentacao.md) |
| J | operação | **Sem backup nem restauração; sem manifestos Kubernetes** | Operar em produção exige improviso | Roadmap | [23-operacao.md](23-operacao.md), [32-roadmap.md](32-roadmap.md) |

`;
  const secs = ['Sem bloqueio', 'Bloqueada por outra decisão ou etapa'];
  for (const s of secs) {
    out += `## ${s === 'Sem bloqueio' ? '1. Sem bloqueio' : '2. Bloqueadas por outra decisão ou etapa'}\n\n`;
    out += s === 'Sem bloqueio'
      ? 'Podem ser respondidas **agora**, em qualquer ordem: não dependem de outra pergunta nem de etapa.\n\n'
      : 'Respondem-se **depois** do que está em **Depende de** (ou junto).\n\n';
    for (const it of items.filter((x) => x.section === s)) {
      const body = relink(it.body);
      const state = /^\[~\]/.test(body) ? '`[~]` parcial' : /^\[ \]/.test(body) ? '`[ ]` aberta' : (it.code.startsWith('PRB') ? 'problema' : '—');
      const dep = (body.match(/\*\*Depende de:\*\*\s*([^.]*\.?)/) || [])[1];
      const kind = it.code.startsWith('PRB') ? (body.match(/^\(([^,)]+)/) || [])[1] : 'dúvida de design';
      out += `### ${it.code}\n\n- **Estado:** ${state}${kind ? ` · **Tipo:** ${kind}` : ''}${dep ? ` · **Depende de:** ${dep.trim()}` : ''}\n\n${body.replace(/^\[[ ~]\]\s*/, '')}\n\n`;
    }
  }
  const road = read('08c-proximos-passos.md');
  out += `## Fontes e provas

- \`doc/08b-pendencias.md\` (${items.length} itens abertos nesta data), reformatado por \`doc/pt-BR/scripts/gerar-historico.mjs\`; a seção "Descobertas" vem de \`doc/pt-BR/_divergencias.md\` e das execuções descritas em [23-operacao.md](23-operacao.md) e [13-observabilidade.md](13-observabilidade.md).
- O roadmap que fecha estas pendências está em [32-roadmap.md](32-roadmap.md).
`;
  fs.writeFileSync(path.join(root, 'doc/pt-BR/31-pendencias.md'), out);
  console.log(`31-pendencias.md: ${items.length} itens`);
}
