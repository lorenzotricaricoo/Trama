# Schermata Impostazioni (frontend). I messaggi di errore sono `settings-error-` seguito dal
# codice restituito dal core, con `_` scritto come `-`; gli errori dei provider sono in providers.ftl.

settings-sections = Sezioni delle impostazioni
settings-loading = Caricamento delle impostazioni…
settings-section-general = Generali
settings-section-providers = Provider e modelli
settings-section-transcription = Trascrizione
settings-section-recording = Registrazione
settings-section-data = Dati e privacy
settings-section-appearance = Aspetto
settings-section-claude = Claude (MCP)
settings-cancel = Annulla
settings-save = Salva
settings-edit = Modifica

## Generali

settings-interface-language = Lingua dell'interfaccia
settings-interface-language-desc = La lingua di menu, pulsanti e messaggi.
settings-language-system = Come macOS
settings-meeting-language = Lingua delle riunioni
settings-meeting-language-desc = La lingua in cui si svolgono le tue riunioni. Trascrizioni e riassunti la usano; non viene mai indovinata.
settings-theme = Tema
settings-theme-system = Sistema
settings-theme-light = Chiaro
settings-theme-dark = Scuro
settings-accent = Colore di accento
settings-accent-desc = Il colore di pulsanti, selezioni e indicatori. Il rosso resta riservato alla registrazione.
settings-accent-preview = Anteprima
settings-reduce-transparency = Riduci trasparenza
settings-reduce-transparency-desc = Rende la finestra opaca, così la scrivania non si vede più attraverso.
settings-reduce-transparency-system = Attiva perché Riduci trasparenza è attivo in macOS (Impostazioni di Sistema › Accessibilità › Schermo).
settings-accent-lime = Lime
settings-accent-blue = Blu
settings-accent-purple = Viola
settings-accent-pink = Rosa
settings-accent-red = Rosso
settings-accent-orange = Arancione
settings-accent-yellow = Giallo
settings-accent-green = Verde
settings-accent-graphite = Grafite
settings-group-accessibility = Accessibilità
settings-reduce-motion = Riduci movimento
settings-reduce-motion-desc = Toglie animazioni e transizioni dell'interfaccia.
settings-reduce-motion-system = Attivo perché Riduci movimento è attivo in macOS (Impostazioni di Sistema › Accessibilità › Schermo).

## Provider e modelli

settings-group-providers = Provider
settings-providers-desc = Qualsiasi servizio compatibile con OpenAI, gateway aziendale o server locale. Le chiavi API restano nel Portachiavi di macOS e non vengono più mostrate.
settings-providers-empty = Nessun provider. Aggiungine uno per generare i riassunti.
settings-provider-add = Aggiungi provider
settings-provider-add-confirm = Aggiungi
settings-provider-test = Prova la connessione
settings-provider-testing = Prova in corso…
# $count: modelli nel catalogo.
settings-provider-test-ok = Connesso: { $count ->
    [one] un modello disponibile.
   *[other] { $count } modelli disponibili.
}
settings-provider-edit = Modifica { $name }
settings-provider-remove = Rimuovi { $name }
settings-provider-remove-title = Rimuovere { $name }?
settings-provider-remove-body = La sua chiave API viene cancellata dal Portachiavi e le attività che usano uno dei suoi modelli restano senza modello. Quanto hanno speso finora i modelli resta nella spesa del mese.
settings-provider-remove-confirm = Rimuovi provider
settings-provider-dialog-add = Aggiungi un provider
settings-provider-dialog-edit = Modifica { $name }
settings-provider-name = Nome
settings-provider-url = Indirizzo
settings-provider-url-desc = L'URL di base dell'API, per esempio https://api.example.com/v1. Il semplice http:// funziona solo per server su questo Mac o sulla rete locale.
settings-provider-needs-key = Questo provider richiede una chiave API
settings-provider-key = Chiave API
settings-provider-key-desc = Salvata nel Portachiavi di macOS.
settings-provider-key-replace = Una chiave è salvata nel Portachiavi. Scrivine una nuova solo per sostituirla. Un nuovo indirizzo su un altro server richiede una nuova chiave.
settings-key-none = Nessuna chiave richiesta
settings-key-stored = Chiave nel Portachiavi
settings-key-missing = Chiave mancante
settings-provider-connected = Connesso · { $count ->
    [one] { $count } modello
   *[other] { $count } modelli
}
settings-provider-untested = Non ancora provato
# $count: modelli; $date: ultima lettura del catalogo.
settings-catalog-fetched = { $count ->
    [one] Un modello
   *[other] { $count } modelli
}, letto { $date }
settings-models = { $count ->
    [0] Modelli
    [one] Un modello
   *[other] { $count } modelli
}
settings-models-list = Modelli
settings-models-empty = Nessun modello. Prova la connessione per leggere il catalogo, o aggiungi un modello a mano.
settings-model-add = Aggiungi modello a mano
settings-model-edit = Modifica { $model }
settings-model-limits = Contesto { $context } · risposta { $output } token
settings-model-context-unknown = Dimensione del contesto sconosciuta
settings-model-unlisted = Non nel catalogo
settings-model-prices = { $input } in ingresso · { $output } in uscita per milione di token
settings-model-no-price = Nessun prezzo
settings-model-dialog-add = Aggiungi un modello
settings-model-dialog-edit = { $model }
settings-model-id = Id del modello
settings-model-context = Finestra di contesto (token)
settings-model-output = Risposta più lunga (token)
settings-model-from-catalog = { $value } nel catalogo
settings-model-input-price = Prezzo per milione di token di prompt
settings-model-output-price = Prezzo per milione di token di risposta
settings-model-dialog-hint = Lascia vuoto un campo per usare il valore del catalogo, o per lasciare il prezzo sconosciuto. I prezzi servono solo a tenere traccia dei costi.

