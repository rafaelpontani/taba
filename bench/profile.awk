# Funções do bench/profile.sh, em awk (roda no host e no contêiner do perf, com o awk do BusyBox).
#   mode=cpu     uma linha do `perf report` (percentual, próprio, nome) com o nome encurtado
#   mode=self    o mesmo para o relatório de tempo próprio (percentual, nome)
#   mode=offcpu  dois arquivos do `perf script` de `sched:sched_switch`: o primeiro de todas as CPUs, sem pilha (dá
#                quanto tempo cada thread ficou fora da CPU), o segundo só do servidor, com a pilha por DWARF (a da
#                musl não tem frame pointer, e a pilha por frame pointer para no `syscall`). O evento é o mesmo nos
#                dois, com o mesmo instante: a soma do tempo fora da CPU sai por pilha.

# O nome de um símbolo Rust (v0, `_R...`) reduzido aos três últimos identificadores (`messages::Messages::take_alone`).
# Não é um decodificador completo: basta para ler um perfil.
function short(s,    i, c, n, id, cnt, L, out, k, parts) {
  sub(/\+0x[0-9a-f]+$/, "", s)
  sub(/\.llvm\.[0-9]+$/, "", s)
  if (substr(s, 1, 2) != "_R") return s
  i = 3; L = length(s); cnt = 0
  while (i <= L) {
    c = substr(s, i, 1)
    if (c ~ /[0-9]/) {
      n = 0
      while (i <= L && substr(s, i, 1) ~ /[0-9]/) { n = n * 10 + substr(s, i, 1); i++ }
      if (substr(s, i, 1) == "_") i++
      id = substr(s, i, n); i += n
      if (id != "") parts[++cnt] = id
    } else if ((c == "s" || c == "B") && match(substr(s, i), /^[sB][0-9A-Za-z]*_/)) {
      i += RLENGTH
    } else {
      i++
    }
  }
  out = ""
  for (k = (cnt > 3 ? cnt - 2 : 1); k <= cnt; k++) out = out (out == "" ? "" : "::") parts[k]
  return out == "" ? s : out
}

# O instante (texto, `12345.678901`) de uma linha de evento do `perf script`.
function stamp(    i) {
  for (i = 1; i <= NF; i++) if ($i ~ /^[0-9]+\.[0-9]+:$/) return substr($i, 1, length($i) - 1)
  return ""
}

# `<comm>:<pid>` → pid global (o nome pode ter `:`).
function pid_of(s) { sub(/ \[[0-9]+\].*$/, "", s); sub(/^.*:/, "", s); return s }

function close_stack() {
  if (skey != "") stack[skey] = frames
  skey = ""
}

mode == "cpu" && NF >= 4 { printf "    %7s %7s  %s\n", $1, $2, short($4); next }
mode == "self" && NF >= 3 { printf "    %7s  %s\n", $1, short($3); next }

# Primeiro arquivo: os tempos. A thread que sai tem o tid do espaço de processos do servidor (o campo do evento) e o
# pid global (no texto do evento); a que entra, só o global.
mode == "offcpu" && FNR == NR {
  if (!index($0, "sched:sched_switch:")) next
  t = stamp()
  rest = substr($0, index($0, "sched:sched_switch:") + 20)
  split(rest, side, " ==> ")
  prev = pid_of(side[1]); nxt = pid_of(side[2])
  n = split(side[1], left, " "); st = left[n]
  local_tid[prev] = $2
  if (nxt in out_at) { took[local_tid[nxt] " " out_at[nxt]] = t - out_at[nxt]; delete out_at[nxt] }
  out_at[prev] = t; state[$2 " " t] = st
  next
}

# Segundo arquivo: a pilha de cada saída das threads do servidor.
mode == "offcpu" && /sched:sched_switch:/ { close_stack(); skey = $2 " " stamp(); frames = ""; kdepth = 0; udepth = 0; next }
mode == "offcpu" && skey != "" && NF >= 2 {
  if ($2 ~ /^(__schedule|schedule|\[unknown\])$/) next
  # Do núcleo, só os três primeiros quadros (por que a thread parou: futex, leitura, epoll); do programa, dez.
  if ($1 ~ /^ffffffff/) { if (kdepth++ >= 3) next } else if (udepth++ >= 10) next
  frames = frames (frames == "" ? "" : " < ") short($2)
  next
}

END {
  if (mode != "offcpu") exit
  close_stack()
  for (k in stack) {
    if (!(k in took)) continue
    key = "[" state[k] "] " stack[k]
    total[key] += took[k]; count[key]++
  }
  for (k in total) printf "%10.1f ms %8d vezes  %s\n", total[k] * 1000, count[k], k
}
