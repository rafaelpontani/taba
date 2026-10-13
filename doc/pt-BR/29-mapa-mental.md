---
título: Mapa mental
resumo: O Taba inteiro em um mapa — o centro e as oito áreas, com as decisões e os parâmetros de cada uma e o link do documento onde se aprofunda.
tipo: referência
status: estável
verificado-em: 2026-10-13 @ 71183b9
fontes: [doc/pt-BR]
---

# Mapa mental

> Um mapa **em texto** (listas aninhadas). Cada ramo diz o essencial e aponta para o documento que o explica. Os diagramas ainda não foram desenhados; os pontos estão marcados ao final.

## O centro

**Taba** = mensageria **HTTP**, de **uma imagem**, com **fila + distribuição**, **sem coordenador**. Cada **nó** é igual aos outros. O que decide *onde* uma mensagem mora é **o anel de hash**; o que decide *se* uma operação vale é **o quórum** (mensagens) ou **todos os nós** (mudança de membros).

```
                    ┌────────────── 1. Tópicos ──────────────┐
                    │  fila · distribuidor · volátil · FIFO   │
   8. Operação ─────┤                                        ├───── 2. Publicação
   configuração     │                                        │      registro + commit · quórum
   runbook          │              T A B A                   │
                    │   nó = mesmo programa, 3 portas        │
   7. Segurança ────┤                                        ├───── 3. Entrega
   token · mTLS     │                                        │      invisibilidade · ack · long polling
                    │                                        │
   6. Observabil. ──┤                                        ├───── 4. Anel e replicação
   OTLP · Prometheus│                                        │      blocos · P/S/T
                    └──────── 5. Cluster e armazenamento ────┘
                              entrada · falha · drain · memória + disco
```

## 1. Tópicos ([09](09-topicos.md))

- **Dois tipos:** `queue` (guarda e entrega a **um** consumidor) · `distributor` (copia para filas cujo nome começa com um **prefixo**)
- **Dois atributos da fila:**
  - **Volátil:** some sem consumidor (`idleTimeout`, padrão 600 s) · criável pelo `GET` (`autoCreate`) · **nó gestor** decide a exclusão
  - **FIFO:** ordem **por grupo** · uma mensagem por grupo em voo · **deduplicação** (janela 300 s) · quórum com **primário + secundário**
- **Dead-letter:** `{fila}-dead-letter`, criada com a fila · recebe o que estoura `maxDeliveries`
- **Campos:** `ttl` (14 d) · `maxDeliveries` (10) · `maxMessageBytes` (5 MB) · tags (telemetria)
- **Imutáveis:** `name`, `id`, `type`, `volatile`, `fifo`

## 2. Publicação ([10](10-publicacao.md), [11](11-durabilidade.md))

- **Qualquer nó** recebe → vira **coordenador** → calcula os responsáveis pelo **`id`** (GUID v7)
- **Fase 1 (registro)** a todos, em memória → **quórum** (primário **sempre** incluso) → **fase 2 (commit)** → resposta
- **Antes** do quórum: pode **abortar** (rollback, `write.pending.ttl`) · **depois**: **nunca** desfaz, só insiste (`write.commit.timeout`)
- **Parâmetros:** `replication.factor` (3) · `write.quorum` (maioria) · `write.retry.*`
- **Garantia:** sobrevive à queda de `quorum − 1` nós · **não** sobrevive à parada de **todos** (produção)

## 3. Entrega ([12](12-entrega.md))

- **Invisibilidade** (`visibilityTimeout` 120 s) · `receiptToken` por entrega · ack com o token · reentrega se vencer
- **Dono da entrega** = primário do bloco · **lista de prontos** (efeito sanfona) · **lista de invisíveis**
- **Long polling** (`waitTime` ≤ 20 s) · **canal permanente** (sinal de vida 2 s, avisos, `take`)
- **Entrega justa** entre grupos (faixas de potência de 2, `normal-every` 10, peso 0,1)
- **Retenção:** `ttl` descarta (sem dead-letter) · `maxDeliveries` → dead-letter

## 4. Anel e replicação ([06](06-anel-e-replicacao.md))

- **Posição** = 8 bytes do SHA-256 · **128 nós virtuais** por nó · **bloco** = arco que termina num nó virtual
- **Primário / secundário / terciário** = nós físicos distintos ao longo do anel
- **Um anel, quatro chaves:** mensagem (`id`) · grupo FIFO (`topicId:groupId`) · gestor da volátil (`topicId`) · dono da definição (`topic:nome`)
- **Anel muda** → nó novo sincroniza · nó que perdeu o bloco só apaga com o cluster íntegro

## 5. Cluster e armazenamento ([07](07-cluster.md), [08](08-armazenamento.md))

