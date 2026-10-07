# Pendências

[← Índice](README.md)

## 10.2 Pendências

Parte da seção 10. As dúvidas que ainda estão abertas; as já fechadas estão em [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md), e a ordem do trabalho em [08c-proximos-passos.md](08c-proximos-passos.md). O objetivo é **zerar as dúvidas e avançar**.

Os valores numéricos ainda "A definir" em [10-configuracao.md](10-configuracao.md) (retries, intervalos, TTLs, limiares, tetos) são **calibragem**, não dúvida de design, e por isso não aparecem aqui: podem ser fechados com um chute razoável e ajustados em teste.

Legenda:

- **D10, D13…** — código da dúvida, para responder por número (ex.: "D13.1: o TTL é de 7 dias"). Uma dúvida com mais de uma pergunta em aberto é dividida em **sub-códigos** (**D13.1**, **D13.2**…), que podem ser respondidos separadamente. Os códigos e sub-códigos não são reaproveitados.
- `[ ]` — em aberto, sem proposta.
- `[~]` — parcialmente resolvida: o texto diz **o que já está definido**, e cada pergunta que falta tem seu sub-código.

- **D10.** [ ] **Linguagem e runtime.** Preferência pessoal avançada em C#, porém Rust parece mais adequado aos requisitos de performance, baixo overhead e concorrência. Precisa estar decidida antes de escrever código; também define o que `msg.node.threads` mede.
- **D13.** [~] **Retenção, expiração (TTL) e dead-letter.** Já definido: cada mensagem tem `deliveryCount` nos metadados, base para um limite de entregas. Reprocessamento histórico está fora do escopo (seção 5).
  - **D13.1** TTL da mensagem.
  - **D13.2** Limite de entregas antes do dead-letter.
  - **D13.3** Para onde a mensagem vai no dead-letter (tópico próprio?).

