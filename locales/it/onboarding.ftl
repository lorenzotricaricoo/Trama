# Onboarding (flusso F1): il primo avvio, un passo alla volta.

onboarding-steps = Passi della configurazione
onboarding-brand = Configura Trama
onboarding-step-count = Passo { $current } di { $total }
onboarding-step-done = fatto
onboarding-later-mark = Dopo
onboarding-skip = Salta la configurazione
onboarding-skip-hint = La Home ricorda cosa manca e ⌘K riapre la configurazione.
onboarding-start = Iniziamo
onboarding-back = Indietro
onboarding-later = Non ora
onboarding-continue = Continua
onboarding-finish = Inizia a usare Trama

onboarding-step-welcome = Benvenuto
onboarding-step-language = Lingua dei meeting
onboarding-step-permissions = Permessi
onboarding-step-provider = Riassunti
onboarding-step-model = Trascrizione
onboarding-step-everyday = Barra dei menu
onboarding-step-ready = Pronto

onboarding-welcome-title = Benvenuto in Trama
onboarding-welcome-desc = Trama registra i tuoi meeting, li trascrive su questo Mac e li tiene accanto alle tue note. Pochi passi lo preparano per il primo meeting; ognuno può aspettare.
onboarding-welcome-record = Entrambi i lati della chiamata
onboarding-welcome-record-desc = Il tuo microfono e gli altri partecipanti, su due tracce.
onboarding-welcome-local = Trascritto su questo Mac
onboarding-welcome-local-desc = L'audio non lascia mai il computer.
onboarding-welcome-summary = Riassunti dal tuo provider
onboarding-welcome-summary-desc = Un modello locale o uno in cloud, con la tua chiave.
onboarding-welcome-accent = Scegli un colore
onboarding-welcome-accent-desc = Per i pulsanti, le selezioni e la tinta della finestra. Il resto è in Impostazioni › Aspetto.

onboarding-language-title = In che lingua sono i tuoi meeting?
onboarding-language-desc = Una lingua per tutti i meeting: Trama trascrive in questa lingua e ci scrive i riassunti.
onboarding-language-label = Lingua dei meeting
onboarding-language-note = I meeting saranno trascritti in { $language }. Puoi cambiarla in Impostazioni › Trascrizione; la lingua dell'interfaccia è in Impostazioni › Generali.

onboarding-permissions-title = Fai sentire il meeting a Trama
onboarding-permissions-desc = macOS chiede una volta sola per ogni permesso. Trama ascolta solo mentre registri un meeting.
onboarding-mic-title = Microfono
onboarding-mic-desc = La tua voce, sulla traccia “io”.
onboarding-system-title = Audio di sistema
onboarding-system-desc = Gli altri partecipanti, dall'app della chiamata, sulla traccia “altri”.
onboarding-allow-mic = Consenti il microfono
onboarding-allow-system = Consenti l'audio di sistema
onboarding-perm-granted = Consentito
onboarding-perm-asking = In attesa di macOS…
onboarding-perm-checking = Verifica in corso…
onboarding-mic-denied = L'accesso al microfono è disattivato. Attiva Trama in Impostazioni di Sistema › Privacy e sicurezza › Microfono, poi verifica di nuovo.
onboarding-mic-restricted = Una regola di questo Mac blocca il microfono.
onboarding-system-not-heard = Trama non sente ancora l'audio di sistema. Se macOS l'ha chiesto, consenti Trama e verifica di nuovo; altrimenti attivalo in Impostazioni di Sistema › Privacy e sicurezza › Registrazione schermo e audio di sistema.
onboarding-system-probe-note = La verifica riproduce un suono troppo basso per essere sentito e dura circa un secondo.
onboarding-open-settings = Apri Impostazioni di Sistema
onboarding-check-again = Verifica di nuovo
onboarding-permissions-later = Senza microfono Trama non registra la tua voce; senza audio di sistema registra solo te. Quando avvii una registrazione il microfono viene chiesto di nuovo, se serve.

onboarding-provider-title = Scegli chi scrive i riassunti
onboarding-provider-desc = La trascrizione resta su questo Mac. Riassunti e titoli li scrive un modello linguistico: uno locale, come Ollama o LM Studio, o un provider in cloud con la tua chiave, conservata nel Portachiavi.
onboarding-provider-group-desc = Qualsiasi servizio con un'API compatibile con OpenAI. Aggiungine uno, poi scegli il suo modello per i riassunti.
onboarding-provider-later = Senza provider i meeting vengono comunque registrati e trascritti; il riassunto aspetta finché non scegli un modello in Impostazioni › Provider.

onboarding-model-title = Scarica il modello di trascrizione
onboarding-model-desc = Trama trascrive con Parakeet, su questo Mac e anche offline. Il modello si scarica una volta sola, e il download continua in background mentre prosegui.
onboarding-model-later = Senza il modello i meeting vengono registrati ma non trascritti: scaricalo qui o in Impostazioni › Trascrizione, poi trascrivili dalla loro pagina.

onboarding-everyday-title = Sempre a portata di mano, nella barra dei menu
onboarding-everyday-desc = Avvia un meeting, scrivi una nota e segui l'elaborazione dalla barra dei menu. Chiudendo la finestra Trama resta attivo lì; esci dal suo menu o con ⌘Q.
onboarding-menubar-record = Registra meeting
onboarding-menubar-note = Nuova nota
onboarding-menubar-latest = Ultimi meeting
onboarding-shortcut-title = Registra da qualsiasi app
onboarding-login-title = All'accesso

onboarding-ready-title = { $state ->
    [later] Pronto, con qualcosa per dopo
   *[all] Trama è pronto
}
onboarding-ready-desc = Registra il primo meeting con { $shortcut }, con il pulsante in cima alla barra laterale o dalla barra dei menu.
onboarding-ready-later = Quello che manca viene spiegato dove serve, e la Home lo ricorda finché non è configurato.
onboarding-checks = Configurazione
onboarding-check-language = Lingua dei meeting
onboarding-check-microphone = Microfono
onboarding-check-system = Audio di sistema
onboarding-check-provider = Riassunti
onboarding-check-model = Modello di trascrizione
onboarding-check-shortcut = Scorciatoia di registrazione
onboarding-check-allowed = Consentito
onboarding-check-not-allowed = Non ancora consentito
onboarding-check-not-checked = Non verificato
onboarding-check-provider-chosen = Modello scelto
onboarding-check-no-provider = Nessun modello scelto
onboarding-check-model-ready = Scaricato
onboarding-check-model-downloading = Download in corso, { $percent }
onboarding-check-model-missing = Non scaricato
onboarding-set-up = Configura

# Dove un prerequisito mancante conta: Home, barra dei comandi, registrazione.
onboarding-home-title = Completa la configurazione di Trama
onboarding-home-model = Il modello di trascrizione non è scaricato: i meeting non possono ancora essere trascritti.
onboarding-home-provider = Nessun modello scrive ancora i riassunti.
onboarding-home-microphone = Trama non ha ancora il permesso di usare il microfono.
onboarding-home-continue = Continua la configurazione
onboarding-home-hide = Nascondi
onboarding-command = Configura Trama…
onboarding-record-without-model = Il modello di trascrizione non è scaricato: il meeting viene registrato, e potrai trascriverlo dalla sua pagina dopo aver scaricato il modello.
