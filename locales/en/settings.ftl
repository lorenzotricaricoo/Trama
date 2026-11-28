# Settings screen (frontend). Error messages are `settings-error-` followed by the code the
# core returns, with `_` written as `-`; provider errors are in providers.ftl.

settings-sections = Settings sections
settings-loading = Loading settings…
settings-section-general = General
settings-section-providers = Providers and models
settings-section-transcription = Transcription
settings-section-recording = Recording
settings-section-data = Data and privacy
settings-section-claude = Claude (MCP)
settings-cancel = Cancel
settings-save = Save

## General

settings-group-language = Language
settings-interface-language = Interface language
settings-interface-language-desc = The language of menus, buttons and messages.
settings-language-system = Same as macOS
settings-meeting-language = Meeting language
settings-meeting-language-desc = The language your meetings are held in. Transcripts and summaries use it; it is never guessed.
settings-group-appearance = Appearance
settings-theme = Theme
settings-theme-system = System
settings-theme-light = Light
settings-theme-dark = Dark
settings-accent = Accent colour
settings-accent-lime = Lime
settings-accent-blue = Blue
settings-accent-purple = Purple
settings-accent-pink = Pink
settings-accent-red = Red
settings-accent-orange = Orange
settings-accent-yellow = Yellow
settings-accent-green = Green
settings-accent-graphite = Graphite

## Providers and models

settings-group-providers = Providers
settings-providers-desc = Any OpenAI-compatible service, company gateway or local server. API keys are kept in the macOS Keychain and never shown again.
settings-providers-empty = No provider yet. Add one to generate summaries.
settings-provider-add = Add provider
settings-provider-add-confirm = Add
settings-provider-test = Test connection
settings-provider-testing = Testing…
# $count: models in the catalogue.
settings-provider-test-ok = Connected: { $count ->
    [one] one model available.
   *[other] { $count } models available.
}
settings-provider-edit = Edit { $name }
settings-provider-remove = Remove { $name }
settings-provider-remove-title = Remove { $name }?
settings-provider-remove-body = Its API key is deleted from the Keychain, and tasks that use one of its models are left without a model. What models cost so far stays in this month's spend.
settings-provider-remove-confirm = Remove provider
settings-provider-dialog-add = Add a provider
settings-provider-dialog-edit = Edit { $name }
settings-provider-name = Name
settings-provider-url = Address
settings-provider-url-desc = The base URL of the API, for example https://api.example.com/v1. Plain http:// works only for servers on this Mac or the local network.
settings-provider-needs-key = This provider needs an API key
settings-provider-key = API key
settings-provider-key-desc = Saved in the macOS Keychain.
settings-provider-key-replace = A key is saved in the Keychain. Type a new one only to replace it. A new address on another server needs a new key.
settings-key-none = No key needed
settings-key-stored = Key in the Keychain
settings-key-missing = Key missing
# $count: models; $date: last catalogue read.
settings-catalog-fetched = { $count ->
    [one] One model
   *[other] { $count } models
}, read { $date }
settings-models = { $count ->
    [0] Models
    [one] One model
   *[other] { $count } models
}
settings-models-list = Models
settings-models-empty = No models yet. Test the connection to read the catalogue, or add a model by hand.
settings-model-add = Add model by hand
settings-model-edit = Edit { $model }
settings-model-limits = Context { $context } · reply { $output } tokens
settings-model-context-unknown = Context size unknown
settings-model-unlisted = Not in the catalogue
settings-model-prices = { $input } in · { $output } out per million tokens
settings-model-no-price = No price
settings-model-dialog-add = Add a model
settings-model-dialog-edit = { $model }
settings-model-id = Model id
settings-model-context = Context window (tokens)
settings-model-output = Longest reply (tokens)
settings-model-from-catalog = { $value } in the catalogue
settings-model-input-price = Price per million prompt tokens
settings-model-output-price = Price per million reply tokens
settings-model-dialog-hint = Leave a field empty to use the catalogue's value, or to leave the price unknown. Prices are only used to track costs.

settings-group-tasks = Model per task
settings-tasks-desc = Each task uses the model you choose for it.
settings-task-summary = Summaries
settings-task-summary-desc = Title, summary and key points of each meeting. A small, cheap model is enough.
settings-task-no-model = No model
settings-task-model-option = { $provider } › { $model }

settings-group-budget = Usage and budget
settings-budget-desc = What language models cost this month, from the prices you set. The month starts on the 1st (UTC).
settings-budget-month = This month
settings-budget-spent-of = { $spent } of { $budget }
settings-budget-usage = { $requests ->
    [one] One request
   *[other] { $requests } requests
}, { $input } prompt and { $output } reply tokens
settings-budget-unpriced = { $count ->
    [one] One request used a model without a price, so the real spend is higher.
   *[other] { $count } requests used models without a price, so the real spend is higher.
}
settings-budget-warning = You have used more than { $percent }% of this month's budget.
settings-budget-exceeded = This month's budget is used up. Tasks still run because the limit is off.
settings-budget-exceeded-blocked = This month's budget is used up: tasks that need a model wait until you raise it or turn the limit off.
settings-currency = Currency
settings-currency-desc = For prices and the budget. Amounts are not converted.
settings-budget-amount = Monthly budget
settings-budget-amount-desc = Leave empty for no budget.
settings-budget-warn = Warn from
settings-budget-enforce = Stop at the budget
settings-budget-enforce-desc = Refuse new tasks that need a model once the budget is used up. A task already running finishes.

