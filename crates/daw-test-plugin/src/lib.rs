//! A tiny VST3 plugin for daw-plugins' tests: "NPT Test Synth" (a sine
//! instrument) and "NPT Test Gain" (a volume effect), each with a separate
//! edit controller like most real plugins. Not shipped with the app.
//!
//! Adapted from the vst3-rs `gain` example
//! (https://github.com/coupler-rs/vst3-rs, examples/gain.rs), MIT OR
//! Apache-2.0.

#![allow(unsafe_code)]
#![allow(non_upper_case_globals, non_snake_case, clippy::missing_safety_doc)]
// The plugin hands out mutable views of host buffers it receives by
// pointer, and VST3 enum types differ in signedness between platforms.
#![allow(clippy::mut_from_ref, clippy::unnecessary_cast)]

use std::ffi::{CString, c_char, c_void};
use std::ptr;
use std::slice;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComRef, ComWrapper, uid};

fn copy_cstring(src: &str, dst: &mut [c_char]) {
    let c_string = CString::new(src).unwrap_or_default();
    for (s, d) in c_string.as_bytes_with_nul().iter().zip(dst.iter_mut()) {
        *d = *s as c_char;
    }
    if c_string.as_bytes_with_nul().len() > dst.len()
        && let Some(last) = dst.last_mut()
    {
        *last = 0;
    }
}

fn copy_wstring(src: &str, dst: &mut [TChar]) {
    let mut len = 0;
    for (s, d) in src.encode_utf16().zip(dst.iter_mut()) {
        *d = s as TChar;
        len += 1;
    }
    if len < dst.len() {
        dst[len] = 0;
    } else if let Some(last) = dst.last_mut() {
        *last = 0;
    }
}

/// Reads the 8-byte parameter value the processors save as their state.
unsafe fn read_value(stream: *mut IBStream) -> Option<f64> {
    let stream = unsafe { ComRef::from_raw(stream)? };
    let mut bytes = [0u8; 8];
    let mut read = 0;
    let r = unsafe { stream.read(bytes.as_mut_ptr() as *mut c_void, 8, &mut read) };
    (r == kResultOk && read == 8).then(|| f64::from_le_bytes(bytes))
}

unsafe fn write_value(stream: *mut IBStream, value: f64) -> tresult {
    let Some(stream) = (unsafe { ComRef::from_raw(stream) }) else {
        return kInvalidArgument;
    };
    let mut bytes = value.to_le_bytes();
    let mut written = 0;
    unsafe { stream.write(bytes.as_mut_ptr() as *mut c_void, 8, &mut written) }
}

/// The last value of parameter 0 in this block's changes, if any.
unsafe fn param0_change(data: &ProcessData) -> Option<f64> {
    let changes = unsafe { ComRef::from_raw(data.inputParameterChanges)? };
    let mut found = None;
    for i in 0..unsafe { changes.getParameterCount() } {
        let Some(queue) = (unsafe { ComRef::from_raw(changes.getParameterData(i)) }) else {
            continue;
        };
        if unsafe { queue.getParameterId() } != 0 {
            continue;
        }
        let points = unsafe { queue.getPointCount() };
        let (mut offset, mut value) = (0, 0.0);
        if points > 0 && unsafe { queue.getPoint(points - 1, &mut offset, &mut value) } == kResultOk
        {
            found = Some(value);
        }
    }
    found
}

/// Output channel buffers of bus 0 (stereo), if the host gave them.
unsafe fn stereo_out(data: &ProcessData) -> Option<(&mut [f32], &mut [f32])> {
    if data.numOutputs < 1 || data.outputs.is_null() {
        return None;
    }
    let bus = unsafe { &*data.outputs };
    if bus.numChannels != 2 {
        return None;
    }
    let n = data.numSamples as usize;
    let chans = unsafe { slice::from_raw_parts(bus.__field0.channelBuffers32, 2) };
    unsafe {
        Some((
            slice::from_raw_parts_mut(chans[0], n),
            slice::from_raw_parts_mut(chans[1], n),
        ))
    }
}

