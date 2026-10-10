# Validação manual — Mac

Máquina: Mac M2 8 GB · macOS: 27.0 · Build: d5b2140 (assinado com identidade local "gow-meet dev", hardened runtime) · Data: 09/10/2026

| # | Cenário | Resultado | Observação |
|---|---|---|---|
| 1 | Primeira abertura: baixa modelos com progresso e verifica hash | ok | modelos já baixados; verificação por hash |
| 2 | Permissões negadas: mensagem e orientação aparecem; com só microfone o app funciona | ok | aviso de Gravação de Tela registrado; sessão seguiu sem o áudio do sistema |
| 3 | Reunião real em inglês pelo app do Teams: tradução acompanha a conversa | ok | percepção do usuário; atraso não cronometrado. Referência do CLI no M2 (small.en): p50 1,2 s / p90 1,5 s |
| 4 | Sua fala aparece como "Você"; a dos outros como "Participante" | ok | "Participante" ok no app (39 falas); "Você" ok no app assinado com a nova interface (build ac9f399: 4 sessões, 100 falas do microfone, 100% traduzidas) |
| 5 | Trocar fone ↔ alto-falante no meio: captura continua | ok | log registrou "reconectando fonte de áudio (Mic)" e a captura continuou. No alto-falante a fala do participante entra pelo microfone e duplica como "Você" (eco, confirmado no CLI) — usar fone |
| 6 | Janela flutuante fica por cima do Teams (fora de tela cheia) e pode ser arrastada | ok | percepção do usuário |
| 7 | Encerrar: `transcript.md` confere com a reunião (original + tradução) | ok | 5 min, 39 falas, 100% traduzidas, transcript.md gerado; encerramento ao sair do app registrado |
| 8 | Forçar saída (`kill -9`) no meio e reabrir: sessão é encerrada e `transcript.md` gerado | pendente | não executado. A recuperação de uma sessão antiga funcionou, mas revelou o bug abaixo |
| 9 | CPU com app aberto e parado (Monitor de Atividade) | ok | 0,0% CPU, ~13 MB (processo principal, 5 amostras) |
| 10 | CPU e memória durante a reunião | pendente | não medido |
| 11 | Logs em `~/Library/Application Support/gow-meet/logs/` não contêm texto de fala | ok | só mensagens de controle (sessão iniciada/encerrada/recuperada, pastas, aviso de permissão) |

Teste ao vivo do CLI (antes do app): áudio do sistema 2/2 frases transcritas e traduzidas; microfone 3/3 frases com fone; sem fone, 1 linha de eco como "Você".

Pendências encontradas:
- Recuperação de sessão sem falas gravava como término a hora da recuperação (duração de 7 h). Corrigido em e0ccfef: sem falas, término = início.
- Cenários 8 e 10 a repetir na próxima reunião.
- Cmd+Q durante uma sessão: encerramento limpo confirmado no log ("sessão encerrada ao sair do app", transcript gerado).
- Eco no alto-falante: só mitigado (aviso para usar fone); cancelamento de eco fica para depois.
