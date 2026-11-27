//! System audio via a Core Audio process tap (macOS 14.2+).
//!
//! Steps: CATapDescription -> AudioHardwareCreateProcessTap -> private aggregate
//! device that contains the tap (+ the default output device as clock source)
//! -> IOProc on the aggregate device. The tap and the private aggregate device
//! are owned by this process: coreaudiod removes them if the process dies.

use crate::sink::RtSide;
use anyhow::{Result, bail};
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_audio::*;
use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSUUID};
use std::ffi::{CStr, c_void};
use std::mem::size_of;
use std::ptr::{NonNull, null};

fn check(status: i32, what: &str) -> Result<()> {
    if status != 0 {
        let b = status.to_be_bytes();
        let fourcc: String = b.iter().map(|&c| if c.is_ascii_graphic() { c as char } else { '.' }).collect();
        bail!("{what} failed: OSStatus {status} ('{fourcc}')");
    }
    Ok(())
}

fn addr(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

unsafe fn get_prop<T: Copy>(obj: AudioObjectID, selector: u32, qualifier: Option<(&[u8],)>) -> Result<T> {
    let mut out: T = unsafe { std::mem::zeroed() };
    let mut size = size_of::<T>() as u32;
    let a = addr(selector);
    let (qs, qp) = match qualifier {
        Some((q,)) => (q.len() as u32, q.as_ptr() as *const c_void),
        None => (0, null()),
    };
    let st = unsafe {
        AudioObjectGetPropertyData(
            obj,
            NonNull::from(&a),
            qs,
            qp,
            NonNull::from(&mut size),
            NonNull::from(&mut out).cast(),
        )
    };
    check(st, &format!("AudioObjectGetPropertyData({selector:#x})"))?;
    Ok(out)
}

unsafe fn get_string(obj: AudioObjectID, selector: u32) -> Result<Retained<NSString>> {
    let p: usize = unsafe { get_prop(obj, selector, None)? };
    if p == 0 {
        bail!("null string property {selector:#x}");
    }
    // CFStringRef is toll-free bridged to NSString; the getter returns +1.
    Ok(unsafe { Retained::from_raw(p as *mut NSString).unwrap() })
}

fn key(k: &CStr) -> Retained<NSString> {
    NSString::from_str(k.to_str().unwrap())
}

fn obj<T: objc2::Message>(r: &Retained<T>) -> &AnyObject {
    unsafe { &*(Retained::as_ptr(r) as *const AnyObject) }
}

pub fn pid_to_process_object(pid: i32) -> Result<AudioObjectID> {
    let q = pid.to_ne_bytes();
    let id: AudioObjectID = unsafe {
        get_prop(kAudioObjectSystemObject as u32, kAudioHardwarePropertyTranslatePIDToProcessObject, Some((&q,)))?
    };
    if id == 0 {
        bail!("pid {pid} has no Core Audio process object (is it playing audio?)");
    }
    Ok(id)
}

pub enum Target {
    /// Everything except this process (what a meeting recorder wants).
    AllButSelf,
    /// Only these PIDs (used by the automated test, together with `mute`).
    Pids(Vec<i32>),
}

pub struct SystemTap {
    tap_id: AudioObjectID,
    agg_id: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    ctx: *mut Ctx,
    pub output_uid: String,
    pub non_interleaved: bool,
}

struct Ctx {
    rt: Option<RtSide>,
    channels: usize,
    scratch: Vec<f32>,
}

/// Read the stream format of the tap before creating the writer.
pub struct Prepared {
    tap_id: AudioObjectID,
    agg_id: AudioObjectID,
    pub asbd: AudioStreamBasicDescription,
    pub output_uid: String,
}

pub fn prepare(target: Target, mute: bool, tap_only: bool) -> Result<Prepared> {
    unsafe {
        let mut objs = Vec::new();
        let desc = match &target {
            Target::AllButSelf => {
                if let Ok(me) = pid_to_process_object(std::process::id() as i32) {
                    objs.push(NSNumber::new_u32(me));
                }
                let arr = NSArray::from_retained_slice(&objs);
                CATapDescription::initStereoGlobalTapButExcludeProcesses(CATapDescription::alloc(), &arr)
            }
            Target::Pids(pids) => {
                for &p in pids {
                    objs.push(NSNumber::new_u32(pid_to_process_object(p)?));
                }
                let arr = NSArray::from_retained_slice(&objs);
                CATapDescription::initStereoMixdownOfProcesses(CATapDescription::alloc(), &arr)
            }
        };
        let tap_uuid = NSUUID::new();
        desc.setUUID(&tap_uuid);
        desc.setName(&NSString::from_str("trama-spike-tap"));
        desc.setPrivate(true);
        desc.setMuteBehavior(if mute { CATapMuteBehavior::Muted } else { CATapMuteBehavior::Unmuted });

        let mut tap_id: AudioObjectID = 0;
        check(AudioHardwareCreateProcessTap(Some(&desc), &mut tap_id), "AudioHardwareCreateProcessTap")?;

        let out_dev: AudioObjectID = get_prop(kAudioObjectSystemObject as u32, kAudioHardwarePropertyDefaultOutputDevice, None)?;
        let out_uid = get_string(out_dev, kAudioDevicePropertyDeviceUID)?;

        let sub_dev = NSDictionary::from_slices(&[&*key(kAudioSubDeviceUIDKey)], &[obj(&out_uid)]);
        let yes = NSNumber::new_bool(true);
        let no = NSNumber::new_bool(false);
        let tap_uid = tap_uuid.UUIDString();
        let sub_tap = NSDictionary::from_slices(
            &[&*key(kAudioSubTapUIDKey), &*key(kAudioSubTapDriftCompensationKey)],
            &[obj(&tap_uid), obj(&yes)],
        );
        let sub_devs = NSArray::from_retained_slice(&[sub_dev]);
        let sub_taps = NSArray::from_retained_slice(&[sub_tap]);
        let agg_uid = NSUUID::new().UUIDString();
        let name = NSString::from_str("Trama spike tap device");
        let k = [
            key(kAudioAggregateDeviceNameKey),
            key(kAudioAggregateDeviceUIDKey),
            key(kAudioAggregateDeviceMainSubDeviceKey),
            key(kAudioAggregateDeviceIsPrivateKey),
            key(kAudioAggregateDeviceIsStackedKey),
            key(kAudioAggregateDeviceTapAutoStartKey),
            key(kAudioAggregateDeviceSubDeviceListKey),
            key(kAudioAggregateDeviceTapListKey),
        ];
        let mut kr: Vec<&NSString> = k.iter().map(|x| &**x).collect();
        let mut v = vec![obj(&name), obj(&agg_uid), obj(&out_uid), obj(&yes), obj(&no), obj(&yes), obj(&sub_devs), obj(&sub_taps)];
        if tap_only {
            // Aggregate made of the tap alone: no dependency on the output device
            // (survives headphones being plugged/unplugged?), tap provides the clock.
            for i in [6, 2] {
                kr.remove(i);
                v.remove(i);
            }
        }
        let dict = NSDictionary::from_slices(&kr, &v);
        let cf: &CFDictionary = &*(Retained::as_ptr(&dict) as *const CFDictionary);
        let mut agg_id: AudioObjectID = 0;
        if let Err(e) = check(AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut agg_id)), "AudioHardwareCreateAggregateDevice") {
            AudioHardwareDestroyProcessTap(tap_id);
            return Err(e);
        }
        let asbd: AudioStreamBasicDescription = get_prop(tap_id, kAudioTapPropertyFormat, None)?;
        Ok(Prepared { tap_id, agg_id, asbd, output_uid: out_uid.to_string() })
    }
}

