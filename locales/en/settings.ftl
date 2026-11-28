# Settings screen (frontend). Error messages are `settings-error-` followed by the code the
# core returns, with `_` written as `-`; provider errors are in providers.ftl.

settings-sections = Settings sections
settings-loading = Loading settings…
settings-section-general = General
settings-section-providers = Providers and models
settings-section-transcription = Transcription
settings-section-recording = Recording
settings-section-data = Data and privacy
settings-section-appearance = Appearance
settings-section-claude = Claude (MCP)
settings-cancel = Cancel
settings-save = Save
settings-edit = Edit

## General

settings-interface-language = Interface language
settings-interface-language-desc = The language of menus, buttons and messages.
settings-language-system = Same as macOS
settings-group-startup = Startup
settings-open-at-login = Open at login
settings-open-at-login-desc = Trama starts in the menu bar, without opening the window, so the shortcut and the menu bar item are ready.
settings-meeting-language = Meeting language
settings-meeting-language-desc = The language your meetings are held in. Transcripts and summaries use it; it is never guessed.
settings-theme = Theme
settings-theme-system = System
settings-theme-light = Light
settings-theme-dark = Dark
settings-accent = Accent colour
settings-accent-desc = The colour of buttons, selections and indicators. Red stays reserved for recording.
settings-accent-preview = Preview
settings-reduce-transparency = Reduce transparency
settings-reduce-transparency-desc = Makes the window opaque, so the desktop no longer shows through.
settings-reduce-transparency-system = On because Reduce transparency is on in macOS (System Settings › Accessibility › Display).
settings-accent-lime = Lime
settings-accent-blue = Blue
settings-accent-purple = Purple
settings-accent-pink = Pink
settings-accent-red = Red
settings-accent-orange = Orange
settings-accent-yellow = Yellow
settings-accent-green = Green
settings-accent-graphite = Graphite
settings-group-accessibility = Accessibility
settings-reduce-motion = Reduce motion
settings-reduce-motion-desc = Turns off the interface's animations and transitions.
settings-reduce-motion-system = On because Reduce motion is on in macOS (System Settings › Accessibility › Display).

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
settings-provider-connected = Connected · { $count ->
    [one] { $count } model
   *[other] { $count } models
}
settings-provider-untested = Not tried yet
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
settings-budget-exceeded-blocked = This month's budget is used up: tasks that need a model fail with an error until you raise it or turn the limit off.
settings-currency = Currency
settings-currency-desc = For prices and the budget. Amounts are not converted.
settings-budget-amount = Monthly budget
settings-budget-amount-desc = Leave empty for no budget.
settings-budget-warn = Warn from
settings-budget-enforce = Stop at the budget
settings-budget-enforce-desc = Refuse calls to a model once the budget is used up, also in the middle of a task, which then fails with an error. It only works with models that have a price: the others are refused.
# $count: models chosen for a task without a price; $models: their ids.
settings-budget-price-missing = { $count ->
    [one] { $models } has no price, so with the limit on its tasks are refused. Set its prices under Providers.
   *[other] { $models } have no price, so with the limit on their tasks are refused. Set their prices under Providers.
}

## Transcription

settings-group-transcription = Transcription model
settings-model-desc = Meetings are transcribed on this Mac: the audio never leaves it. The model is downloaded once.
settings-model-name = Parakeet Ultra
settings-model-spec = fp32 · { $size }
settings-model-fact-languages = 25 languages
settings-model-fact-languages-desc = European, English included
settings-model-fact-local = On the Mac
settings-model-fact-local-desc = Audio never leaves the computer
settings-model-fact-offline = Offline
settings-model-fact-offline-desc = Transcribes without a connection too
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

