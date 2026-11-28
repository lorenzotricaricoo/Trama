# Desktop app interface (frontend).

-brand = Trama

## Sidebar

sidebar-label = Sidebar
sidebar-collapse = Collapse sidebar
sidebar-expand = Expand sidebar
nav-label = Main
nav-home = Home
nav-sources = Sources
nav-settings = Settings
record-meeting = Record meeting
record-stop-meeting = Stop recording
# Sidebar button while recording.
record-in-progress-short = Recording · { $clock }
record-show-panel = Show the recording window

## Recording window

record-panel-label = Recording window
record-in-progress = Recording
# Follows the ⌥⌘R key.
record-stop-hint = to stop
record-track-me = Me
record-track-others = Others
record-note-placeholder = Type a note: it sticks to the minute
record-note-label = Note
record-notes-label = Notes
record-note-failed = Note not saved: the text is back in the field, try again.
record-reduce = Reduce
record-expand = Expand
record-stop = Stop
record-stopping = Stopping…
record-notice-microphone-silent = No sound from the microphone: check that it is on and not muted.
record-notice-system-audio-silent = No audio from the others: check the System Audio Recording permission in System Settings.
record-notice-interrupted = Audio device lost: reconnecting, the recording goes on.
record-notice-frames-dropped = The Mac is under load: a few moments of audio may be missing.
record-notice-write-failed = Cannot write the audio: the disk may be full.
record-notice-track-size-limit = The recording reached the longest length of a track.

## Recording errors

record-error-microphone-denied = { -brand } cannot use the microphone: allow it in System Settings → Privacy & Security → Microphone.
record-error-microphone-restricted = The microphone is blocked by a policy on this Mac.
record-error-no-input-device = No microphone connected.
record-error-device-open-failed = Cannot open the audio device. Reconnect it and try again.
record-error-system-audio-denied = { -brand } cannot record the others' audio: allow System Audio Recording in System Settings → Privacy & Security.
record-error-system-audio-unavailable = Cannot record the Mac's audio: no audio output available.
record-error-write-failed = Cannot write the audio: the disk may be full.
record-error-track-size-limit = The recording went past the longest length of a track.
record-error-storage-failed = Cannot save the meeting in the library.
record-error-already-recording = A recording is already running.
record-error-generic = The recording failed ({ $code }).

## Top bar and notices

back = Back
notice-close = Close
job-transcribe = Transcribing
job-summarize = Summarizing
job-delete_audio = Deleting audio
job-queued = Waiting
# Further background jobs after the one shown.
job-more = +{ $count }

## Command bar

command-open = Commands
command-bar-label = Command bar
command-input = Type a command
command-group-navigate = Go to
command-group-actions = Actions
command-group-sources = In your sources
command-empty-title = No results
command-empty-body = Try the name of a screen or an action.
command-hint-move = to move
command-hint-run = to run
command-hint-close = to close
# Label of the Escape key in hint chips.
key-escape = esc

## Screens

welcome-title = Welcome to { -brand }
home-empty-title = Your overview starts here
home-empty-body = Your latest meetings and recent notes will appear here. Start by recording a meeting or writing a note.
sources-description = Meetings, notes and documents, kept as you brought them.
sources-empty-title = Everything you bring into { -brand } lands here
sources-empty-body = Record a meeting, write a note or drop Markdown files here to start.
home-title = Overview
home-latest-meetings = Latest meetings
home-all-sources = All sources
home-no-meetings = No meetings yet.
home-recent-notes = Recent notes
home-no-notes = No notes yet.
sources-search = Search sources
sources-search-clear = Clear the search
sources-filter = Show
sources-filter-all = All
sources-filter-meetings = Meetings
sources-filter-notes = Notes
sources-count = { $count ->
    [one] 1 source
   *[other] { $count } sources
}
sources-results = { $count ->
    [one] 1 result
   *[other] { $count } results
}
sources-results-label = Search results
sources-column-title = TITLE
sources-column-when = WHEN
sources-column-length = LENGTH
sources-none-of-kind = Nothing of this kind yet.
sources-no-results = Nothing found for “{ $query }”
sources-no-results-body = Try another word, or clear the filters.
sources-reset = Clear filters
# A timestamp button next to a passage found in a meeting.
sources-open-at = Open at { $time }: { $text }
sources-more = Show more
# Length of a meeting in lists.
source-duration = { $minutes } min
source-status-recording = Recording
source-status-processing = Processing
source-status-failed = Failed

## Notes

