//! The microphone through `cpal` (Core Audio AUHAL underneath).

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{DeviceId, ErrorKind};

use super::super::{
    ActiveStream, CaptureError, DeviceInfo, FaultReporter, FrameSink, Result, StreamFormat,
    error_code,
};

fn info(device: &cpal::Device) -> DeviceInfo {
    DeviceInfo {
        id: device.id().map(|id| id.to_string()).unwrap_or_default(),
        name: device
            .description()
            .map(|d| d.name().to_owned())
            .unwrap_or_default(),
    }
}

pub(super) fn default_device() -> Option<DeviceInfo> {
    cpal::default_host()
        .default_input_device()
        .map(|d| info(&d))
}

pub(super) fn input_devices() -> Result<Vec<DeviceInfo>> {
    let devices = cpal::default_host()
        .input_devices()
        .map_err(|e| CaptureError::new(error_code::DEVICE_OPEN_FAILED, e))?;
    Ok(devices.map(|d| info(&d)).collect())
}

fn error_code_of(err: &cpal::Error) -> &'static str {
    match err.kind() {
        ErrorKind::PermissionDenied => error_code::MICROPHONE_DENIED,
        ErrorKind::DeviceNotAvailable => error_code::DEVICE_UNAVAILABLE,
        _ => error_code::DEVICE_OPEN_FAILED,
    }
}

fn open_error(err: cpal::Error) -> CaptureError {
    CaptureError::new(error_code_of(&err), err)
}

struct MicStream {
    _stream: cpal::Stream,
    device: DeviceInfo,
    format: StreamFormat,
}

impl ActiveStream for MicStream {
    fn format(&self) -> StreamFormat {
        self.format
    }
    fn device(&self) -> DeviceInfo {
        self.device.clone()
    }
    fn backend(&self) -> &'static str {
        "cpal"
    }
}

pub(super) fn open(
    pinned: Option<&str>,
    mut sink: FrameSink,
    faults: FaultReporter,
) -> Result<Box<dyn ActiveStream>> {
    let host = cpal::default_host();
    let device = match pinned {
        Some(id) => {
            let unavailable = || CaptureError::new(error_code::DEVICE_UNAVAILABLE, id);
            let id: DeviceId = id.parse().map_err(|_| unavailable())?;
            host.device_by_id(&id).ok_or_else(unavailable)?
        }
        None => host
            .default_input_device()
            .ok_or_else(|| CaptureError::new(error_code::NO_INPUT_DEVICE, ""))?,
    };
    let config = device.default_input_config().map_err(open_error)?;
    let format = StreamFormat {
        sample_rate: config.sample_rate(),
        channels: config.channels(),
    };
    let channels = usize::from(format.channels);
    let stream = device
        .build_input_stream::<f32, _, _>(
            config.config(),
            move |data: &[f32], info: &cpal::InputCallbackInfo| {
                // Capture time, on the host clock like the process tap's input time.
                let host_ns = info.timestamp().capture.as_nanos() as u64;
                sink.push(data, channels, host_ns);
            },
            move |err: cpal::Error| match err.kind() {
                // Informational: the stream keeps running.
                ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied => {}
                ErrorKind::DeviceNotAvailable => {
                    faults.report(error_code::DEVICE_UNAVAILABLE, err);
                }
                _ => faults.report(error_code::STREAM_FAILED, err),
            },
            None,
        )
        .map_err(open_error)?;
    stream.play().map_err(open_error)?;
    Ok(Box::new(MicStream {
        _stream: stream,
        device: info(&device),
        format,
    }))
}