settings-group-tasks = Modello per attività
settings-tasks-desc = Ogni attività usa il modello che scegli per lei.
settings-task-summary = Riassunti
settings-task-summary-desc = Titolo, riassunto e punti chiave di ogni riunione. Basta un modello piccolo ed economico.
settings-task-no-model = Nessun modello
settings-task-model-option = { $provider } › { $model }

settings-group-budget = Consumi e budget
settings-budget-desc = Quanto costano i modelli linguistici questo mese, secondo i prezzi che imposti. Il mese inizia il giorno 1 (UTC).
settings-budget-month = Questo mese
settings-budget-spent-of = { $spent } di { $budget }
settings-budget-usage = { $requests ->
    [one] Una richiesta
   *[other] { $requests } richieste
}, { $input } token di prompt e { $output } di risposta
settings-budget-unpriced = { $count ->
    [one] Una richiesta ha usato un modello senza prezzo, quindi la spesa reale è più alta.
   *[other] { $count } richieste hanno usato modelli senza prezzo, quindi la spesa reale è più alta.
}
settings-budget-warning = Hai usato più del { $percent }% del budget di questo mese.
settings-budget-exceeded = Il budget di questo mese è esaurito. Le attività continuano perché il limite è disattivato.
settings-budget-exceeded-blocked = Il budget di questo mese è esaurito: le attività che richiedono un modello falliscono con un errore finché non lo aumenti o disattivi il limite.
settings-currency = Valuta
settings-currency-desc = Per prezzi e budget. Gli importi non vengono convertiti.
settings-budget-amount = Budget mensile
settings-budget-amount-desc = Lascia vuoto per non avere un budget.
settings-budget-warn = Avvisa dal
settings-budget-enforce = Fermati al budget
settings-budget-enforce-desc = Rifiuta le chiamate a un modello quando il budget è esaurito, anche a metà di un'attività, che allora fallisce con un errore. Funziona solo con modelli che hanno un prezzo: gli altri vengono rifiutati.
# $count: modelli scelti per un'attività senza prezzo; $models: i loro id.
settings-budget-price-missing = { $count ->
    [one] { $models } non ha un prezzo, quindi con il limite attivo le sue attività vengono rifiutate. Imposta i prezzi in Provider.
   *[other] { $models } non hanno un prezzo, quindi con il limite attivo le loro attività vengono rifiutate. Imposta i prezzi in Provider.
}

## Trascrizione

