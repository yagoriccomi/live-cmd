# ENTREGA — testes instáveis do host (desktop)

Plano: `docs/planos/PLANO-flaky-testes.md`.

## Resultado

| Fatia | Commit | Correção |
|---|---|---|
| A | `fb92f74` | `loopback_backpressure` com prazos de retomada/drenagem compatíveis com carga; `fdir` limpa `live-cmd-d4c-<pid>` uma vez por processo (`Once`), eliminando corrida entre testes; revogação espera no máximo 400 ms pelo ack do erro (antes até 2 s) — **mudança de produto (N12)**: a revogação fecha em ≤ 1 s. |
| B | `da5f4f0` | `n25_slow_network_expires` aplica `net_ms = 50` só antes da finalização (`ticked`), sem depender do relógio na fase de envio. |
| C | — | `remote_upload_file` não reproduziu em laço nem sob carga; `s2_two_clients_simultaneous` também não. Sem alteração. |

## Verificação

- Cada teste corrigido: 30 execuções seguidas isoladas (`--exact` com caminho completo), todas ok.
- Bateria: `cargo test --lib -p live-cmd-echo -j 2 -- --test-threads=4` → 148 passed, 0 failed, 11 ignored (42,7 s).

## Não verificado

- Fatia C pode voltar a falhar em máquina mais lenta; sem reprodução não há causa confirmada.
- A bateria com paralelismo total (padrão) não rodou: o sistema a encerrou por falta de memória.
