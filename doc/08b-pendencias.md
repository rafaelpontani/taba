# Pendências

[← Índice](README.md)

## 10.2 Pendências

Parte da seção 10. As dúvidas que ainda estão abertas; as já fechadas estão em [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md), e a ordem do trabalho em [08c-proximos-passos.md](08c-proximos-passos.md). O objetivo é **zerar as dúvidas e avançar**.

Os valores numéricos ainda "A definir" em [10-configuracao.md](10-configuracao.md) (retries, intervalos, TTLs, limiares, tetos) são **calibragem**, não dúvida de design, e por isso não aparecem aqui: podem ser fechados com um chute razoável e ajustados em teste.

Legenda:

- **D10, D13…** — código da dúvida, para responder por número (ex.: "D13: sim"). Uma dúvida com mais de uma pergunta em aberto é dividida em **sub-códigos** (**D13.1**, **D13.2**…), que podem ser respondidos separadamente. Os códigos e sub-códigos não são reaproveitados.
- `[ ]` — em aberto, sem proposta.
- `[~]` — parcialmente resolvida: o texto traz **só as perguntas que faltam**, cada uma com seu sub-código. A parte já decidida vai para [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md), nunca fica aqui.

- **D68.** [ ] **Versões-base dos conectores: confirmar a tabela.** A "versão-base" é a versão mais antiga de cada linguagem que o conector aceita rodar. Em vez de escolher cada uma no olho, a proposta é uma regra só, derivada do que já foi decidido: a base é a **mais antiga** que (1) fala TLS 1.2 ou superior (D69), (2) tem cliente HTTP com `PATCH` (D72) e, onde a linguagem tem assíncrono, o usa (D71). A tabela que sai da regra está em [19a-conectores-linguagens.md](19a-conectores-linguagens.md), seção 18.10.1: **Java 11, .NET Framework 4.6.1 (clássico) e .NET 5 (moderno), Python 2.7.9 e 3.4.3, PHP 5.6, Ruby 2.0, C89 com `libcurl` 7.34, C++03, Node 0.12, Go 1.7 e Rust 1.70**. A pergunta é: **a regra e a tabela estão certas?** O ponto de maior impacto é o **Java**, que sobe de 6 para 11 e deixa o **Java 8** (ainda muito usado) de fora, só por causa do `PATCH`. O Rust 1.70 é uma estimativa a fechar no protótipo.
- **D70.** [~] **Publicação dos conectores.** Já decidido: o lugar de cada um (a regra "mais popular e gratuito"). Falta:
  - **D70.1.** O **nome da organização** e os identificadores de pacote em cada registro: Maven *groupId* (que exige um domínio ou uma organização verificada), NuGet, PyPI, Packagist, RubyGems, npm, crates.io e o caminho do módulo Go (que é o do repositório).
  - **D70.2.** O **PHP** (Packagist exige o `composer.json` na raiz) sai de um **espelho somente leitura** por `git subtree split`? E as versões dos conectores são **independentes** da do servidor, como proposto (SemVer próprio, declarando a versão do contrato)?
- **D71.1.** [ ] **`subscribe(handler)` na primeira versão?** O laço de consumo (long polling, chamada do *handler* e ack automático, com o corte de `visibilityTimeout`) é um adicional de poucas linhas sobre o `consume` + `ack` ([19-conectores.md](19-conectores.md), seção 18.3). A proposta é incluí-lo na primeira versão **nas linguagens com assíncrono**, já que ele é a forma natural de usar o assíncrono, e deixá-lo de fora do C, C++, PHP e Ruby.
