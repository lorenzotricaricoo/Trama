//! System audio through a Core Audio process tap (macOS 14.2+), as validated by spike #11.
//!
//! `CATapDescription` (a stereo global tap that excludes this process) ->
//! `AudioHardwareCreateProcessTap` -> a private aggregate device that holds the tap,
//! clocked by the default output device -> an IOProc on the aggregate device. The tap and
//! the aggregate device belong to this process: Core Audio removes them if it dies.
//!
//! Without the "System Audio Recording" permission the tap delivers silence and reports
//! no error; [`probe`] checks it by tapping an inaudible tone of our own.

use std::ffi::{CStr, c_void};
use std::mem::size_of;
use std::ptr::{NonNull, null};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_audio::*;
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp,
};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSUUID};

use super::super::backend::frame_channel;
use super::super::{
    ActiveStream, CaptureError, DeviceInfo, FrameSink, PermissionState, Result, StreamFormat,
    error_code,
};
use super::os_error;

const UNAVAILABLE: &str = error_code::SYSTEM_AUDIO_UNAVAILABLE;
/// `kAudioFormatFlagIsFloat`.
const FLAG_FLOAT: u32 = 1;
/// Frames the IOProc can interleave per callback; Core Audio buffers are far smaller.
const SCRATCH_FRAMES: usize = 16_384;

fn check(status: i32, what: &str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(os_error(UNAVAILABLE, what, status))
    }
}

fn global(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

/// Reads a fixed-size property.
///
/// # Safety
/// `T` must be the type Core Audio stores for `selector` on `object`.
unsafe fn property<T: Copy>(object: AudioObjectID, selector: u32, qualifier: &[u8]) -> Result<T> {
    // SAFETY: `T: Copy` and plain data; Core Audio writes at most `size` bytes.
    let mut out: T = unsafe { std::mem::zeroed() };
    let mut size = size_of::<T>() as u32;
    let address = global(selector);
    let (qualifier_size, qualifier_ptr) = if qualifier.is_empty() {
        (0, null())
    } else {
        (qualifier.len() as u32, qualifier.as_ptr().cast::<c_void>())
    };
    // SAFETY: every pointer is valid for the duration of the call.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&address),
            qualifier_size,
            qualifier_ptr,
            NonNull::from(&mut size),
            NonNull::from(&mut out).cast(),
        )
    };
    check(
        status,
        &format!("AudioObjectGetPropertyData({selector:#x})"),
    )?;
    Ok(out)
}

/// Reads a `CFString` property.
fn string_property(object: AudioObjectID, selector: u32) -> Result<Retained<NSString>> {
    // SAFETY: the property is a `CFStringRef`, returned with +1 retain count, and
    // toll-free bridged to `NSString`.
    unsafe {
        let ptr: usize = property(object, selector, &[])?;
        Retained::from_raw(ptr as *mut NSString)
            .ok_or_else(|| CaptureError::new(UNAVAILABLE, format!("null property {selector:#x}")))
    }
}

fn key(k: &CStr) -> Retained<NSString> {
    NSString::from_str(k.to_str().unwrap_or_default())
}

fn any<T: objc2::Message>(object: &Retained<T>) -> &AnyObject {
    // SAFETY: every Objective-C object is an `AnyObject`.
    unsafe { &*(Retained::as_ptr(object).cast::<AnyObject>()) }
}

fn output_device() -> Result<Option<(AudioObjectID, DeviceInfo)>> {
    // SAFETY: the default output device is an `AudioObjectID`.
    let id: AudioObjectID = unsafe {
        property(
            kAudioObjectSystemObject as AudioObjectID,
            kAudioHardwarePropertyDefaultOutputDevice,
            &[],
        )?
    };
    if id == kAudioObjectUnknown as AudioObjectID {
        return Ok(None);
    }
    let uid = string_property(id, kAudioDevicePropertyDeviceUID)?.to_string();
    let name = string_property(id, kAudioObjectPropertyName)
        .map(|n| n.to_string())
        .unwrap_or_else(|_| uid.clone());
    Ok(Some((id, DeviceInfo { id: uid, name })))
}

pub(super) fn default_output_device() -> Result<Option<DeviceInfo>> {
    Ok(output_device()?.map(|(_, info)| info))
}

