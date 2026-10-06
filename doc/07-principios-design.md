# Princípios de design

[← Índice](README.md)

## 9. Princípios de design
 
1. **Simplicidade primeiro:** menos conceitos, menos configuração, padrões sensatos.
2. **Velocidade por padrão:** o caminho crítico passa pela memória.
3. **Durabilidade configurável:** o usuário escolhe o equilíbrio entre latência e garantia.
4. **Operação automática:** cluster que se organiza sozinho.
5. **Reuso de protocolo:** o mesmo mecanismo de duas fases (registro → commit) serve tanto para mensagens quanto para registro/desregistro de nós e recuperação de mensagens.
6. **Falha isolada não vira falha de saúde:** a perda pontual de uma mensagem durante sincronização gera alerta, não exclusão do nó; já a falha do nó como um todo no registro leva à exclusão e ao rebalanceamento.
7. **Resiliência a instabilidade:** retries antes de votar exclusão, e período de freezing configurável, evitam reações exageradas a falhas transitórias e nós entrando/saindo repetidamente do cluster.
8. **Memória para quem usa mais:** o espaço de memória por tópico não é fixo — tópicos com maior fluxo de leitura ganham prioridade, mantendo o caminho crítico rápido onde mais importa.

