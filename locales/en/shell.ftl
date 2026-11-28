# Texts of the desktop shell shown outside the app windows: window titles and system
# notifications. The `-brand` term comes from `menu.ftl`.

recording-panel-title = { -brand } — Recording
meeting-ready-title = Meeting ready
# Shown when the meeting has no title yet.
meeting-ready-body = The transcript and the summary are ready.
meeting-failed-title = Transcription failed
meeting-failed-body = Open the meeting in { -brand } to try again.

## Menu bar item. Record, New Note, Settings and Quit use the labels of menu.ftl.

tray-tooltip = { -brand }
tray-tooltip-recording = { -brand } — Recording
tray-open = Open { -brand }
tray-processing = Processing
# A meeting being processed: its title, then what is happening, for example
# "Weekly sync — Transcribing 62%".
tray-processing-row = { $title } — { $status }
tray-transcribing = Transcribing
tray-transcribing-percent = Transcribing { $percent }%
tray-summarizing = Summarizing
tray-waiting = Waiting
tray-retrying = Will try again
tray-latest-meetings = Latest Meetings
tray-no-meetings = No meetings yet
tray-meeting-untitled = Untitled meeting

## Confirmation when quitting during a recording or while a meeting is processed.

quit-recording-title = Stop the recording and quit?
quit-recording-body = The meeting recorded so far is kept, and transcribed the next time you open { -brand }.
quit-processing-title = Quit while a meeting is being processed?
quit-processing-body = { -brand } is transcribing or summarizing a meeting. It carries on the next time you open { -brand }.
quit-confirm = Quit
quit-cancel = Cancel