unsafe fn stereo_in(data: &ProcessData) -> Option<(&[f32], &[f32])> {
    if data.numInputs < 1 || data.inputs.is_null() {
        return None;
    }
    let bus = unsafe { &*data.inputs };
    if bus.numChannels != 2 {
        return None;
    }
    let n = data.numSamples as usize;
    let chans = unsafe { slice::from_raw_parts(bus.__field0.channelBuffers32, 2) };
    unsafe {
        Some((
            slice::from_raw_parts(chans[0], n),
            slice::from_raw_parts(chans[1], n),
        ))
    }
}

fn bus_info(bus: *mut BusInfo, media: MediaTypes, dir: BusDirections, channels: i32, name: &str) {
    let bus = unsafe { &mut *bus };
    bus.mediaType = media as MediaType;
    bus.direction = dir as BusDirection;
    bus.channelCount = channels;
    copy_wstring(name, &mut bus.name);
    bus.busType = BusTypes_::kMain as BusType;
    bus.flags = BusInfo_::BusFlags_::kDefaultActive as u32;
}

/// What differs between the two plugins.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Synth,
    Gain,
}

impl Kind {
    fn controller_cid(self) -> TUID {
        match self {
            Kind::Synth => SYNTH_CONTROLLER,
            Kind::Gain => GAIN_CONTROLLER,
        }
    }
    fn param_name(self) -> &'static str {
        match self {
            Kind::Synth => "Level",
            Kind::Gain => "Gain",
        }
    }
}

const SYNTH_PROCESSOR: TUID = uid(0x4E505431, 0x53594E54, 0x48000000, 0x00000001);
const SYNTH_CONTROLLER: TUID = uid(0x4E505431, 0x53594E54, 0x48000000, 0x00000002);
const GAIN_PROCESSOR: TUID = uid(0x4E505431, 0x4741494E, 0x00000000, 0x00000001);
const GAIN_CONTROLLER: TUID = uid(0x4E505431, 0x4741494E, 0x00000000, 0x00000002);

/// Default for parameter 0 (synth level, or gain where 0.5 = unchanged).
const DEFAULT_VALUE: f64 = 0.5;

const VOICES: usize = 8;

#[derive(Clone, Copy, Default)]
struct Voice {
    note: i16,
    on: bool,
    phase: f64,
    step: f64,
    velocity: f32,
}

struct Processor {
    kind: Kind,
    value: AtomicU64,
    sample_rate: AtomicU64,
    voices: Mutex<[Voice; VOICES]>,
}

impl Class for Processor {
    type Interfaces = (IComponent, IAudioProcessor, IProcessContextRequirements);
}

impl Processor {
    fn new(kind: Kind) -> Self {
        Processor {
            kind,
            value: AtomicU64::new(DEFAULT_VALUE.to_bits()),
            sample_rate: AtomicU64::new(48_000f64.to_bits()),
            voices: Mutex::new([Voice::default(); VOICES]),
        }
    }
    fn value(&self) -> f64 {
        f64::from_bits(self.value.load(Ordering::Relaxed))
    }