settings-group-transcription = Modello di trascrizione
settings-model-desc = Le riunioni vengono trascritte su questo Mac: l'audio non lo lascia mai. Il modello si scarica una volta sola.
settings-model-name = Parakeet Ultra
settings-model-spec = fp32 · { $size }
settings-model-fact-languages = 25 lingue
settings-model-fact-languages-desc = Europee, compreso l'italiano
settings-model-fact-local = Sul Mac
settings-model-fact-local-desc = L'audio non lascia mai il computer
settings-model-fact-offline = Offline
settings-model-fact-offline-desc = Trascrive anche senza connessione
settings-model-state-missing = Non scaricato
settings-model-state-downloading = Download in corso
settings-model-state-verifying = Verifica in corso
settings-model-state-ready = Pronto
settings-model-state-corrupt = Danneggiato
settings-model-downloading = Download in corso
settings-model-verifying = Verifica dei file
settings-model-progress = { $done } di { $total }
settings-model-download = Scarica
settings-model-resume = Riprendi il download
settings-model-download-again = Scarica di nuovo
settings-model-pause = Metti in pausa
settings-model-cancel-check = Interrompi la verifica
settings-model-verify = Verifica i file
settings-model-remove = Rimuovi
settings-model-remove-title = Rimuovere il modello di trascrizione?
settings-model-remove-body = Le nuove riunioni non potranno essere trascritte finché non lo scarichi di nuovo (circa 2,5 GB). Le trascrizioni che hai già restano.
settings-model-remove-confirm = Rimuovi modello
settings-model-credit = Parakeet Ultra di Moondream, basato su NVIDIA Parakeet TDT 0.6B v3, con licenza CC BY 4.0.

## Registrazione

settings-microphone = Microfono
settings-microphone-desc = Seguire quello di sistema segue anche i suoi cambi durante una riunione.
settings-microphone-default = Predefinito di sistema
settings-microphone-missing = Microfono scelto (non collegato)
settings-microphone-refresh = Aggiorna i dispositivi
settings-group-shortcut = Scorciatoia globale
settings-shortcut = Avvia e ferma la registrazione
settings-shortcut-desc = Funziona da qualsiasi app, senza aprire Trama.
settings-shortcut-change = Cambia scorciatoia
settings-shortcut-stop = Annulla
settings-shortcut-press = Premi i tasti…
settings-shortcut-recording = Premi la nuova scorciatoia, con almeno ⌘, ⌥ o ⌃. Premi Esc per annullare.
settings-shortcut-reset = Ripristina predefinita
settings-shortcut-note = Trama ascolta questa scorciatoia quando potrà funzionare dalla barra dei menu.
settings-key-space = Spazio

## Dati e privacy

settings-group-retention = Conservazione dell'audio
settings-retention-desc = Trama conserva la trascrizione; l'audio è facoltativo.
settings-retention = Conserva l'audio delle riunioni
settings-retention-never = Mai
settings-retention-never-desc = Cancellato appena la trascrizione riesce.
settings-retention-7_days = 7 giorni
settings-retention-7_days-desc = Per riascoltare o controllare la trascrizione per una settimana.
settings-retention-30_days = 30 giorni
settings-retention-30_days-desc = Conservato per un mese dopo la trascrizione.
settings-retention-always = Sempre
settings-retention-always-desc = Conservato finché non cancelli la riunione. Occupa più spazio.
settings-retention-note = Senza audio una riunione non può essere trascritta di nuovo con un motore migliore. Trama chiede sempre prima di cancellare l'audio che hai già.
settings-group-leaves = Cosa esce dal Mac
settings-leaves-desc = Trama manda fuori solo il testo che serve a un compito, e solo al provider che hai scelto.
settings-leaves-audio = Audio
settings-leaves-audio-desc = Mai. Resta sul Mac e viene trascritto qui.
settings-leaves-sources = Testo delle fonti
settings-leaves-sources-desc = Solo i passaggi che servono a una risposta, al provider scelto.
settings-leaves-index = Dati e indici
settings-leaves-index-desc = Sempre sul Mac.
settings-group-library = Libreria
settings-library-desc = Le tue riunioni, le note e il loro audio, in una cartella su questo Mac.
settings-library-reveal = Mostra nel Finder
settings-library-change = Cambia…
settings-library-default = Usa la posizione predefinita
settings-library-restart = Al prossimo avvio Trama apre la libreria in { $path }. La libreria attuale non viene spostata.
settings-library-overridden = La posizione è impostata da TRAMA_LIBRARY per lo sviluppo.
settings-library-confirm-title = Usare questa cartella per la libreria?
settings-library-confirm-body = Trama apre qui la libreria al prossimo avvio. Niente viene spostato o cancellato: per portare la libreria attuale, esci da Trama e sposta tu la sua cartella qui.
settings-library-confirm = Usa questa cartella
settings-warning-library-maybe-synced = Questa cartella potrebbe essere sincronizzata da iCloud Drive, che può danneggiare una libreria aperta. Una cartella fuori da Documenti e Scrivania è più sicura.
settings-warning-library-exists = Questa cartella contiene già una libreria di Trama: verrà aperta così com'è.

