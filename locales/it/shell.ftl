# Testi della shell desktop mostrati fuori dalle finestre dell'app: titoli delle finestre e
# notifiche di sistema. Il termine `-brand` viene da `menu.ftl`.

recording-panel-title = { -brand } — Registrazione
meeting-ready-title = Meeting pronto
# Mostrato quando il meeting non ha ancora un titolo.
meeting-ready-body = La trascrizione e il riassunto sono pronti.
meeting-failed-title = Trascrizione non riuscita
meeting-failed-body = Apri il meeting in { -brand } per riprovare.

## Elemento della barra dei menu. Registra, Nuova nota, Impostazioni ed Esci usano le
## etichette di menu.ftl.

tray-tooltip = { -brand }
tray-tooltip-recording = { -brand } — Registrazione
tray-open = Apri { -brand }
tray-processing = In elaborazione
# Un meeting in elaborazione: il titolo, poi cosa sta succedendo, ad esempio
# "Weekly sync — Trascrizione 62%".
tray-processing-row = { $title } — { $status }
tray-transcribing = Trascrizione
tray-transcribing-percent = Trascrizione { $percent }%
tray-summarizing = Riassunto
tray-waiting = In attesa
tray-retrying = Nuovo tentativo più tardi
tray-latest-meetings = Ultimi meeting
tray-no-meetings = Ancora nessun meeting
tray-meeting-untitled = Meeting senza titolo

## Conferma quando si esce durante una registrazione o mentre un meeting è in elaborazione.

quit-recording-title = Interrompere la registrazione e uscire?
quit-recording-body = Il meeting registrato finora viene conservato e trascritto alla prossima apertura di { -brand }.
quit-processing-title = Uscire mentre un meeting è in elaborazione?
quit-processing-body = { -brand } sta trascrivendo o riassumendo un meeting. Riprenderà alla prossima apertura di { -brand }.
quit-confirm = Esci
quit-cancel = Annulla