fn process_object(pid: i32) -> Result<AudioObjectID> {
    // SAFETY: the property maps a pid (qualifier) to an `AudioObjectID`.
    let id: AudioObjectID = unsafe {
        property(
            kAudioObjectSystemObject as AudioObjectID,
            kAudioHardwarePropertyTranslatePIDToProcessObject,
            &pid.to_ne_bytes(),
        )?
    };
    if id == kAudioObjectUnknown as AudioObjectID {
        return Err(CaptureError::new(
            UNAVAILABLE,
            format!("pid {pid} has no audio object"),
        ));
    }
    Ok(id)
}

#[derive(Clone, Copy)]
enum Target {
    /// Everything but this process: what a meeting recorder wants.
    OthersMixed,
    /// Only this process, muted: the permission probe.
    OwnProcessMuted,
}

/// A tap and its aggregate device, not started yet.
struct Prepared {
    tap: AudioObjectID,
    aggregate: AudioObjectID,
    asbd: AudioStreamBasicDescription,
    output: DeviceInfo,
}

impl Prepared {
    fn new(target: Target) -> Result<Self> {
        let (_, output) =
            output_device()?.ok_or_else(|| CaptureError::new(error_code::NO_OUTPUT_DEVICE, ""))?;
        let own = process_object(std::process::id() as i32);
        // SAFETY: Objective-C calls on objects created here; the tap is destroyed if the
        // aggregate device cannot be created.
        unsafe {
            let description = match target {
                Target::OthersMixed => {
                    // If we never played audio we have no process object: nothing to exclude.
                    let exclude: Vec<_> = own.into_iter().map(NSNumber::new_u32).collect();
                    let exclude = NSArray::from_retained_slice(&exclude);
                    CATapDescription::initStereoGlobalTapButExcludeProcesses(
                        CATapDescription::alloc(),
                        &exclude,
                    )
                }
                Target::OwnProcessMuted => {
                    let only = NSArray::from_retained_slice(&[NSNumber::new_u32(own?)]);
                    CATapDescription::initStereoMixdownOfProcesses(CATapDescription::alloc(), &only)
                }
            };
            let tap_uuid = NSUUID::new();
            description.setUUID(&tap_uuid);
            description.setName(&NSString::from_str("Trama"));
            description.setPrivate(true);
            description.setMuteBehavior(match target {
                Target::OthersMixed => CATapMuteBehavior::Unmuted,
                Target::OwnProcessMuted => CATapMuteBehavior::Muted,
            });
            let mut tap: AudioObjectID = 0;
            check(
                AudioHardwareCreateProcessTap(Some(&description), &mut tap),
                "AudioHardwareCreateProcessTap",
            )?;

            let output_uid = NSString::from_str(&output.id);
            let yes = NSNumber::new_bool(true);
            let no = NSNumber::new_bool(false);
            let tap_uid = tap_uuid.UUIDString();
            let sub_device =
                NSDictionary::from_slices(&[&*key(kAudioSubDeviceUIDKey)], &[any(&output_uid)]);
            let sub_tap = NSDictionary::from_slices(
                &[
                    &*key(kAudioSubTapUIDKey),
                    &*key(kAudioSubTapDriftCompensationKey),
                ],
                &[any(&tap_uid), any(&yes)],
            );
            let sub_devices = NSArray::from_retained_slice(&[sub_device]);
            let sub_taps = NSArray::from_retained_slice(&[sub_tap]);
            let aggregate_uid = NSUUID::new().UUIDString();
            let name = NSString::from_str("Trama system audio");
            let keys = [
                key(kAudioAggregateDeviceNameKey),
                key(kAudioAggregateDeviceUIDKey),
                key(kAudioAggregateDeviceMainSubDeviceKey),
                key(kAudioAggregateDeviceIsPrivateKey),
                key(kAudioAggregateDeviceIsStackedKey),
                key(kAudioAggregateDeviceTapAutoStartKey),
                key(kAudioAggregateDeviceSubDeviceListKey),
                key(kAudioAggregateDeviceTapListKey),
            ];
            let keys: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
            let values = [
                any(&name),
                any(&aggregate_uid),
                any(&output_uid),
                any(&yes),
                any(&no),
                any(&yes),
                any(&sub_devices),
                any(&sub_taps),
            ];
            let dict = NSDictionary::from_slices(&keys, &values);
            let cf: &CFDictionary = &*(Retained::as_ptr(&dict).cast::<CFDictionary>());
            let mut aggregate: AudioObjectID = 0;
            if let Err(err) = check(
                AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut aggregate)),
                "AudioHardwareCreateAggregateDevice",
            ) {
                AudioHardwareDestroyProcessTap(tap);
                return Err(err);
            }
            let mut prepared = Prepared {
                tap,
                aggregate,
                asbd: std::mem::zeroed(),
                output,
            };
            let asbd: AudioStreamBasicDescription = property(tap, kAudioTapPropertyFormat, &[])?;
            prepared.asbd = asbd;
            if asbd.mFormatFlags & FLAG_FLOAT == 0
                || asbd.mBitsPerChannel != 32
                || asbd.mChannelsPerFrame == 0
            {
                return Err(CaptureError::new(
                    UNAVAILABLE,
                    format!(
                        "unexpected tap format: flags {:#x}, {} bits, {} channels",
                        asbd.mFormatFlags, asbd.mBitsPerChannel, asbd.mChannelsPerFrame
                    ),
                ));
            }
            Ok(prepared)
        }
    }

    fn format(&self) -> StreamFormat {
        StreamFormat {
            sample_rate: self.asbd.mSampleRate.round() as u32,
            channels: self.asbd.mChannelsPerFrame as u16,
        }
    }

    fn start(self, sink: FrameSink) -> Result<SystemTap> {
        let format = self.format();
        let ctx = Box::into_raw(Box::new(IoContext {
            sink,
            channels: usize::from(format.channels),
            scratch: vec![0.0; SCRATCH_FRAMES * usize::from(format.channels)],
        }));
        let mut tap = SystemTap {
            prepared: self,
            proc_id: None,
            ctx,
            format,
        };
        // SAFETY: `ctx` stays valid until `SystemTap::drop` has stopped and destroyed the
        // IOProc; on failure the drop cleans up what was created.
        unsafe {
            check(
                AudioDeviceCreateIOProcID(
                    tap.prepared.aggregate,
                    Some(io_proc),
                    ctx.cast(),
                    NonNull::from(&mut tap.proc_id),
                ),
                "AudioDeviceCreateIOProcID",
            )?;
            check(
                AudioDeviceStart(tap.prepared.aggregate, tap.proc_id),
                "AudioDeviceStart",
            )?;
        }
        Ok(tap)
    }
}

