# Próximos passos e referências

[← Índice](README.md)

## 12. Próximos passos
 
1. Validar este escopo e fechar os pontos em aberto.
2. Definir o contrato da API (OpenAPI), incluindo os endpoints de registro/descoberta de nós, mensagens, sincronização e recuperação (seções 6, 7 e 8).
3. Desenhar a arquitetura do cluster e o fluxo de escrita em camadas.
4. Construir um protótipo de nó único (memória + disco), incluindo o overflow por tópico (seção 3.2).
5. Implementar o protocolo genérico de registro/commit e reutilizá-lo em nós, mensagens e recuperação.
6. Implementar o anel de hash circular.
7. Implementar a sincronização distribuída de blocos na entrada de novos nós, incluindo o evento de falha de perda de mensagem.
8. Implementar o rebalanceamento de blocos em caso de falha de registro ou desregistro de nó.
9. Implementar retries de ping, votação com quórum e o período de freezing anti-flapping.
10. Implementar o fluxo de retorno de nó (recuperação de mensagens perdidas + novo bootstrap).
11. Empacotar em Docker e validar em Kubernetes.
## 13. Referências internas
 
- Mapa mental do projeto (`mapa-mental.html`, neste projeto) — visão gráfica do escopo e das decisões.