    unsafe fn handle_events(&self, data: &ProcessData, voices: &mut [Voice; VOICES]) {
        let Some(events) = (unsafe { ComRef::from_raw(data.inputEvents) }) else {
            return;
        };
        let sr = f64::from_bits(self.sample_rate.load(Ordering::Relaxed));
        for i in 0..unsafe { events.getEventCount() } {
            let mut e: Event = unsafe { std::mem::zeroed() };
            if unsafe { events.getEvent(i, &mut e) } != kResultOk {
                continue;
            }
            match e.r#type as Event_::EventTypes {
                Event_::EventTypes_::kNoteOnEvent => {
                    let on = unsafe { e.__field0.noteOn };
                    if let Some(v) = voices.iter_mut().find(|v| !v.on) {
                        let hz = 440.0 * 2f64.powf((on.pitch as f64 - 69.0) / 12.0);
                        *v = Voice {
                            note: on.pitch,
                            on: true,
                            phase: 0.0,
                            step: hz / sr,
                            velocity: on.velocity,
                        };
                    }
                }
                Event_::EventTypes_::kNoteOffEvent => {
                    let off = unsafe { e.__field0.noteOff };
                    for v in voices.iter_mut().filter(|v| v.on && v.note == off.pitch) {
                        v.on = false;
                    }
                }
                _ => {}
            }
        }
    }
}

impl IPluginBaseTrait for Processor {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for Processor {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        unsafe { *class_id = self.kind.controller_cid() };
        kResultOk
    }
    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }
    unsafe fn getBusCount(&self, media: MediaType, dir: BusDirection) -> i32 {
        match (media as MediaTypes, dir as BusDirections, self.kind) {
            (MediaTypes_::kAudio, BusDirections_::kOutput, _) => 1,
            (MediaTypes_::kAudio, BusDirections_::kInput, Kind::Gain) => 1,
            (MediaTypes_::kEvent, BusDirections_::kInput, Kind::Synth) => 1,
            _ => 0,
        }
    }
    unsafe fn getBusInfo(
        &self,
        media: MediaType,
        dir: BusDirection,
        index: i32,
        bus: *mut BusInfo,
    ) -> tresult {
        if index != 0 || unsafe { self.getBusCount(media, dir) } == 0 {
            return kInvalidArgument;
        }
        let media = media as MediaTypes;
        let channels = if media == MediaTypes_::kEvent { 16 } else { 2 };
        bus_info(bus, media, dir as BusDirections, channels, "Main");
        kResultOk
    }
    unsafe fn getRoutingInfo(&self, _i: *mut RoutingInfo, _o: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }
    unsafe fn activateBus(&self, _m: MediaType, _d: BusDirection, _i: i32, _s: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setActive(&self, _state: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        match unsafe { read_value(state) } {
            Some(v) => {
                self.value.store(v.to_bits(), Ordering::Relaxed);
                kResultOk
            }
            None => kResultFalse,
        }
    }
    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        unsafe { write_value(state, self.value()) }
    }
}

impl IAudioProcessorTrait for Processor {
    unsafe fn setBusArrangements(
        &self,
        inputs: *mut SpeakerArrangement,
        num_ins: i32,
        outputs: *mut SpeakerArrangement,
        num_outs: i32,
    ) -> tresult {
        let ins_ok = match self.kind {
            Kind::Gain => num_ins == 1 && unsafe { *inputs } == SpeakerArr::kStereo,
            Kind::Synth => num_ins == 0,
        };
        if ins_ok && num_outs == 1 && unsafe { *outputs } == SpeakerArr::kStereo {
            kResultTrue
        } else {
            kResultFalse
        }
    }
    unsafe fn getBusArrangement(
        &self,
        dir: BusDirection,
        index: i32,
        arr: *mut SpeakerArrangement,
    ) -> tresult {
        if index != 0 || unsafe { self.getBusCount(MediaTypes_::kAudio as MediaType, dir) } == 0 {
            return kInvalidArgument;
        }
        unsafe { *arr = SpeakerArr::kStereo };
        kResultOk
    }
    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        if size as SymbolicSampleSizes == SymbolicSampleSizes_::kSample32 {
            kResultOk
        } else {
            kNotImplemented
        }
    }
    unsafe fn getLatencySamples(&self) -> u32 {
        0
    }
    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        let sr = unsafe { (*setup).sampleRate };
        self.sample_rate.store(sr.to_bits(), Ordering::Relaxed);
        kResultOk
    }
    unsafe fn setProcessing(&self, _state: TBool) -> tresult {
        kResultOk
    }
    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let data = unsafe { &*data };
        if let Some(v) = unsafe { param0_change(data) } {
            self.value.store(v.to_bits(), Ordering::Relaxed);
        }
        let value = self.value() as f32;
        let Some((out_l, out_r)) = (unsafe { stereo_out(data) }) else {
            return kResultOk;
        };
        match self.kind {
            Kind::Gain => {
                let gain = value * 2.0;
                if let Some((in_l, in_r)) = unsafe { stereo_in(data) } {
                    for i in 0..out_l.len() {
                        out_l[i] = in_l[i] * gain;
                        out_r[i] = in_r[i] * gain;
                    }
                }
            }
            Kind::Synth => {
                let Ok(mut voices) = self.voices.lock() else {
                    return kResultOk;
                };
                unsafe { self.handle_events(data, &mut voices) };
                for i in 0..out_l.len() {
                    let mut s = 0.0f32;
                    for v in voices.iter_mut().filter(|v| v.on) {
                        s += (v.phase * std::f64::consts::TAU).sin() as f32 * v.velocity;
                        v.phase = (v.phase + v.step).fract();
                    }
                    out_l[i] = s * value * 0.5;
                    out_r[i] = s * value * 0.5;
                }
            }
        }
        kResultOk
    }
    unsafe fn getTailSamples(&self) -> u32 {
        0
    }
}