settings-microphone = Microphone
settings-microphone-desc = Following the system default also follows its changes during a meeting.
settings-microphone-default = System default
settings-microphone-missing = Chosen microphone (not connected)
settings-microphone-refresh = Refresh devices
settings-group-shortcut = Global shortcut
settings-shortcut = Start and stop recording
settings-shortcut-desc = Works from any app, without opening Trama.
settings-shortcut-change = Change shortcut
settings-shortcut-stop = Cancel
settings-shortcut-press = Press the keys…
settings-shortcut-recording = Press the new shortcut, with at least ⌘, ⌥ or ⌃. Press Escape to cancel.
settings-shortcut-reset = Restore default
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
settings-retention-held = { $count ->
    [one] One recording from before this update still has its audio: Trama keeps it until you confirm that the setting applies to it too.
   *[other] { $count } recordings from before this update still have their audio: Trama keeps it until you confirm that the setting applies to them too.
}
settings-retention-held-apply = Apply the setting to them
settings-retention-confirm-title = Delete audio you already have?
settings-retention-confirm-now = { $count ->
    [one] The audio of 1 recording will be deleted now.
   *[other] The audio of { $count } recordings will be deleted now.
}
settings-retention-confirm-later = { $count ->
    [one] The audio of 1 recording will be deleted { $days } days after its transcription.
   *[other] The audio of { $count } recordings will be deleted { $days } days after their transcription.
}
settings-retention-confirm-undo = Transcripts and notes stay. Deleted audio can't be recovered: this can't be undone.
settings-retention-confirm = Delete audio
settings-group-leaves = What leaves the Mac
settings-leaves-desc = Trama sends out only the text a task needs, and only to the provider you chose.
settings-leaves-audio = Audio
settings-leaves-audio-desc = Never. It stays on the Mac and is transcribed here.
settings-leaves-sources = Source text
settings-leaves-sources-desc = Only the passages an answer needs, to the chosen provider.
settings-leaves-index = Data and indexes
settings-leaves-index-desc = Always on the Mac.
settings-group-library = Library
settings-library-desc = Your meetings, notes and their audio, in one folder on this Mac.
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
settings-error-audio-deletion-unconfirmed = Confirm first that the audio you already have can be deleted.
settings-error-config-write = Trama couldn't save the settings on this Mac.
settings-error-name-required = Give the provider a name.
settings-error-url-scheme = The address must start with https:// (or http:// for a local server).
settings-error-url-invalid = This isn't a valid address.
settings-error-url-insecure = Plain http:// works only for servers on this Mac or the local network. Use https://.
settings-error-unsupported-language = This language isn't available.
settings-error-unknown-appearance = This appearance isn't available.
settings-error-invalid-shortcut = Use a key with at least ⌘, ⌥ or ⌃, for example ⌥⌘R.
settings-error-shortcut-reserved = macOS or Trama already uses this shortcut. Choose another, for example with ⌥ or ⌃.
settings-error-shortcut-taken = Trama can't listen for this shortcut: another app may be using it. Choose another.
settings-error-login-item-failed = Couldn't change opening at login. Try again, or check System Settings › General › Login Items.
settings-error-library-not-absolute = Choose a folder on a disk.
settings-error-library-not-a-folder = This isn't a folder.
settings-error-library-folder-not-empty = This folder has other files in it. Choose an empty folder or one that already holds a Trama library.
settings-error-library-synced = This folder is synced by iCloud Drive or another cloud service, which can damage an open library. Choose a folder outside it.
settings-error-library-not-writable = Trama can't write in this folder. Choose one you have permission to change.
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

## Problems found at start ($path: the folder or file)

settings-notice-settings-reset = The settings file couldn't be read, so the defaults are in use. The damaged file is kept next to it as settings.json.corrupt.
settings-notice-settings-unreadable = The settings file couldn't be read, so the defaults are in use. Changes aren't saved, so that the file is not overwritten: check { $path }.
settings-notice-library-unavailable = The library in { $path } couldn't be opened, so Trama opened the default library. Connect its disk or check its permissions, then restart Trama.
settings-notice-library-failed = No library could be opened in { $path }. Check that the disk is connected and that you can write to it, then restart Trama.
