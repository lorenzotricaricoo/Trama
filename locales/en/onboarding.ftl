# Onboarding (flow F1): the first launch, one step at a time.

onboarding-steps = Setup steps
onboarding-brand = Set up Trama
onboarding-step-count = Step { $current } of { $total }
onboarding-step-done = done
onboarding-later-mark = Later
onboarding-skip = Skip setup
onboarding-skip-hint = Home keeps a reminder of what's missing, and ⌘K reopens the setup.
onboarding-start = Get started
onboarding-back = Back
onboarding-later = Not now
onboarding-continue = Continue
onboarding-finish = Start using Trama

onboarding-step-welcome = Welcome
onboarding-step-language = Meeting language
onboarding-step-permissions = Permissions
onboarding-step-provider = Summaries
onboarding-step-model = Transcription
onboarding-step-everyday = Menu bar
onboarding-step-ready = Ready

onboarding-welcome-title = Welcome to Trama
onboarding-welcome-desc = Trama records your meetings, transcribes them on this Mac and keeps them next to your notes. A few steps get it ready for the first meeting; any of them can wait.
onboarding-welcome-record = Both sides of the call
onboarding-welcome-record-desc = Your microphone and the other participants, on two tracks.
onboarding-welcome-local = Transcribed on this Mac
onboarding-welcome-local-desc = The audio never leaves your computer.
onboarding-welcome-summary = Summaries by your provider
onboarding-welcome-summary-desc = A local model or a cloud one, with your own key.
onboarding-welcome-accent = Pick a colour
onboarding-welcome-accent-desc = For buttons, selections and the tint of the window. Settings › Appearance has the rest.

onboarding-language-title = What language are your meetings in?
onboarding-language-desc = One language for every meeting: Trama transcribes in it and writes the summaries in it.
onboarding-language-label = Meeting language
onboarding-language-note = Meetings will be transcribed in { $language }. You can change it in Settings › Transcription; the language of the interface is in Settings › General.

onboarding-permissions-title = Let Trama hear the meeting
onboarding-permissions-desc = macOS asks once for each permission. Trama listens only while you record a meeting.
onboarding-mic-title = Microphone
onboarding-mic-desc = Your voice, on the “me” track.
onboarding-system-title = System audio
onboarding-system-desc = The other participants, from the call app, on the “others” track.
onboarding-allow-mic = Allow microphone
onboarding-allow-system = Allow system audio
onboarding-perm-granted = Allowed
onboarding-perm-asking = Waiting for macOS…
onboarding-perm-checking = Checking…
onboarding-mic-denied = Microphone access is off. Turn on Trama in System Settings › Privacy & Security › Microphone, then check again.
onboarding-mic-restricted = A policy on this Mac blocks the microphone.
onboarding-system-not-heard = Trama couldn't hear the system audio yet. If macOS asked, allow Trama and check again; otherwise turn it on in System Settings › Privacy & Security › Screen & System Audio Recording.
onboarding-system-probe-note = The check plays a sound too quiet to hear and takes about a second.
onboarding-open-settings = Open System Settings
onboarding-check-again = Check again
onboarding-permissions-later = Without the microphone Trama can't record your voice; without system audio it records only you. Starting a recording asks for the microphone again if needed.

onboarding-provider-title = Choose who writes the summaries
onboarding-provider-desc = Transcription stays on this Mac. Summaries and titles come from a language model: a local one, such as Ollama or LM Studio, or a cloud provider with your key, kept in the Keychain.
onboarding-provider-group-desc = Any service with an OpenAI-compatible API. Add one, then choose its model for summaries.
onboarding-provider-later = Without a provider, meetings are still recorded and transcribed; the summary waits until a model is chosen in Settings › Providers.

onboarding-model-title = Download the transcription model
onboarding-model-desc = Trama transcribes with Parakeet, on this Mac and even offline. The model is downloaded once, and the download goes on in the background while you continue.
onboarding-model-later = Without the model, meetings are recorded but not transcribed: download it here or in Settings › Transcription, then transcribe them from their page.

onboarding-everyday-title = Always at hand, in the menu bar
onboarding-everyday-desc = Start a meeting, write a note and follow the processing from the menu bar. Closing the window keeps Trama running there; quit from its menu or with ⌘Q.
onboarding-menubar-record = Record meeting
onboarding-menubar-note = New note
onboarding-menubar-latest = Latest meetings
onboarding-shortcut-title = Record from anywhere
onboarding-login-title = At login

onboarding-ready-title = { $state ->
    [later] Ready, with a few things for later
   *[all] Trama is ready
}
onboarding-ready-desc = Record the first meeting with { $shortcut }, with the button at the top of the sidebar or from the menu bar.
onboarding-ready-later = What's missing is explained where it's needed, and Home keeps a reminder until it's set up.
onboarding-ready-import = Already have notes in Markdown? Drop the files on the window, or use File › Import Notes… ({ $shortcut }).
onboarding-checks = Setup
onboarding-check-language = Meeting language
onboarding-check-microphone = Microphone
onboarding-check-system = System audio
onboarding-check-provider = Summaries
onboarding-check-model = Transcription model
onboarding-check-shortcut = Recording shortcut
onboarding-check-allowed = Allowed
onboarding-check-not-allowed = Not allowed yet
onboarding-check-not-checked = Not checked
onboarding-check-provider-chosen = Model chosen
onboarding-check-no-provider = No model chosen
onboarding-check-model-ready = Downloaded
onboarding-check-model-downloading = Downloading, { $percent }
onboarding-check-model-missing = Not downloaded
onboarding-set-up = Set up

# Where a missing prerequisite matters: Home, the command bar, recording.
onboarding-home-title = Finish setting up Trama
onboarding-home-model = The transcription model isn't downloaded: meetings can't be transcribed yet.
onboarding-home-provider = No model writes the summaries yet.
onboarding-home-microphone = Trama isn't allowed to use the microphone yet.
onboarding-home-continue = Continue setup
onboarding-home-hide = Hide
onboarding-command = Set up Trama…
onboarding-record-without-model = The transcription model isn't downloaded: this meeting is recorded, and you can transcribe it from its page once the model is downloaded.
