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
record-stop-meeting = Ferma la registrazione
# Pulsante della barra laterale durante la registrazione.
record-in-progress-short = In corso · { $clock }
record-show-panel = Mostra la finestra di registrazione

## Finestra di registrazione

record-panel-label = Finestra di registrazione
record-in-progress = Registrazione in corso
# Segue il tasto ⌥⌘R.
record-stop-hint = per fermare
record-track-me = Io
record-track-others = Altri
record-note-placeholder = Scrivi un appunto: si aggancia al minuto
record-note-label = Appunto
record-notes-label = Appunti
record-note-failed = Appunto non salvato: il testo è tornato nel campo, riprova.
record-reduce = Riduci
record-expand = Espandi
record-stop = Stop
record-stopping = Chiudo…
record-notice-microphone-silent = Nessun suono dal microfono: controlla che sia acceso e non silenziato.
record-notice-system-audio-silent = Nessun audio dagli altri: controlla il permesso «Registrazione audio di sistema» in Impostazioni di sistema.
record-notice-interrupted = Dispositivo audio perso: riprovo a collegarlo, la registrazione continua.
record-notice-frames-dropped = Il Mac è sotto sforzo: qualche istante di audio potrebbe mancare.
record-notice-write-failed = Non riesco a scrivere l'audio: il disco potrebbe essere pieno.
record-notice-track-size-limit = La registrazione ha raggiunto la durata massima di una traccia.

## Errori della registrazione

record-error-microphone-denied = { -brand } non può usare il microfono: consentilo in Impostazioni di sistema → Privacy e sicurezza → Microfono.
record-error-microphone-restricted = Il microfono è bloccato da un criterio di questo Mac.
record-error-no-input-device = Nessun microfono collegato.
record-error-device-open-failed = Non riesco ad aprire il dispositivo audio. Ricollegalo e riprova.
record-error-system-audio-denied = { -brand } non può registrare l'audio degli altri: consenti «Registrazione audio di sistema» in Impostazioni di sistema → Privacy e sicurezza.
record-error-system-audio-unavailable = Non riesco a registrare l'audio del Mac: nessuna uscita audio disponibile.
record-error-write-failed = Non riesco a scrivere l'audio: il disco potrebbe essere pieno.
record-error-track-size-limit = La registrazione ha superato la durata massima di una traccia.
record-error-storage-failed = Non riesco a salvare il meeting nella libreria.
record-error-already-recording = Una registrazione è già in corso.
record-error-generic = La registrazione non è riuscita ({ $code }).

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
command-group-sources = Nelle fonti
command-empty-title = Nessun risultato
command-empty-body = Prova con il nome di una schermata o di un'azione.
command-hint-move = per muoverti
command-hint-run = per eseguire
command-hint-close = per chiudere
# Etichetta del tasto Esc nei suggerimenti.
key-escape = esc

## Schermate

welcome-title = Benvenuto in { -brand }
home-empty-title = La tua panoramica parte da qui
home-empty-body = Qui compariranno gli ultimi meeting e le note recenti. Inizia registrando un meeting o scrivendo una nota.
sources-description = Meeting, note e documenti, tenuti così come li hai portati.
sources-empty-title = Qui arriva tutto quello che porti in { -brand }
sources-empty-body = Inizia registrando un meeting, scrivendo una nota o trascinando qui dei file Markdown.
home-title = Panoramica
home-latest-meetings = Ultimi meeting
home-all-sources = Tutte le fonti
home-no-meetings = Ancora nessun meeting.
home-recent-notes = Note recenti
home-no-notes = Ancora nessuna nota.
sources-search = Cerca nelle fonti
sources-search-clear = Cancella la ricerca
sources-filter = Mostra
sources-filter-all = Tutte
sources-filter-meetings = Meeting
sources-filter-notes = Note
sources-count = { $count ->
    [one] 1 fonte
   *[other] { $count } fonti
}
sources-results = { $count ->
    [one] 1 risultato
   *[other] { $count } risultati
}
sources-results-label = Risultati della ricerca
sources-column-title = TITOLO
sources-column-when = QUANDO
sources-column-length = DURATA
sources-none-of-kind = Ancora niente di questo tipo.
sources-no-results = Nessun risultato per «{ $query }»
sources-no-results-body = Prova con un'altra parola o togli i filtri.
sources-reset = Azzera filtri
# Pulsante con il minuto accanto a un passaggio trovato in un meeting.
sources-open-at = Apri a { $time }: { $text }
sources-more = Mostra altre
# Durata di un meeting negli elenchi.
source-duration = { $minutes } min
source-status-recording = In registrazione
source-status-processing = In elaborazione
source-status-failed = Non riuscito

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

