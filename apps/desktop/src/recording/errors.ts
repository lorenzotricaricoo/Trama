/**
 * Message ids for the error codes of the recording (see `recording.rs` and the capture
 * codes in `trama_core::capture::error_code`). Codes without their own message get the
 * generic one, with the code shown for support.
 */
const START_ERRORS: Record<string, string> = {
  microphone_denied: 'record-error-microphone-denied',
  microphone_restricted: 'record-error-microphone-restricted',
  no_input_device: 'record-error-no-input-device',
  device_open_failed: 'record-error-device-open-failed',
  device_unavailable: 'record-error-device-open-failed',
  system_audio_denied: 'record-error-system-audio-denied',
  system_audio_unavailable: 'record-error-system-audio-unavailable',
  no_output_device: 'record-error-system-audio-unavailable',
  write_failed: 'record-error-write-failed',
  track_size_limit: 'record-error-track-size-limit',
  storage_failed: 'record-error-storage-failed',
  already_recording: 'record-error-already-recording',
};

/** The message id for an error of start or stop. */
export function recordingErrorMessage(code: string): string {
  return START_ERRORS[code] ?? 'record-error-generic';
}

/** Warnings shown in the panel while recording, by capture code. */
const NOTICES: Record<string, string> = {
  microphone_silent: 'record-notice-microphone-silent',
  system_audio_silent: 'record-notice-system-audio-silent',
  stream_failed: 'record-notice-interrupted',
  stream_stalled: 'record-notice-interrupted',
  device_unavailable: 'record-notice-interrupted',
  frames_dropped: 'record-notice-frames-dropped',
  write_failed: 'record-notice-write-failed',
  track_size_limit: 'record-notice-track-size-limit',
};

/** The message id for a warning during the recording; undefined for codes not shown. */
export function recordingNoticeMessage(code: string | null): string | undefined {
  return code === null ? undefined : NOTICES[code];
}

/** An error as a code: commands reject with a string, anything else is unexpected. */
export function errorCode(error: unknown): string {
  return typeof error === 'string' ? error : 'internal';
}