- **Entrada:** `cluster.entry-address` → `GET /cluster/nodes` → `join` → registro + commit em **todos** → **sincroniza** (tópicos, blocos, banco de eventos, verificação final) → **pronto**
- **Falha:** canal cai → reconecta (3×) → **votação** (51%, ≥ 2 votantes) → desregistro → **freezing** (5 min)
- **Retorno:** lista de perdas → devolve o que tem → entra **como nó novo**
- **Drain:** tudo para o disco → desregistro **sem votação** · `SIGTERM` dispara
- **Modo desenvolvimento:** 1 nó, sem réplicas · drain = *save*
- **Armazenamento:** **memória primeiro**; **overflow** em disco (`memory.limit`) · `_node.db` · `<topico>/_topic.db` + `<id>.body` · `_sync/sync.db`
- **Estados do nó:** `joining` · `syncing` · `active` · `returning` · `failed` (+ `drainState`)

## 6. Observabilidade ([13](13-observabilidade.md))

- **OTLP** (push: métricas, traces, logs) · **`/metrics`** Prometheus (um alvo por nó) · **JSON** (`/health/cluster`, `/metrics/topics*`)
- **84 métricas** de nó (`taba.*`, `http.server.*`, `process.*`) · de tópico **opt-in** (`telemetry.topics.mode`)
- **Condições do cluster (8):** `member-unreachable` · `node-frozen` · `syncing` · `under-replicated` · `write-quorum-at-risk` · `quorum-lost` · `membership-divergent` · `storage-pressure`
- **Saúde:** `/health/live` · `/health/ready` (503 em drain/sincronização)

## 7. Segurança ([14](14-seguranca.md))

- **Cliente:** token `tb_<id>.<segredo>` (hash salgado) · permissões `publish`/`consume`/`create`/`admin` · prefixos · token **raiz**
- **Cluster:** **TLS mútuo**, certificado único em pasta recarregada, **rotação** com `--starts-in`
- **Operação:** sem autenticação → **rede** + `operations.allowed-ips`
- **Rate limit** por token ou IP (desligado por padrão) · IP do cliente + `trusted-proxies`

## 8. Operação e interfaces ([15](15-api-visao-geral.md)–[23](23-operacao.md))

- **APIs:** cliente (8080) · cluster (8081) · operação (opcional)
- **CLIs:** `taba` (servidor: `init`, `cert generate`, `token generate`) · `taba-cli` (cliente, perfis, TUI)
- **Conectores:** Rust ✅ · C# ✅ · 8 ⬜
- **Configuração:** YAML + `TABA_*` · `config/params`
- **Runbook:** um nó por vez · drain antes de parar · nunca parar tudo

## Mapa dos documentos por pergunta

| Pergunta | Documento |
|---|---|
| "O que é isso e como o ponho de pé?" | [01](01-visao-geral.md), [02](02-inicio-rapido.md) |
| "Onde fica minha mensagem?" | [06](06-anel-e-replicacao.md), [08](08-armazenamento.md) |
| "Posso perder uma mensagem?" | [11](11-durabilidade.md) |
| "Como a mensagem chega ao consumidor?" | [12](12-entrega.md) |
| "Um nó caiu: e agora?" | [07](07-cluster.md), [23](23-operacao.md) |
| "Qual rota, qual erro?" | [16](16-api-cliente.md), [19](19-erros-e-limites.md) |
| "Como configuro?" | [22](22-configuracao.md) |
| "Como monitoro?" | [13](13-observabilidade.md) |
| "Como mexo no código?" | [24](24-convencoes.md), [25](25-testes.md), [26](26-casos-de-uso.md) |

![Mapa mental geral](../diagramas/img/mapa-mental.svg)

![1. Escrita e quórum](../diagramas/img/mapa-mental-escrita.svg)

![2. Entrega](../diagramas/img/mapa-mental-entrega.svg)

![3. Invisibilidade](../diagramas/img/mapa-mental-invisibilidade.svg)

![4. Tópicos](../diagramas/img/mapa-mental-topicos.svg)

![5. Cluster e membros](../diagramas/img/mapa-mental-cluster.svg)

![6. Sincronização](../diagramas/img/mapa-mental-sync.svg)

![7. Armazenamento](../diagramas/img/mapa-mental-disco.svg)

![8. API e observabilidade](../diagramas/img/mapa-mental-obs.svg)

## Fontes e provas

- `16-mapa-mental.md` (a estrutura em oito áreas) reescrito sobre os documentos desta pasta; os números (84 métricas, 8 condições, 5 estados, 128 nós virtuais, 3 portas) conferidos no código ([13](13-observabilidade.md), [07](07-cluster.md), [06](06-anel-e-replicacao.md)).
- Casos de uso: todas as áreas ([26a](26a-casos-de-uso-implementados.md)).
