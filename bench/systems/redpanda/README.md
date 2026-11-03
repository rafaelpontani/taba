# Redpanda

Imagem `redpandadata/redpanda:v24.2.7`, um nó, fator 1, sem `dev-container` (que desliga o fsync). Driver `kafka` do
OMB. `default` liga o *write caching* do tópico (confirma antes do fsync); `fsync` o desliga.
