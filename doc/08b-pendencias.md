# Pendências

[← Índice](README.md)

## 10.2 Pendências

Parte da seção 10. As dúvidas que ainda estão abertas; as já fechadas estão em [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md), e a ordem do trabalho em [08c-proximos-passos.md](08c-proximos-passos.md). O objetivo é **zerar as dúvidas e avançar**.

Os valores numéricos ainda "A definir" em [10-configuracao.md](10-configuracao.md) (retries, intervalos, TTLs, limiares, tetos) são **calibragem**, não dúvida de design, e por isso não aparecem aqui: podem ser fechados com um chute razoável e ajustados em teste.

Legenda:

- **D10, D13…** — código da dúvida, para responder por número (ex.: "D10: Rust"). Uma dúvida com mais de uma pergunta em aberto é dividida em **sub-códigos** (**D13.1**, **D13.2**…), que podem ser respondidos separadamente. Os códigos e sub-códigos não são reaproveitados.
- `[ ]` — em aberto, sem proposta.
- `[~]` — parcialmente resolvida: o texto traz **só as perguntas que faltam**, cada uma com seu sub-código. A parte já decidida vai para [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md), nunca fica aqui.

- **D65.** [ ] **Rate limit por token.** Com a autenticação por token (D64), o limite continua por IP do cliente, contado em cada nó. Passar a contar por token (também local, com o `ratelimit.burst` comum) resolveria o IP escondido atrás do balanceador e daria a cada aplicação o seu limite. Decidir se vale já: se o limite de uma chamada autenticada passa a ser do token e o IP fica só para as sem token; se cada token pode ter um limite próprio; e como fica o `Retry-After`.
- **D10.** [ ] **Linguagem e runtime.** Preferência pessoal avançada em C#, porém Rust parece mais adequado aos requisitos de performance, baixo overhead e concorrência. Precisa estar decidida antes de escrever código; também define o que `msg.node.threads` mede.

