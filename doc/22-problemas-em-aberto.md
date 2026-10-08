# Problemas em aberto

[← Índice](README.md)

Registro dos **problemas não resolvidos** que a implementação vai descobrindo, principalmente de **lógica** (o comportamento atual tem um furo) e de **falta de definição** (o documento não diz o que fazer). É diferente de dois arquivos vizinhos:

- [08b-pendencias.md](08b-pendencias.md) guarda as **perguntas de design** para o autor responder (com proposta). Todo problema deste arquivo que precisa de uma decisão aponta para a sua dúvida `D...` lá.
- [21-historico-implementacao.md](21-historico-implementacao.md) registra o que foi feito e os desvios. Um desvio que vira risco de comportamento sobe para cá.

Um problema **resolvido** não é apagado: a linha vai para "Resolvidos" com a data e o que mudou (o código `PRB-NNN` não é reaproveitado).

**Tipos:** `lógica` (o comportamento atual está errado ou injusto em algum caso), `definição` (falta decidir), `limite` (funciona, mas só até certo ponto) e `risco` (pode dar errado em produção). **Situação:** `Aberto` ou `Decidido` (já há resposta em [08a](08a-decisoes-tomadas.md) ou no documento técnico, falta implementar).

## Em aberto

| Código | Tipo | Onde | O problema | Efeito | Decisão | Situação |
|---|---|---|---|---|---|---|
| PRB-001 | lógica | `engine/messages` | Um `Mutex` único guarda a memória e o acesso ao disco de **todos** os tópicos. | Uma gravação lenta em disco de um tópico trava o `GET`, o ack e a publicação de todos os outros. | Etapa 10 (uma fila de comandos por tópico, [18](18-estrutura-codigo.md) §17.4). | Decidido |
| PRB-002 | lógica | `engine/messages` | O overflow só manda para o disco a mensagem **nova**; nada é rebaixado. | O tópico que encheu a memória primeiro a mantém para sempre; os outros ficam sempre em disco, mais lentos. | D83 | Aberto |
| PRB-003 | lógica | `cluster/state`, `engine/messages` | O nó em `failed` só muda o `/health/ready`; continua aceitando e respondendo chamadas. | Com o disco corrompido, cada publicação devolve `500` e o nó não sai da rotação sozinho se o balanceador só olha as chamadas. | D78 | Aberto |
| PRB-004 | lógica | `cluster/drain` | O drain pedido pela API que estoura o prazo (ou tem erro de disco) fica em `draining` sem terminar nem voltar. | O nó fica fora da rotação sem a marca do save, esperando um `DELETE` do operador. | D75 | Aberto |
| PRB-005 | definição | `http` (drain) | No drain o nó recusa **toda** chamada de cliente, inclusive o ack e o `PUT visibility`. Sem outro nó para repassar, o consumidor não confirma o que já recebeu. | A mensagem entregue volta a ficar pronta quando a invisibilidade vence (duplicata) se o nó seguir em drain por mais que o `visibilityTimeout`. O `receiptToken` só volta a valer depois do `DELETE` ou do reinício. | Falta decidir se o ack e o `PUT visibility` passam durante o drain no modo desenvolvimento; ainda sem dúvida em 08b. | Aberto |
| PRB-006 | definição | `http/operations` | O `allowed-ips` filtra também `/health/live` e `/health/ready`, e o `403` sai como `ErrorResponse`, contra o que o 06b diz dessas rotas. | O orquestrador pode ler `403` como nó doente. | D76 | Aberto |
| PRB-007 | risco | `http/operations` | O filtro usa o IP da conexão. Atrás de um balanceador todos os pedidos vêm do IP dele. | Ou todos passam (IP do balanceador na lista) ou nenhum. | Etapa 7 (`client-ip.headers`, `client-ip.trusted-proxies`, [10](10-configuracao.md)). | Decidido |
| PRB-008 | limite | `storage/messages` | Nome de tópico igual a um nome reservado do Windows (`con`, `nul`...) não vira pasta. | Publicar nesse tópico no Windows dá erro de disco (`500`). | D80 | Aberto |
| PRB-009 | risco | `engine/topics` | A fila FIFO (`fifo: true`) é aceita, mas sem ordem por grupo nem deduplicação. | Quem cria uma FIFO acredita numa garantia que não existe. Proposta: recusar a criação com `422` até a etapa 11. | Ainda sem dúvida em 08b. | Aberto |
| PRB-010 | risco | `http/messages` | O parâmetro `autoCreate` do `GET` é ignorado em silêncio. | O cliente espera a fila criada e recebe `404`. | Etapa 9 ([15](15-topicos-distribuidor.md) §14.4). | Decidido |
| PRB-011 | definição | `storage` | Os tokens ainda são documentados como um arquivo por token, contra o critério do `_node.db`. | A etapa 8 não pode começar sem a decisão. | D81 | Aberto |
| PRB-012 | definição | `storage/messages` | A tabela `expiry` do doc 14 não existe; o índice `invisible_until` faz o papel. | O texto de 14 descreve uma tabela que o código não tem. | D82 | Aberto |
| PRB-013 | definição | `http/operations` | `NodeStatus.storage.diskFreeBytes` não é devolvido. | O campo documentado falta no `/health/cluster`. | D79 | Aberto |
| PRB-014 | limite | `engine/messages` | O drain move uma mensagem por vez, tópico por tópico, em ordem de nome. | Mais lento que o previsto em [04](04-cluster.md) (paralelo entre tópicos, menor taxa de leitura primeiro); com muita mensagem em memória pode estourar `drain.timeout`. | Etapas 10 (taxa de leitura) e 14. | Decidido |

## Resolvidos

Nenhum ainda.