## Errori, per codice

settings-error-internal = Qualcosa è andato storto. Riprova.
settings-error-storage = Trama non è riuscita a leggere o salvare le impostazioni nella libreria.
settings-error-invalid-setting = Questo valore non può essere salvato.
settings-error-config-write = Trama non è riuscita a salvare le impostazioni su questo Mac.
settings-error-name-required = Dai un nome al provider.
settings-error-url-scheme = L'indirizzo deve iniziare con https:// (o http:// per un server locale).
settings-error-url-invalid = Questo non è un indirizzo valido.
settings-error-url-insecure = Il semplice http:// funziona solo per server su questo Mac o sulla rete locale. Usa https://.
settings-error-unsupported-language = Questa lingua non è disponibile.
settings-error-unknown-appearance = Questo aspetto non è disponibile.
settings-error-invalid-shortcut = Usa un tasto con almeno ⌘, ⌥ o ⌃, per esempio ⌥⌘R.
settings-error-library-not-absolute = Scegli una cartella su un disco.
settings-error-library-not-a-folder = Questa non è una cartella.
settings-error-library-folder-not-empty = Questa cartella contiene altri file. Scegli una cartella vuota o una che contiene già una libreria di Trama.
settings-error-library-synced = Questa cartella è sincronizzata da iCloud Drive o da un altro servizio cloud, che può danneggiare una libreria aperta. Scegline una fuori.
settings-error-library-not-writable = Trama non può scrivere in questa cartella. Scegline una che hai il permesso di modificare.
settings-error-model-not-enough-space = Non c'è abbastanza spazio libero sul disco per il modello.
settings-error-model-network = Il download si è interrotto. Controlla la connessione e riprendilo.
settings-error-model-http-status = Il server del download ha rifiutato la richiesta. Riprova più tardi.
settings-error-model-size-mismatch = Un file scaricato ha la dimensione sbagliata. Scaricalo di nuovo.
settings-error-model-checksum-mismatch = Un file scaricato è danneggiato. Scaricalo di nuovo.
settings-error-model-cancelled = Il download è stato interrotto.
settings-error-model-io = Trama non è riuscita a scrivere i file del modello.
settings-error-model-panicked = Il download si è fermato per un errore di Trama.
settings-error-capture-no-input-device = Nessun microfono collegato.
settings-error-capture-device-open-failed = L'elenco dei microfoni non si può leggere ora.

## Problemi trovati all'avvio ($path: la cartella o il file)

settings-notice-settings-reset = Non è stato possibile leggere il file delle impostazioni, quindi sono in uso quelle predefinite. Il file danneggiato è conservato accanto come settings.json.corrupt.
settings-notice-settings-unreadable = Non è stato possibile leggere il file delle impostazioni, quindi sono in uso quelle predefinite. Le modifiche non vengono salvate, per non sovrascrivere il file: controlla { $path }.
settings-notice-library-unavailable = Non è stato possibile aprire la libreria in { $path }, quindi Trama ha aperto quella predefinita. Collega il suo disco o controlla i permessi, poi riavvia Trama.
settings-notice-library-failed = Non è stato possibile aprire nessuna libreria in { $path }. Controlla che il disco sia collegato e che tu possa scriverci, poi riavvia Trama.