## Importazione di file Markdown come note

import-notes = Importa note…
import-dialog-title = Importa file Markdown come note
import-dialog-filter = File Markdown
# Mostrato sopra la finestra mentre ci si trascinano sopra dei file.
import-drop-title = Rilascia per importare come note
import-drop-body = Ogni file Markdown diventa una nota. Anche le cartelle.
import-done-one = Importata “{ $title }”.
import-done = Importate { $count } note.
import-none = Nessuna nota importata.
# Alcuni nomi di file, poi quanti altri: “a.md, b.md e altri 3”.
import-files-more = { $names } e altri { $count }
import-skip-not_markdown = { $count ->
    [one] { $files } non è un file Markdown.
   *[other] { $files } non sono file Markdown.
}
import-skip-not_text = { $count ->
    [one] { $files } non è testo UTF-8.
   *[other] { $files } non sono testo UTF-8.
}
import-skip-too_large = { $count ->
    [one] { $files } supera { $limit }.
   *[other] { $files } superano { $limit }.
}
import-skip-unreadable = { $count ->
    [one] { $files } non si può leggere.
   *[other] { $files } non si possono leggere.
}
import-skip-duplicate = { $count ->
    [one] { $files } è già in { -brand }.
   *[other] { $files } sono già in { -brand }.
}
import-skip-too_many = { $count ->
    [one] Un altro file è stato escluso: al massimo { $max } alla volta.
   *[other] Altri { $count } file sono stati esclusi: al massimo { $max } alla volta.
}
import-failed = Non è stato possibile importare le note ({ $code }).

## Meeting

meeting-title = Meeting
source-kind-meeting = Meeting
# Titolo di un meeting finché il riassunto non ne dà uno.
meeting-untitled = Meeting del { $date }
meeting-missing = Meeting non trovato
meeting-minutes = { $count ->
    [one] 1 minuto
   *[other] { $count } minuti
}
meeting-recording = Registrazione in corso
meeting-show-panel = Mostra la finestra
meeting-queued = In attesa della trascrizione
meeting-transcribing = Trascrizione in corso
meeting-summarizing = Riassunto in corso
meeting-retry = Riprova
meeting-retry-failed = Non riesco a riprovare adesso.
meeting-summary = Riassunto
meeting-summary-none = Nessun riassunto.
meeting-summary-failed = Il riassunto non è riuscito: la trascrizione è comunque pronta.
meeting-summary-generate = Genera riassunto
meeting-transcript = Trascrizione
meeting-transcript-pending = La trascrizione compare qui quando è pronta.
meeting-note = Appunto
meeting-title-label = Titolo del meeting
meeting-rename-failed = Non riesco a rinominare il meeting adesso.
meeting-copy-summary = Copia riassunto
meeting-summary-copied = Riassunto copiato.
meeting-copy-failed = Non riesco a copiare il riassunto.
meeting-summary-generated = generato · modificabile
meeting-summary-edited = modificato da te
meeting-edit = Modifica
meeting-save = Salva
meeting-cancel = Annulla
meeting-summary-body-label = Testo del riassunto
meeting-key-points = Punti chiave
meeting-key-point-label = Punto chiave { $n }
meeting-key-point-remove = Rimuovi il punto
meeting-key-point-add = Aggiungi un punto
meeting-summary-save-failed = Non riesco a salvare il riassunto adesso.
meeting-jump = Vai a { $time }
meeting-player = Audio del meeting
meeting-play = Riproduci
meeting-pause = Pausa
meeting-position = Posizione
meeting-audio-failed = Non riesco a riprodurre l'audio.
meeting-audio-deleted = audio non conservato · impostazione «{ $setting }»
meeting-audio-deleted-plain = audio non conservato
meeting-error-model-missing = Il modello di trascrizione non è scaricato: scaricalo e riprova.
meeting-error-no-audio = Il meeting non ha audio da trascrivere.
meeting-error-audio-missing = L'audio del meeting non c'è più.
meeting-error-audio-invalid = L'audio del meeting non si legge.
meeting-error-language-unsupported = La lingua del meeting non è supportata dal modello.
meeting-error-cancelled = Trascrizione annullata.
meeting-error-recording-interrupted = L'app si è chiusa durante la registrazione: l'audio potrebbe essere incompleto.
meeting-error-write-failed = La registrazione non è stata salvata per intero: il disco potrebbe essere pieno.
meeting-error-generic = La trascrizione non è riuscita ({ $code }).
