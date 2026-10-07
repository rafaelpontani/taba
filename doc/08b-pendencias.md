# Pendências

[← Índice](README.md)

## 10.2 Pendências

Parte da seção 10. As dúvidas que ainda estão abertas; as já fechadas estão em [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md), e a ordem do trabalho em [08c-proximos-passos.md](08c-proximos-passos.md). O objetivo é **zerar as dúvidas e avançar**.

Os valores numéricos ainda "A definir" em [10-configuracao.md](10-configuracao.md) (retries, intervalos, TTLs, limiares, tetos) são **calibragem**, não dúvida de design, e por isso não aparecem aqui: podem ser fechados com um chute razoável e ajustados em teste.

Legenda:

- **D10, D13…** — código da dúvida, para responder por número (ex.: "D13: sim"). Uma dúvida com mais de uma pergunta em aberto é dividida em **sub-códigos** (**D13.1**, **D13.2**…), que podem ser respondidos separadamente. Os códigos e sub-códigos não são reaproveitados.
- `[ ]` — em aberto, sem proposta.
- `[~]` — parcialmente resolvida: o texto traz **só as perguntas que faltam**, cada uma com seu sub-código. A parte já decidida vai para [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md), nunca fica aqui.

- **D68.** [ ] **Versões-base dos conectores.** Proposta em [19a-conectores-linguagens.md](19a-conectores-linguagens.md), seção 18.10.1: Java 6, .NET Framework 3.5, Python 2.6, PHP 5.3, Ruby 1.8.7, C89, C++03, Node 0.10, Go 1.4 e Rust 1.31 (a regra é a versão das distribuições de suporte longo de 2011, e, para Go, Rust e Node, a mais antiga estável). Confirmar a regra e os casos de borda: PHP 5.2 em vez de 5.3 (ainda comum em hospedagens, mas sem *namespaces*), Python 2.6 em vez de 2.7, Java 6 em vez de 7, .NET 3.5 em vez de 4.0. Quanto mais antiga a base, mais código a evitar e mais ambientes de teste no CI.
- **D69.** [ ] **TLS em plataformas antigas.** Java 6, .NET 3.5, Python 2.6 e Ruby 1.8.7 não falam TLS 1.2 (ou o fazem só com ajustes do sistema) e algumas não conferem o certificado. O TLS do cliente é do balanceador ([06-api.md](06-api.md), seção 8.0.1). Decidir como esses conectores alcançam o Taba: (a) o balanceador **também** expõe uma porta HTTP **só na rede privada**, e a documentação o recomenda para as plataformas antigas; (b) o balanceador mantém uma porta HTTPS com TLS 1.0 e 1.1 habilitados, para esses clientes; ou (c) o conector exige a plataforma com TLS 1.2 e a base sobe. Em qualquer caso, o conector não traz uma pilha de TLS própria.
- **D70.** [ ] **Publicação dos conectores.** Falta decidir: o nome da organização e os identificadores de pacote em cada registro (Maven *groupId*, NuGet, PyPI, Packagist, RubyGems, npm, crates.io, caminho do módulo Go); se o **PHP** (e o Go) sai de um **espelho somente leitura** por `git subtree split`, já que o Packagist exige o `composer.json` na raiz; e se as versões dos conectores seguem a do servidor ou são **independentes**, como proposto (SemVer próprio, declarando a versão do contrato).
- **D71.** [ ] **Loop de consumo e API assíncrona.** A superfície mínima tem só `consume` e `ack` ([19-conectores.md](19-conectores.md), seção 18.3). Decidir se a primeira versão já traz o `subscribe(handler)` (long polling, chamada do *handler* e ack automático, com o corte de `visibilityTimeout`), e se as linguagens com `async` (C# 4.5, Python 3, Rust, Node, Go com contexto) ganham métodos assíncronos na primeira versão ou só depois. A proposta é deixar os dois para depois do contrato estar estável, sem afetar a versão-base.
- **D72.** [ ] **`PATCH` em clientes HTTP antigos.** O `HttpURLConnection` do Java recusa `PATCH`, e proxies corporativos antigos bloqueiam `PATCH`, `PUT` e `DELETE`. O `updateTopic` e a alteração de tokens dependem do `PATCH`; a invisibilidade, do `PUT`. Decidir se o Taba aceita o cabeçalho `X-HTTP-Method-Override` num `POST` (valendo só para `PATCH`, `PUT` e `DELETE`, com a mesma autorização do verbo real), ou se o conector Java usa uma conexão HTTP própria sobre `Socket` só para esse verbo.

