# Interfaccia dell'app desktop (frontend).

-brand = Trama

## Barra laterale

sidebar-label = Barra laterale
sidebar-collapse = Chiudi la barra laterale
sidebar-expand = Apri la barra laterale
nav-label = Principale
nav-home = Home
nav-sources = Fonti
nav-settings = Impostazioni
record-meeting = Registra meeting
record-unavailable = La registrazione non è ancora disponibile: arriva con la finestra di registrazione.

## Barra in alto e avvisi

back = Indietro
notice-close = Chiudi
job-transcribe = Trascrizione
job-summarize = Riassunto
job-delete_audio = Eliminazione dell'audio
job-queued = In attesa
# Altre elaborazioni in corso oltre a quella mostrata.
job-more = +{ $count }

## Barra dei comandi

command-open = Comandi
command-bar-label = Barra dei comandi
command-input = Scrivi un comando
command-group-navigate = Vai a
command-group-actions = Azioni
command-empty-title = Nessun risultato
command-empty-body = Prova con il nome di una schermata o di un'azione.
command-hint-move = per muoverti
command-hint-run = per eseguire
command-hint-close = per chiudere
# Etichetta del tasto Esc nei suggerimenti.
key-escape = esc

## Schermate

welcome-title = Benvenuto in { -brand }
home-description = Porta qui meeting, note e documenti: al resto pensa il curatore.
home-empty-title = La tua panoramica parte da qui
home-empty-body = Qui compariranno gli ultimi meeting, le azioni aperte e i progetti attivi.
sources-description = Meeting, note e documenti, tenuti così come li hai portati.
sources-empty-title = Qui arriva tutto quello che porti in { -brand }
sources-empty-body = Inizia scrivendo una nota. L'elenco completo, con filtri e ricerca, arriva in una prossima versione.

## Notes

note-new = Nuova nota
note-title = Nota
note-editor-label = Testo della nota
note-placeholder = Inizia a scrivere. Il Markdown funziona: # per un titolo, - per un elenco.
# Shown next to the editor; notes save on their own.
note-status-idle = Salvataggio automatico
note-status-saving = Salvataggio…
note-status-saved = Salvata
note-status-error = Non ancora salvata: nuovo tentativo in corso.
note-untitled = Nota senza titolo
note-title-label = Titolo della nota
note-title-placeholder = Senza titolo
note-status-saved-at = Salvata alle { $time }
note-mode-label = Vista
note-mode-write = Scrivi
note-mode-split = Affiancato
note-mode-read = Anteprima
note-format-label = Formattazione
note-format-heading = Titolo
note-format-bold = Grassetto
note-format-italic = Corsivo
note-format-bullets = Elenco puntato
note-format-numbers = Elenco numerato
note-format-todo = Cosa da fare
note-format-quote = Citazione
note-format-code = Codice
note-format-link = Collegamento
note-preview-empty = Ancora niente da mostrare. Passa a Scrivi per iniziare.
note-stats = { $count ->
    [one] 1 parola
   *[other] { $count } parole
} · { $minutes } min di lettura
note-stats-empty = Ancora nessuna parola
note-link-failed = Impossibile aprire questo collegamento.
note-load-failed-title = Impossibile aprire questa nota
note-load-failed-body = Potrebbe essere stata spostata nel cestino.
sources-list-label = Fonti
source-kind-note = Nota
# Shown when quitting is cancelled because a note could not be saved.
quit-unsaved = Una nota non è ancora salvata, quindi { -brand } è rimasta aperta. Esci di nuovo entro 30 secondi per uscire comunque.
