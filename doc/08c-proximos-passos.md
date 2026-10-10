# Próximos passos

[← Índice](README.md)

## 10.3 Passos

Parte da seção 10. Cada passo lista o que cobre e as dúvidas que precisam estar fechadas antes dele (em [08b-pendencias.md](08b-pendencias.md)); as decisões já tomadas estão em [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md). Uma pendência aparece uma vez, no primeiro passo que depende dela.

## Pronto: o servidor

O nó e o cluster estão implementados no crate `taba-node` (o mapa de módulos está na seção 17.7 de [18-estrutura-codigo.md](18-estrutura-codigo.md)): um nó com memória e disco, drain, rotas de operação e observabilidade, e o cluster com anel de hash, tabela de membros, canal WebSocket com TLS mútuo, escrita e ack por quórum, sincronização, votação com freezing e retorno de nó. Os testes de nível **C** rodam sobre uma rede em memória, e há testes com o `Transport` real. O que já se fez nesta frente (os passos 2 a 9 do plano antigo) não volta a ser listado.

## Versão 0.1: o que falta

A 0.1 fecha quando uma pessoa sobe um cluster de três nós com um comando, usa a API pelo `taba-cli` num terminal e por um aplicativo de console em C#. A ordem é esta, e cada passo só começa depois do anterior:

| Passo | O que cobre | Dúvidas abertas |
|---|---|---|
| 1 — `taba-cli`, o primeiro conector | A linha de comando do cliente ([19b-cli.md](19b-cli.md)): todos os comandos da API de cliente e de operação (tópicos, publicar, consumir, `subscribe`, ack, invisibilidade, tokens, saúde, métricas e drain), a **imagem Docker em Alpine** e os **scripts de build** para Windows, Linux e macOS, em x64 e arm64. Inclui os **perfis de conexão** (arquivo YAML na pasta de configuração do usuário, perfil padrão e perfil de sessão; sem configuração usa `127.0.0.1` na porta padrão). É também a ferramenta de teste de qualquer terminal: com ela se confere o cluster nos passos seguintes. Fica sobre o conector Rust. | — |
| 2 — Docker e Docker Compose | Na **raiz do repositório**: o `Dockerfile` do nó (binário `taba` em imagem mínima, com `taba init`) e o `docker-compose.yml` de **cluster completo**: um contêiner inicial roda `taba init` e cria a pasta de segredos (certificado do cluster e token raiz) num volume, e os **três nós** montam o volume, sobem na ordem certa (o primeiro com `cluster.first-node`, e os outros esperam o `healthcheck` dele) e se juntam por um **HAProxy** na frente deles, que é o `cluster.entry-address` e também balanceia a API de cliente. O HAProxy é **só um exemplo**: qualquer balanceador serve. O serviço `taba-cli`, sob o *profile* `tools`, usa a mesma rede. O **README.md** da raiz ganha um exemplo curto de como subir o Compose e fazer a primeira publicação com o `taba-cli`. O exemplo de [06-api.md](06-api.md) é o ponto de partida. | — |
| 3 — Conector C# | `connectors/csharp/` (`Taba.Client`, roda no .NET Framework 4.6.1 e no .NET 5, e compila com o SDK 8 e o 10, [19a-conectores-linguagens.md](19a-conectores-linguagens.md)): as nove operações, **todas** com a forma `...Async` (`Task`, `async`/`await`), o `subscribe(handler)`, o leitor de JSON mínimo e os testes NUnit contra um Taba real. É escrito a partir de [06a-api-cliente.md](06a-api-cliente.md) e do que o `taba-cli` já fixou. Fica de fora da 0.1 a publicação no nuget.org. | — |
| 4 — Exemplo console em C# | Um projeto de console (`exemplos/csharp-console/`) que usa o conector: cria um tópico, publica, consome com long polling e confirma. Lê o endereço e o token raiz de variáveis de ambiente. | — |
| 5 — Validação | Subir o Compose do zero e conferir de ponta a ponta: o cluster forma (`taba-cli health cluster` sem condições), o `taba-cli` e o exemplo C# publicam e consomem, e a queda de um nó (`docker stop`) é detectada, excluída por votação e o nó volta como nó novo sem perder mensagens confirmadas. O que falhar vira correção no servidor, no CLI, no conector ou no Compose, e a 0.1 só fecha quando o roteiro passa limpo. | — |

## Depois da 0.1

- **Contrato OpenAPI da API de cliente** ([06a-api-cliente.md](06a-api-cliente.md)): a fase 0 do plano de conectores ([19-conectores.md](19-conectores.md), seção 18.8). Os outros nove conectores esperam por ele, para não divergirem do que o C# fixou.
- **Os demais conectores**, em fases (seção 18.8 de [19-conectores.md](19-conectores.md)); o C# passou a ser o primeiro, antes da fase 1.
- **Publicação** dos pacotes (D70) e a **validação em Kubernetes**.

## Referências internas

- [Mapa mental do projeto](16-mapa-mental.md) — visão gráfica das áreas e das decisões.
