/** Codes stored on a failed meeting that have their own message. */
const MEETING_ERRORS: Record<string, string> = {
  model_missing: 'meeting-error-model-missing',
  model_corrupt: 'meeting-error-model-missing',
  no_audio: 'meeting-error-no-audio',
  audio_missing: 'meeting-error-audio-missing',
  audio_invalid: 'meeting-error-audio-invalid',
  audio_read_failed: 'meeting-error-audio-invalid',
  language_unsupported: 'meeting-error-language-unsupported',
  transcription_cancelled: 'meeting-error-cancelled',
  recording_interrupted: 'meeting-error-recording-interrupted',
  write_failed: 'meeting-error-write-failed',
};

/** The message id for why a meeting failed. */
export function meetingErrorMessage(code: string | null): string {
  return (code && MEETING_ERRORS[code]) ?? 'meeting-error-generic';
}
