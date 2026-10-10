# Próximos passos

[← Índice](README.md)

## 10.3 Passos

Parte da seção 10. Cada passo lista o que cobre e as dúvidas que precisam estar fechadas antes dele (em [08b-pendencias.md](08b-pendencias.md)); as decisões já tomadas estão em [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md). Uma pendência aparece uma vez, no primeiro passo que depende dela.


**Situação (2026-10-10).** Os passos 2 a 9 estão implementados no crate `taba-node` (o mapa de módulos está na seção 17.7 de [18-estrutura-codigo.md](18-estrutura-codigo.md)), com testes de nível **C** sobre uma rede em memória e o `Transport` real com TLS mútuo. A observabilidade e as rotas de operação do passo 10 também estão feitas. **Faltam:** o passo 1 (o contrato OpenAPI), o passo 10 no que é empacotar em Docker e validar em Kubernetes, e o passo 11 (os conectores).

| Passo | O que cobre | Dúvidas abertas |
|---|---|---|
| 1 — Contrato da API (OpenAPI) | Endpoints de registro/descoberta de nós, mensagens, sincronização e recuperação (seções 6, 7 e 8). | — |
| 2 — Arquitetura do cluster e fluxo em camadas | O fluxo de escrita está definido (seção 3.1). | — |
| 3 — Protótipo de nó único (memória + disco) | O overflow por tópico (seção 3.2) e o modo drain (seção 6.10). Linguagem e estrutura do código: [18-estrutura-codigo.md](18-estrutura-codigo.md). | — |
| 4 — Protocolo genérico de registro/commit | Reutilizado em nós, mensagens e recuperação. O rollback só é seguro **antes** do quórum decidir; depois, o coordenador insiste dentro de um teto configurável. Mensagens decidem o commit por quórum mínimo (com o primário); registro/desregistro de nó e definição de tópico exigem todos os nós ([05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2). | — |
| 5 — Anel de hash circular | Blocos, nós virtuais e papéis (seção 7.1). | — |
| 6 — Sincronização distribuída de blocos | Entrada de novos nós, incluindo o evento de perda de mensagem. | — |
| 7 — Rebalanceamento de blocos | Falha de registro ou desregistro de nó. | — |
| 8 — Sinal de vida pelo canal, votação com quórum e freezing anti-flapping | Detecção de falhas, exclusão por votação e período de freezing. | — |
| 9 — Retorno de nó | Recuperação de mensagens perdidas e novo bootstrap. | — |
| 10 — Empacotamento em Docker e validação em Kubernetes | Observabilidade e rotas de operação. | — |
| 11 — Conectores | O contrato comum e os dez conectores, em fases ([19-conectores.md](19-conectores.md), seção 18.8). A fase 0 espera o contrato OpenAPI (passo 1); a fase 5, o protótipo de nó único do passo 3. | D68, D70.1, D70.2, D71.1 |

## Referências internas

- [Mapa mental do projeto](16-mapa-mental.md) — visão gráfica das áreas e das decisões.