note-new = New note
note-title = Note
note-editor-label = Note text
note-placeholder = Start writing. Markdown works: # for a heading, - for a list.
# Shown next to the editor; notes save on their own.
note-status-idle = Saved automatically
note-status-saving = Saving…
note-status-saved = Saved
note-status-error = Not saved yet: trying again.
note-untitled = Untitled note
note-title-label = Note title
note-title-placeholder = Untitled
note-status-saved-at = Saved at { $time }
note-mode-label = View
note-mode-write = Write
note-mode-split = Side by side
note-mode-read = Preview
note-format-label = Formatting
note-format-heading = Heading
note-format-bold = Bold
note-format-italic = Italic
note-format-bullets = Bulleted list
note-format-numbers = Numbered list
note-format-todo = To-do
note-format-quote = Quote
note-format-code = Code
note-format-link = Link
note-preview-empty = Nothing to preview yet. Switch to Write to start.
note-stats = { $count ->
    [one] 1 word
   *[other] { $count } words
} · { $minutes } min read
note-stats-empty = No words yet
note-front-matter = Properties
note-link-failed = This link can't be opened.
note-load-failed-title = This note can't be opened
note-load-failed-body = It may have been moved to the trash.
sources-list-label = Sources
source-kind-note = Note
# Shown when quitting is cancelled because a note could not be saved.
quit-unsaved = A note is not saved yet, so { -brand } stayed open. Quit again within 30 seconds to quit anyway.

## Import of Markdown files as notes

import-notes = Import notes…
import-dialog-title = Import Markdown files as notes
import-dialog-filter = Markdown files
# Shown over the window while files are dragged onto it.
import-drop-title = Drop to import as notes
import-drop-body = Each Markdown file becomes a note. Folders work too.
import-done-one = Imported “{ $title }”.
import-done = Imported { $count } notes.
import-none = No notes imported.
# A few file names, then how many others: “a.md, b.md and 3 more”.
import-files-more = { $names } and { $count } more
import-skip-not_markdown = { $count ->
    [one] { $files } isn't a Markdown file.
   *[other] { $files } aren't Markdown files.
}
import-skip-not_text = { $count ->
    [one] { $files } isn't UTF-8 text.
   *[other] { $files } aren't UTF-8 text.
}
import-skip-too_large = { $count ->
    [one] { $files } is larger than { $limit }.
   *[other] { $files } are larger than { $limit }.
}
import-skip-unreadable = { $count ->
    [one] { $files } can't be read.
   *[other] { $files } can't be read.
}
import-skip-duplicate = { $count ->
    [one] { $files } is already in { -brand }.
   *[other] { $files } are already in { -brand }.
}
import-skip-too_many = { $count ->
    [one] 1 more file was left out: at most { $max } at a time.
   *[other] { $count } more files were left out: at most { $max } at a time.
}
import-failed = The notes couldn't be imported ({ $code }).

## Meeting

meeting-title = Meeting
source-kind-meeting = Meeting
# Title of a meeting until the summary gives one.
meeting-untitled = Meeting of { $date }
meeting-missing = Meeting not found
meeting-minutes = { $count ->
    [one] 1 minute
   *[other] { $count } minutes
}
meeting-recording = Recording
meeting-show-panel = Show the window
meeting-queued = Waiting for transcription
meeting-transcribing = Transcribing
meeting-summarizing = Summarizing
meeting-retry = Try again
meeting-retry-failed = Cannot try again right now.
meeting-summary = Summary
meeting-summary-none = No summary.
meeting-summary-failed = The summary failed: the transcript is ready anyway.
meeting-summary-generate = Generate summary
meeting-transcript = Transcript
meeting-transcript-pending = The transcript shows here once it is ready.
meeting-note = Note
meeting-title-label = Meeting title
meeting-rename-failed = Cannot rename the meeting right now.
meeting-copy-summary = Copy summary
meeting-summary-copied = Summary copied.
meeting-copy-failed = Cannot copy the summary.
meeting-summary-generated = generated · editable
meeting-summary-edited = edited by you
meeting-edit = Edit
meeting-save = Save
meeting-cancel = Cancel
meeting-summary-body-label = Summary text
meeting-key-points = Key points
meeting-key-point-label = Key point { $n }
meeting-key-point-remove = Remove the key point
meeting-key-point-add = Add a key point
meeting-summary-save-failed = Cannot save the summary right now.
meeting-jump = Go to { $time }
meeting-player = Meeting audio
meeting-play = Play
meeting-pause = Pause
meeting-position = Position
meeting-audio-failed = Cannot play the audio.
meeting-audio-deleted = audio not kept · setting “{ $setting }”
meeting-audio-deleted-plain = audio not kept
meeting-error-model-missing = The transcription model is not downloaded: download it and try again.
meeting-error-no-audio = The meeting has no audio to transcribe.
meeting-error-audio-missing = The meeting's audio is gone.
meeting-error-audio-invalid = The meeting's audio cannot be read.
meeting-error-language-unsupported = The model does not support the meeting's language.
meeting-error-cancelled = Transcription cancelled.
meeting-error-recording-interrupted = The app closed while recording: the audio may be incomplete.
meeting-error-write-failed = The recording was not saved in full: the disk may be full.
meeting-error-generic = The transcription failed ({ $code }).