impl Drop for Prepared {
    fn drop(&mut self) {
        // SAFETY: both objects were created by `Prepared::new` and are destroyed once.
        unsafe {
            AudioHardwareDestroyAggregateDevice(self.aggregate);
            AudioHardwareDestroyProcessTap(self.tap);
        }
    }
}

struct IoContext {
    sink: FrameSink,
    channels: usize,
    scratch: Vec<f32>,
}

/// A running tap. Dropping it stops the IOProc and removes the tap.
struct SystemTap {
    prepared: Prepared,
    proc_id: AudioDeviceIOProcID,
    ctx: *mut IoContext,
    format: StreamFormat,
}

impl ActiveStream for SystemTap {
    fn format(&self) -> StreamFormat {
        self.format
    }
    fn device(&self) -> DeviceInfo {
        self.prepared.output.clone()
    }
    fn backend(&self) -> &'static str {
        "coreaudio_process_tap"
    }
}

impl Drop for SystemTap {
    fn drop(&mut self) {
        // SAFETY: stopping from outside the IOProc is synchronous, so the IOProc no longer
        // runs when the context is freed. `Prepared::drop` then removes the devices.
        unsafe {
            if self.proc_id.is_some() {
                AudioDeviceStop(self.prepared.aggregate, self.proc_id);
                AudioDeviceDestroyIOProcID(self.prepared.aggregate, self.proc_id);
            }
            drop(Box::from_raw(self.ctx));
        }
    }
}

/// `kAudioTimeStampHostTimeValid`.
const HOST_TIME_VALID: u32 = 1 << 1;