## Transcription

settings-group-transcription = Transcription model
settings-model-desc = Meetings are transcribed on this Mac: the audio never leaves it. The model is downloaded once.
settings-model-name = Parakeet Ultra
settings-model-size = Download of { $size }
settings-model-state-missing = Not downloaded
settings-model-state-downloading = Downloading
settings-model-state-verifying = Checking
settings-model-state-ready = Ready
settings-model-state-corrupt = Damaged
settings-model-downloading = Downloading
settings-model-verifying = Checking the files
settings-model-progress = { $done } of { $total }
settings-model-download = Download
settings-model-resume = Resume download
settings-model-download-again = Download again
settings-model-pause = Pause
settings-model-cancel-check = Stop the check
settings-model-verify = Check the files
settings-model-remove = Remove
settings-model-remove-title = Remove the transcription model?
settings-model-remove-body = New meetings can't be transcribed until you download it again (about 2.5 GB). Transcripts you already have are kept.
settings-model-remove-confirm = Remove model
settings-model-credit = Parakeet Ultra by Moondream, based on NVIDIA Parakeet TDT 0.6B v3, licensed under CC BY 4.0.

## Recording

settings-group-devices = Devices
settings-devices-desc = Your voice is recorded from the microphone, the other participants from what the Mac plays.
settings-microphone = Microphone
settings-microphone-desc = Following the system default also follows its changes during a meeting.
settings-microphone-default = System default
settings-microphone-default-named = System default ({ $name })
settings-microphone-missing = Chosen microphone (not connected)
settings-microphone-refresh = Refresh devices
settings-group-shortcut = Global shortcut
settings-shortcut = Start and stop recording
settings-shortcut-desc = Works from any app, without opening Trama.
settings-shortcut-change = Change shortcut
settings-shortcut-stop = Cancel
settings-shortcut-recording = Press the new shortcut, with at least ⌘, ⌥ or ⌃. Press Escape to cancel.
settings-shortcut-reset = Restore default
settings-shortcut-note = Trama listens for this shortcut once it can run from the menu bar.
settings-key-space = Space

## Data and privacy

settings-group-retention = Audio retention
settings-retention-desc = The transcript is what Trama keeps; the audio is optional.
settings-retention = Keep meeting audio
settings-retention-never = Never
settings-retention-never-desc = Deleted as soon as the transcription succeeds.
settings-retention-7_days = 7 days
settings-retention-7_days-desc = Lets you listen again or check the transcript for a week.
settings-retention-30_days = 30 days
settings-retention-30_days-desc = Kept for a month after the transcription.
settings-retention-always = Always
settings-retention-always-desc = Kept until you delete the meeting. Uses the most space.
settings-retention-note = Without audio a meeting can't be transcribed again with a better engine. Trama always asks before deleting audio you already have.
settings-group-library = Library
settings-library-desc = Your meetings, notes and their audio, in one folder on this Mac.
settings-library-location = Location
settings-library-reveal = Show in Finder
settings-library-change = Change…
settings-library-default = Use the default location
settings-library-restart = At the next start Trama opens the library in { $path }. The current library is not moved.
settings-library-overridden = The location is set by TRAMA_LIBRARY for development.
settings-library-confirm-title = Use this folder for the library?
settings-library-confirm-body = Trama opens the library here at the next start. Nothing is moved or deleted: to bring your current library, quit Trama and move its folder here yourself.
settings-library-confirm = Use this folder
settings-warning-library-maybe-synced = This folder may be synced by iCloud Drive, which can damage an open library. A folder outside Documents and Desktop is safer.
settings-warning-library-exists = This folder already holds a Trama library: it will be opened as it is.

## Errors, by code

settings-error-internal = Something went wrong. Try again.
settings-error-storage = Trama couldn't read or save the settings in the library.
settings-error-invalid-setting = This value can't be saved.
settings-error-config-write = Trama couldn't save the settings on this Mac.
settings-error-name-required = Give the provider a name.
settings-error-url-scheme = The address must start with https:// (or http:// for a local server).
settings-error-url-invalid = This isn't a valid address.
settings-error-url-insecure = Plain http:// works only for servers on this Mac or the local network. Use https://.
settings-error-unsupported-language = This language isn't available.
settings-error-unknown-appearance = This appearance isn't available.
settings-error-invalid-shortcut = Use a key with at least ⌘, ⌥ or ⌃, for example ⌥⌘R.
settings-error-library-not-absolute = Choose a folder on a disk.
settings-error-library-not-a-folder = This isn't a folder.
settings-error-library-folder-not-empty = This folder has other files in it. Choose an empty folder or one that already holds a Trama library.
settings-error-library-synced = This folder is synced by iCloud Drive or another cloud service, which can damage an open library. Choose a folder outside it.
settings-error-model-not-enough-space = There isn't enough free space on the disk for the model.
settings-error-model-network = The download was interrupted. Check the connection and resume it.
settings-error-model-http-status = The download server refused the request. Try again later.
settings-error-model-size-mismatch = A downloaded file has the wrong size. Download it again.
settings-error-model-checksum-mismatch = A downloaded file is damaged. Download it again.
settings-error-model-cancelled = The download was stopped.
settings-error-model-io = Trama couldn't write the model files.
settings-error-model-panicked = The download stopped because of an error in Trama.
settings-error-capture-no-input-device = No microphone is connected.
settings-error-capture-device-open-failed = The microphone list can't be read right now.