impl Prepared {
    pub fn start(self, rt: RtSide) -> Result<SystemTap> {
        let channels = self.asbd.mChannelsPerFrame as usize;
        let non_interleaved = self.asbd.mFormatFlags & (1 << 5) != 0; // kAudioFormatFlagIsNonInterleaved
        if self.asbd.mFormatFlags & 1 == 0 /* kAudioFormatFlagIsFloat */  || self.asbd.mBitsPerChannel != 32 {
            bail!("unexpected tap format (flags {:#x}, {} bits)", self.asbd.mFormatFlags, self.asbd.mBitsPerChannel);
        }
        let ctx = Box::into_raw(Box::new(Ctx { rt: Some(rt), channels, scratch: vec![0.0; 16384 * channels] }));
        let mut proc_id: AudioDeviceIOProcID = None;
        unsafe {
            check(
                AudioDeviceCreateIOProcID(self.agg_id, Some(io_proc), ctx as *mut c_void, NonNull::from(&mut proc_id)),
                "AudioDeviceCreateIOProcID",
            )?;
            check(AudioDeviceStart(self.agg_id, proc_id), "AudioDeviceStart")?;
        }
        Ok(SystemTap {
            tap_id: self.tap_id,
            agg_id: self.agg_id,
            proc_id,
            ctx,
            output_uid: self.output_uid,
            non_interleaved,
        })
    }
}