/// The IOProc: copies the tap's input into the sink. Real-time: no locks, no allocation.
unsafe extern "C-unwind" fn io_proc(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    input: NonNull<AudioBufferList>,
    input_time: NonNull<AudioTimeStamp>,
    _output: NonNull<AudioBufferList>,
    _output_time: NonNull<AudioTimeStamp>,
    client: *mut c_void,
) -> i32 {
    // SAFETY: `client` is the `IoContext` registered with this IOProc, used only here
    // while the IOProc runs; the buffer list and timestamps are valid during the call.
    unsafe {
        let ctx = &mut *client.cast::<IoContext>();
        let list = input.as_ptr();
        let count = (*list).mNumberBuffers as usize;
        if count == 0 {
            return 0;
        }
        let buffers = std::slice::from_raw_parts(
            std::ptr::addr_of!((*list).mBuffers).cast::<AudioBuffer>(),
            count,
        );
        let time = input_time.as_ref();
        let host_ns = if time.mFlags.0 & HOST_TIME_VALID != 0 {
            AudioConvertHostTimeToNanos(time.mHostTime)
        } else {
            AudioConvertHostTimeToNanos(AudioGetCurrentHostTime())
        };
        if count == 1 {
            let buffer = &buffers[0];
            if buffer.mData.is_null() {
                return 0;
            }
            let samples = std::slice::from_raw_parts(
                buffer.mData.cast::<f32>(),
                buffer.mDataByteSize as usize / 4,
            );
            ctx.sink
                .push(samples, buffer.mNumberChannels.max(1) as usize, host_ns);
        } else {
            // Non-interleaved: one buffer per channel.
            let channels = count.min(ctx.channels.max(1));
            let frames = (buffers[0].mDataByteSize as usize / 4).min(SCRATCH_FRAMES);
            for (c, buffer) in buffers.iter().take(channels).enumerate() {
                if buffer.mData.is_null() {
                    continue;
                }
                let samples = std::slice::from_raw_parts(buffer.mData.cast::<f32>(), frames);
                for (i, sample) in samples.iter().enumerate() {
                    ctx.scratch[i * channels + c] = *sample;
                }
            }
            ctx.sink
                .push(&ctx.scratch[..frames * channels], channels, host_ns);
        }
    }
    0
}

/// Opens the tap on everything but this process. On failure, gives the sink back if
/// the tap could not even be created, so a fallback can use it.
pub(super) fn try_open(
    sink: FrameSink,
) -> std::result::Result<Box<dyn ActiveStream>, (CaptureError, Option<FrameSink>)> {
    let prepared = match Prepared::new(Target::OthersMixed) {
        Ok(prepared) => prepared,
        Err(err) => return Err((err, Some(sink))),
    };
    match prepared.start(sink) {
        Ok(tap) => Ok(Box::new(tap)),
        Err(err) => Err((err, None)),
    }
}

#[cfg_attr(feature = "screencapturekit", allow(dead_code))]
pub(super) fn open(sink: FrameSink) -> Result<Box<dyn ActiveStream>> {
    try_open(sink).map_err(|(err, _)| err)
}

/// Checks the "System Audio Recording" permission: plays a tone at -80 dBFS (inaudible,
/// and muted by the tap) and checks whether the tap hears it.
pub(super) fn probe() -> PermissionState {
    probe_inner().unwrap_or(PermissionState::Unknown)
}

fn probe_inner() -> Option<PermissionState> {
    let device = cpal::default_host().default_output_device()?;
    let config = device.default_output_config().ok()?;
    let rate = config.sample_rate() as f32;
    let channels = usize::from(config.channels());
    let mut phase = 0.0f32;
    let tone = device
        .build_output_stream::<f32, _, _>(
            config.config(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(channels) {
                    phase = (phase + 1_000.0 / rate).fract();
                    frame.fill(1e-4 * (phase * std::f32::consts::TAU).sin());
                }
            },
            |_| {},
            None,
        )
        .ok()?;
    tone.play().ok()?;
    // Our process gets an audio object once it plays.
    std::thread::sleep(Duration::from_millis(200));
    let prepared = Prepared::new(Target::OwnProcessMuted).ok()?;
    let rate = prepared.format().sample_rate;
    let (sink, mut source, _) = frame_channel();
    let tap = prepared.start(sink).ok()?;
    let deadline = Instant::now() + Duration::from_millis(1_500);
    let mut received = 0usize;
    let mut heard = false;
    while Instant::now() < deadline && !heard {
        std::thread::sleep(Duration::from_millis(50));
        while source.metas.pop().is_ok() {}
        let n = source.samples.slots();
        if let Ok(chunk) = source.samples.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            heard = a.iter().chain(b).any(|&s| s != 0.0);
            received += n;
            chunk.commit_all();
        }
    }
    drop(tap);
    drop(tone);
    Some(if heard {
        PermissionState::Granted
    } else if received > rate as usize / 2 {
        // A third of a second of exact zeros while we were playing: denied.
        PermissionState::Denied
    } else {
        PermissionState::Unknown
    })
}
