# Provider di AI: errori delle chiamate ai provider, per codice restituito dal core.
# Ogni id è `provider-error-` seguito dal codice, con `_` scritto come `-`.

provider-error-timeout = Il provider non ha risposto in tempo.
provider-error-network = Il provider non è raggiungibile. Controlla la connessione e l'indirizzo del provider.
provider-error-rate-limited = Il provider sta limitando le richieste. Trama riproverà più tardi.
provider-error-auth-failed = Il provider ha rifiutato la chiave API. Controlla la chiave nelle Impostazioni.
provider-error-not-found = Il provider non conosce questo modello o indirizzo.
provider-error-bad-request = Il provider ha rifiutato la richiesta.
provider-error-server-error = Il provider ha avuto un errore. Trama riproverà più tardi.
provider-error-invalid-response = Il provider ha inviato una risposta che Trama non sa leggere.
provider-error-truncated = La risposta è stata interrotta al limite di output del modello.
provider-error-content-filtered = Il filtro dei contenuti del provider ha bloccato la risposta.
provider-error-empty-reply = Il modello ha inviato una risposta vuota.
provider-error-invalid-json = La risposta del modello non era nel formato previsto.
provider-error-schema-mismatch = Nella risposta del modello mancavano informazioni richieste.
provider-error-degenerate-output = La risposta del modello era vuota o conteneva solo segnaposto.
provider-error-context-exceeded = Il testo è troppo lungo per questo modello.
provider-error-limits-unknown = La dimensione del contesto del modello è sconosciuta. Impostala nelle Impostazioni.
provider-error-no-model-for-task = Nessun modello scelto per questa attività. Scegline uno nelle Impostazioni.
provider-error-missing-key = Manca la chiave API del provider. Aggiungila nelle Impostazioni.
provider-error-keychain = Trama non è riuscita ad accedere alla chiave API nel Portachiavi.
provider-error-storage = Trama non è riuscita a leggere o salvare le impostazioni dei provider.
provider-error-key-required-for-new-url = L'indirizzo del provider ora punta a un altro server. Inserisci la chiave API per quel server: la chiave salvata non viene mai inviata a un server diverso.
provider-error-budget-exceeded = Il budget di questo mese per i modelli linguistici è esaurito. Aumentalo o disattiva il limite nelle Impostazioni.