impl IProcessContextRequirementsTrait for Processor {
    unsafe fn getProcessContextRequirements(&self) -> u32 {
        0
    }
}

struct Controller {
    kind: Kind,
    value: AtomicU64,
}

impl Class for Controller {
    type Interfaces = (IEditController,);
}

impl IPluginBaseTrait for Controller {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IEditControllerTrait for Controller {
    unsafe fn setComponentState(&self, state: *mut IBStream) -> tresult {
        if let Some(v) = unsafe { read_value(state) } {
            self.value.store(v.to_bits(), Ordering::Relaxed);
        }
        kResultOk
    }
    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getParameterCount(&self) -> i32 {
        1
    }
    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        if index != 0 {
            return kInvalidArgument;
        }
        let info = unsafe { &mut *info };
        info.id = 0;
        copy_wstring(self.kind.param_name(), &mut info.title);
        copy_wstring(self.kind.param_name(), &mut info.shortTitle);
        copy_wstring("%", &mut info.units);
        info.stepCount = 0;
        info.defaultNormalizedValue = DEFAULT_VALUE;
        info.unitId = 0;
        info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate as i32;
        kResultOk
    }
    unsafe fn getParamStringByValue(&self, id: u32, value: f64, string: *mut String128) -> tresult {
        if id != 0 {
            return kInvalidArgument;
        }
        let percent = match self.kind {
            Kind::Synth => value * 100.0,
            Kind::Gain => value * 200.0,
        };
        copy_wstring(&format!("{percent:.0}"), unsafe { &mut *string });
        kResultOk
    }
    unsafe fn getParamValueByString(&self, _id: u32, _s: *mut TChar, _v: *mut f64) -> tresult {
        kNotImplemented
    }
    unsafe fn normalizedParamToPlain(&self, _id: u32, value: f64) -> f64 {
        value
    }
    unsafe fn plainParamToNormalized(&self, _id: u32, plain: f64) -> f64 {
        plain
    }
    unsafe fn getParamNormalized(&self, id: u32) -> f64 {
        if id == 0 {
            f64::from_bits(self.value.load(Ordering::Relaxed))
        } else {
            0.0
        }
    }
    unsafe fn setParamNormalized(&self, id: u32, value: f64) -> tresult {
        if id != 0 {
            return kInvalidArgument;
        }
        self.value.store(value.to_bits(), Ordering::Relaxed);
        kResultOk
    }
    unsafe fn setComponentHandler(&self, _handler: *mut IComponentHandler) -> tresult {
        kResultOk
    }
    unsafe fn createView(&self, _name: *const c_char) -> *mut IPlugView {
        ptr::null_mut()
    }
}