impl SystemTap {
    pub fn stop(self) {
        unsafe {
            AudioDeviceStop(self.agg_id, self.proc_id);
            AudioDeviceDestroyIOProcID(self.agg_id, self.proc_id);
            AudioHardwareDestroyAggregateDevice(self.agg_id);
            AudioHardwareDestroyProcessTap(self.tap_id);
            drop(Box::from_raw(self.ctx));
        }
    }
}

const KAUDIO_TIMESTAMP_SAMPLE_VALID: u32 = 1;

unsafe extern "C-unwind" fn io_proc(
    _dev: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    input: NonNull<AudioBufferList>,
    in_time: NonNull<AudioTimeStamp>,
    _out: NonNull<AudioBufferList>,
    _out_time: NonNull<AudioTimeStamp>,
    client: *mut c_void,
) -> i32 {
    unsafe {
        let ctx = &mut *(client as *mut Ctx);
        let Some(rt) = ctx.rt.as_mut() else { return 0 };
        let abl = input.as_ptr();
        let n = (*abl).mNumberBuffers as usize;
        if n == 0 {
            return 0;
        }
        let bufs = std::slice::from_raw_parts(std::ptr::addr_of!((*abl).mBuffers) as *const AudioBuffer, n);
        let t = in_time.as_ref();
        let host_ns = AudioConvertHostTimeToNanos(t.mHostTime);
        let st = if t.mFlags.0 & KAUDIO_TIMESTAMP_SAMPLE_VALID != 0 { t.mSampleTime } else { f64::NAN };
        if n == 1 {
            let b = &bufs[0];
            if b.mData.is_null() {
                return 0;
            }
            let s = std::slice::from_raw_parts(b.mData as *const f32, b.mDataByteSize as usize / 4);
            rt.push(s, b.mNumberChannels.max(1) as usize, host_ns, st);
        } else {
            // Non-interleaved: one buffer per channel -> interleave into scratch.
            let ch = n.min(ctx.channels.max(1));
            let frames = (bufs[0].mDataByteSize as usize / 4).min(ctx.scratch.len() / ch);
            for (c, b) in bufs.iter().take(ch).enumerate() {
                if b.mData.is_null() {
                    continue;
                }
                let s = std::slice::from_raw_parts(b.mData as *const f32, frames);
                for (i, v) in s.iter().enumerate() {
                    ctx.scratch[i * ch + c] = *v;
                }
            }
            rt.push(&ctx.scratch[..frames * ch], ch, host_ns, st);
        }
    }
    0
}