struct Factory;

impl Class for Factory {
    type Interfaces = (IPluginFactory2,);
}

/// (cid, category, name, subcategories)
const CLASSES: [(TUID, &str, &str, &str); 4] = [
    (
        SYNTH_PROCESSOR,
        "Audio Module Class",
        "NPT Test Synth",
        "Instrument|Synth",
    ),
    (
        SYNTH_CONTROLLER,
        "Component Controller Class",
        "NPT Test Synth",
        "",
    ),
    (
        GAIN_PROCESSOR,
        "Audio Module Class",
        "NPT Test Gain",
        "Fx|Dynamics",
    ),
    (
        GAIN_CONTROLLER,
        "Component Controller Class",
        "NPT Test Gain",
        "",
    ),
];

impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        let info = unsafe { &mut *info };
        copy_cstring("Nunc Pro Tune", &mut info.vendor);
        copy_cstring(
            "https://github.com/NuncProTunc7/digital-audio-workstation",
            &mut info.url,
        );
        copy_cstring("", &mut info.email);
        info.flags = PFactoryInfo_::FactoryFlags_::kUnicode as int32;
        kResultOk
    }
    unsafe fn countClasses(&self) -> i32 {
        CLASSES.len() as i32
    }
    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        let Some((cid, category, name, _)) = CLASSES.get(index as usize) else {
            return kInvalidArgument;
        };
        let info = unsafe { &mut *info };
        info.cid = *cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring(category, &mut info.category);
        copy_cstring(name, &mut info.name);
        kResultOk
    }
    unsafe fn createInstance(
        &self,
        cid: FIDString,
        iid: FIDString,
        obj: *mut *mut c_void,
    ) -> tresult {
        let cid = unsafe { *(cid as *const TUID) };
        let instance = match cid {
            SYNTH_PROCESSOR => {
                ComWrapper::new(Processor::new(Kind::Synth)).to_com_ptr::<FUnknown>()
            }
            GAIN_PROCESSOR => ComWrapper::new(Processor::new(Kind::Gain)).to_com_ptr::<FUnknown>(),
            SYNTH_CONTROLLER | GAIN_CONTROLLER => ComWrapper::new(Controller {
                kind: if cid == SYNTH_CONTROLLER {
                    Kind::Synth
                } else {
                    Kind::Gain
                },
                value: AtomicU64::new(DEFAULT_VALUE.to_bits()),
            })
            .to_com_ptr::<FUnknown>(),
            _ => None,
        };
        let Some(instance) = instance else {
            return kInvalidArgument;
        };
        let p = instance.as_ptr();
        unsafe { ((*(*p).vtbl).queryInterface)(p, iid as *mut TUID, obj) }
    }
}

impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: i32, info: *mut PClassInfo2) -> tresult {
        let Some((cid, category, name, sub)) = CLASSES.get(index as usize) else {
            return kInvalidArgument;
        };
        let info = unsafe { &mut *info };
        info.cid = *cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring(category, &mut info.category);
        copy_cstring(name, &mut info.name);
        info.classFlags = 0;
        copy_cstring(sub, &mut info.subCategories);
        copy_cstring("Nunc Pro Tune", &mut info.vendor);
        copy_cstring("1.0.0", &mut info.version);
        copy_cstring("VST 3.8.0", &mut info.sdkVersion);
        kResultOk
    }
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
extern "system" fn InitDll() -> bool {
    true
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
extern "system" fn ExitDll() -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleEntry(_library_handle: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleExit() -> bool {
    true
}

/// The VST3 entry point.
#[unsafe(no_mangle)]
pub extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    ComWrapper::new(Factory)
        .to_com_ptr::<IPluginFactory>()
        .map_or(ptr::null_mut(), |p| p.into_raw())
}
