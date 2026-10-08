//! A running plugin: created and saved on the main thread
//! ([`Instance`]), played on the audio thread ([`PluginProcessor`]).

use std::sync::{Arc, Mutex};

use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{ComPtr, ComWrapper, Interface};

use crate::com::{
    ComponentHandler, Edit, EditSink, EventList, HostApplication, MemoryStream, ParameterChanges,
    Shared, from_wide,
};
use crate::{Module, PluginInfo, PluginKind, main_thread};

/// One of a plugin's parameters, as its controller describes it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ParamInfo {
    pub id: u32,
    pub name: String,
    pub units: String,
    /// Current value, 0–1.
    pub value: f64,
    pub default: f64,
    /// What the plugin shows for the current value, e.g. "-6.0 dB".
    pub display: String,
    /// 0 = continuous; otherwise the number of steps (1 = on/off).
    pub steps: i32,
    pub automatable: bool,
}

/// The COM objects of one plugin instance, released together.
struct Parts {
    component: Shared<IComponent>,
    processor: Shared<IAudioProcessor>,
    controller: Option<Shared<IEditController>>,
    /// The controller is its own object (most plugins), to terminate too.
    separate_controller: bool,
    connection: Option<(Shared<IConnectionPoint>, Shared<IConnectionPoint>)>,
    // Kept alive while the plugin may call them.
    _host: ComWrapper<HostApplication>,
    // Last, so the code stays loaded until everything else is released.
    _module: Arc<Module>,
}

#[allow(unsafe_code)]
fn teardown(p: Parts) {
    // SAFETY: plain VST3 shutdown calls on live objects, in the documented
    // order (stop processing, deactivate, disconnect, terminate).
    unsafe {
        p.processor.0.setProcessing(0);
        p.component.0.setActive(0);
        if let Some((a, b)) = &p.connection {
            a.0.disconnect(b.0.as_ptr());
            b.0.disconnect(a.0.as_ptr());
        }
        if let Some(c) = &p.controller {
            c.0.setComponentHandler(std::ptr::null_mut());
            if p.separate_controller {
                c.0.terminate();
            }
        }
        p.component.0.terminate();
    }
    drop(p);
}

struct Inner {
    parts: Option<Parts>,
    handler: ComWrapper<ComponentHandler>,
    info: PluginInfo,
    shape: BusShape,
    setup: Setup,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(parts) = self.parts.take() {
            let parts = Shared2(parts);
            main_thread::post(move || teardown(parts.0));
        }
    }
}

/// Lets `Parts` cross to the main thread for teardown.
struct Shared2(Parts);
// SAFETY: every field is a Shared COM pointer or Send already.
#[allow(unsafe_code)]
unsafe impl Send for Shared2 {}

/// A plugin instance (cheap to clone; the plugin is released when the last
/// clone and its processor are gone).
#[derive(Clone)]
pub struct Instance(Arc<Inner>);

/// What to create.
#[derive(Debug, Clone, Copy)]
pub struct Setup {
    pub sample_rate_hz: f64,
    /// Largest block `process` will be given.
    pub max_block: usize,
    /// Rendering to a file rather than playing live (plugins may use
    /// higher quality, and may take their time).
    pub offline: bool,
}

const STATE_MAGIC: &[u8; 4] = b"NPT1";

/// Packs component and controller state into one blob.
fn pack_state(component: &[u8], controller: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + component.len() + controller.len());
    out.extend_from_slice(STATE_MAGIC);
    out.extend_from_slice(&(component.len() as u32).to_le_bytes());
    out.extend_from_slice(component);
    out.extend_from_slice(&(controller.len() as u32).to_le_bytes());
    out.extend_from_slice(controller);
    out
}

fn unpack_state(blob: &[u8]) -> Option<(&[u8], &[u8])> {
    let rest = blob.strip_prefix(STATE_MAGIC.as_slice())?;
    let take = |b: &[u8]| -> Option<(usize, usize)> {
        let n = u32::from_le_bytes(b.get(..4)?.try_into().ok()?) as usize;
        (b.len() >= 4 + n).then_some((4, 4 + n))
    };
    let (a0, a1) = take(rest)?;
    let comp = &rest[a0..a1];
    let rest = &rest[a1..];
    let (b0, b1) = take(rest)?;
    Some((comp, &rest[b0..b1]))
}

fn check(r: tresult, what: &str) -> Result<(), String> {
    if r == kResultOk {
        Ok(())
    } else {
        Err(format!("the plugin refused to {what}"))
    }
}

/// Channels in a speaker arrangement (one bit per speaker).
fn channel_count(arr: SpeakerArrangement) -> usize {
    arr.count_ones() as usize
}

impl Instance {
    /// Creates `info` ready to play, restoring `state` (from
    /// [`Instance::state`]) if given. Runs on the main thread.
    pub fn create(
        info: &PluginInfo,
        setup: Setup,
        state: Option<Vec<u8>>,
    ) -> Result<(Instance, PluginProcessor), String> {
        if crate::guard::is_blocked(&info.uid) {
            return Err(format!(
                "{} is switched off because it closed the app the last time it started;                  after updating or reinstalling it, choose Look for new plugins to try again",
                info.name
            ));
        }
        let info = info.clone();
        main_thread::run(move || {
            let watched = info.clone();
            crate::guard::watch(&watched, || Self::create_here(info, setup, state))
        })?
    }

    #[allow(unsafe_code)]
    fn create_here(
        info: PluginInfo,
        setup: Setup,
        state: Option<Vec<u8>>,
    ) -> Result<(Instance, PluginProcessor), String> {
        let module = Module::open(std::path::Path::new(&info.path))?;
        let cid = crate::parse_uid(&info.uid).ok_or("the plugin's id is damaged")?;
        let factory = module.factory()?;
        let host = ComWrapper::new(HostApplication);
        let host_ptr = host
            .as_com_ref::<IHostApplication>()
            .ok_or("host")?
            .as_ptr() as *mut FUnknown;
        let handler = ComWrapper::new(ComponentHandler {
            sink: Mutex::new(None),
        });

        // SAFETY: standard VST3 instantiation on live objects with valid
        // pointers; every created object is owned by a ComPtr.
        unsafe {
            let mut obj = std::ptr::null_mut();
            check(
                factory.createInstance(
                    cid.as_ptr() as FIDString,
                    IComponent::IID.as_ptr() as FIDString,
                    &mut obj,
                ),
                "start",
            )?;
            let component =
                ComPtr::<IComponent>::from_raw(obj as *mut IComponent).ok_or("no component")?;
            check(component.initialize(host_ptr), "start")?;

            // The controller: the component itself, or a separate class.
            let (controller, separate) = match component.cast::<IEditController>() {
                Some(c) => (Some(c), false),
                None => {
                    let mut ccid: TUID = [0; 16];
                    let mut ctrl = None;
                    if component.getControllerClassId(&mut ccid) == kResultOk {
                        let mut obj = std::ptr::null_mut();
                        if factory.createInstance(
                            ccid.as_ptr() as FIDString,
                            IEditController::IID.as_ptr() as FIDString,
                            &mut obj,
                        ) == kResultOk
                            && let Some(c) =
                                ComPtr::<IEditController>::from_raw(obj as *mut IEditController)
                            && c.initialize(host_ptr) == kResultOk
                        {
                            ctrl = Some(c);
                        }
                    }
                    (ctrl, true)
                }
            };
            if let Some(c) = &controller
                && let Some(h) = handler.as_com_ref::<IComponentHandler>()
            {
                c.setComponentHandler(h.as_ptr());
            }
            let connection = match (&controller, separate) {
                (Some(c), true) => match (
                    component.cast::<IConnectionPoint>(),
                    c.cast::<IConnectionPoint>(),
                ) {
                    (Some(a), Some(b)) => {
                        a.connect(b.as_ptr());
                        b.connect(a.as_ptr());
                        Some((Shared(a), Shared(b)))
                    }
                    _ => None,
                },
                _ => None,
            };

            // State: restore it, or sync the controller with the defaults.
            let (comp_state, ctrl_state) = match state.as_deref().and_then(unpack_state) {
                Some((a, b)) => (Some(a.to_vec()), Some(b.to_vec())),
                None => (None, None),
            };
            if let Some(bytes) = &comp_state {
                let s = MemoryStream::new(bytes.clone());
                let p = s.as_com_ref::<IBStream>().ok_or("stream")?.as_ptr();
                component.setState(p);
            }
            if let Some(c) = &controller {
                let s = MemoryStream::new(Vec::new());
                let p = s.as_com_ref::<IBStream>().ok_or("stream")?.as_ptr();
                if component.getState(p) == kResultOk {
                    let r = MemoryStream::new(s.bytes());
                    c.setComponentState(r.as_com_ref::<IBStream>().ok_or("stream")?.as_ptr());
                }
                if let Some(bytes) = ctrl_state.filter(|b| !b.is_empty()) {
                    let s = MemoryStream::new(bytes);
                    c.setState(s.as_com_ref::<IBStream>().ok_or("stream")?.as_ptr());
                }
            }

            let processor = component
                .cast::<IAudioProcessor>()
                .ok_or("the plugin can't process audio")?;
            let shape = BusShape::negotiate(&component, &processor);
            for (media, dir, count) in [
                (
                    MediaTypes_::kAudio,
                    BusDirections_::kInput,
                    shape.inputs.len(),
                ),
                (
                    MediaTypes_::kAudio,
                    BusDirections_::kOutput,
                    shape.outputs.len(),
                ),
                (
                    MediaTypes_::kEvent,
                    BusDirections_::kInput,
                    component
                        .getBusCount(
                            MediaTypes_::kEvent as MediaType,
                            BusDirections_::kInput as BusDirection,
                        )
                        .max(0) as usize,
                ),
            ] {
                // Main buses on; extra ones (sidechains, extra outs) are
                // given silence and ignored.
                for i in 0..count {
                    component.activateBus(
                        media as MediaType,
                        dir as BusDirection,
                        i as i32,
                        (i == 0) as TBool,
                    );
                }
            }
            let mut ps = ProcessSetup {
                processMode: if setup.offline {
                    ProcessModes_::kOffline as i32
                } else {
                    ProcessModes_::kRealtime as i32
                },
                symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
                maxSamplesPerBlock: setup.max_block as i32,
                sampleRate: setup.sample_rate_hz,
            };
            check(processor.setupProcessing(&mut ps), "set up processing")?;
            check(component.setActive(1), "switch on")?;
            // Some plugins return kNotImplemented here; that's fine.
            processor.setProcessing(1);

            let parts = Parts {
                component: Shared(component),
                processor: Shared(processor.clone()),
                controller: controller.map(Shared),
                separate_controller: separate,
                connection,
                _host: host,
                _module: module,
            };
            let instance = Instance(Arc::new(Inner {
                parts: Some(parts),
                handler,
                info: info.clone(),
                shape,
                setup,
            }));
            let rt = instance.processor();
            Ok((instance, rt))
        }
    }

    pub fn info(&self) -> &PluginInfo {
        &self.0.info
    }

    /// Starts a plugin as a song saved it: its settings (base64, from
    /// [`Instance::state`]) and then its parameter values on top.
    pub fn start(
        info: &PluginInfo,
        setup: Setup,
        state_base64: Option<&str>,
        params: &[(u32, f64)],
    ) -> Result<(Instance, PluginProcessor), String> {
        use base64::Engine as _;
        let state =
            state_base64.and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok());
        let (instance, mut rt) = Instance::create(info, setup, state)?;
        instance.apply_params(&mut rt, params)?;
        Ok((instance, rt))
    }

    /// Whether two handles are the very same running plugin.
    pub fn same_as(&self, other: &Instance) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Another audio-thread handle for this same plugin, for when the
    /// engine rebuilds its tracks: the plugin keeps its sound and settings.
    /// Only one handle may process at a time (the old one stops when the
    /// new graph takes over). Starts by releasing any held notes.
    pub fn processor(&self) -> PluginProcessor {
        let processor = self
            .parts()
            .map(|p| p.processor.0.clone())
            .expect("a live instance has a processor");
        let mut rt = PluginProcessor::new(
            Shared(processor),
            self.clone(),
            self.0.shape.clone(),
            self.0.setup,
            self.0.info.kind,
        );
        rt.release_everything();
        rt
    }

    /// Sets parameters that differ from the plugin's current values, both
    /// in its window and its sound, before it starts playing. Main thread
    /// (call before handing `rt` to the audio thread).
    pub fn apply_params(
        &self,
        rt: &mut PluginProcessor,
        params: &[(u32, f64)],
    ) -> Result<(), String> {
        let current = self.params_including_hidden()?;
        let changed: Vec<(u32, f64)> = params
            .iter()
            .copied()
            .filter(|(id, v)| {
                current
                    .iter()
                    .any(|(cid, cv)| cid == id && (cv - v).abs() > 1e-9)
            })
            .collect();
        for chunk in changed.chunks(crate::com::MAX_PARAM_QUEUES) {
            for &(id, v) in chunk {
                rt.set_param(id, v);
                self.show_param(id, v);
            }
            // A one-sample silent block delivers the queued changes.
            let (mut l, mut r) = ([0.0f32], [0.0f32]);
            rt.process(&mut l, &mut r);
        }
        Ok(())
    }

    /// Every parameter's current value, hidden ones included. Main thread.
    pub fn values(&self) -> Result<Vec<(u32, f64)>, String> {
        self.params_including_hidden()
    }

    #[allow(unsafe_code)]
    fn params_including_hidden(&self) -> Result<Vec<(u32, f64)>, String> {
        let me = self.clone();
        main_thread::run(move || {
            let Some(c) = me
                .parts()
                .ok()
                .and_then(|p| p.controller.as_ref().map(|c| c.0.clone()))
            else {
                return Vec::new();
            };
            let mut out = Vec::new();
            // SAFETY: read-only controller queries.
            unsafe {
                for i in 0..c.getParameterCount() {
                    let mut p: ParameterInfo = std::mem::zeroed();
                    if c.getParameterInfo(i, &mut p) == kResultOk {
                        out.push((p.id, c.getParamNormalized(p.id)));
                    }
                }
            }
            out
        })
    }

    fn parts(&self) -> Result<&Parts, String> {
        self.0
            .parts
            .as_ref()
            .ok_or_else(|| "the plugin was closed".into())
    }

    /// The plugin's full state (to save in the song). Main thread.
    pub fn state(&self) -> Result<Vec<u8>, String> {
        let me = self.clone();
        main_thread::run(move || me.state_here())?
    }

    #[allow(unsafe_code)]
    fn state_here(&self) -> Result<Vec<u8>, String> {
        let p = self.parts()?;
        let comp = MemoryStream::new(Vec::new());
        let ctrl = MemoryStream::new(Vec::new());
        // SAFETY: getState writes into our streams.
        unsafe {
            check(
                p.component
                    .0
                    .getState(comp.as_com_ref::<IBStream>().ok_or("stream")?.as_ptr()),
                "save its settings",
            )?;
            if let Some(c) = &p.controller {
                c.0.getState(ctrl.as_com_ref::<IBStream>().ok_or("stream")?.as_ptr());
            }
        }
        Ok(pack_state(&comp.bytes(), &ctrl.bytes()))
    }

    /// Every parameter with its current value. Main thread.
    pub fn params(&self) -> Result<Vec<ParamInfo>, String> {
        let me = self.clone();
        main_thread::run(move || me.params_here())?
    }

    #[allow(unsafe_code)]
    fn params_here(&self) -> Result<Vec<ParamInfo>, String> {
        let Some(c) = &self.parts()?.controller else {
            return Ok(Vec::new());
        };
        let c = &c.0;
        let mut out = Vec::new();
        // SAFETY: read-only controller queries with valid out-structs.
        unsafe {
            for i in 0..c.getParameterCount() {
                let mut p: ParameterInfo = std::mem::zeroed();
                if c.getParameterInfo(i, &mut p) != kResultOk {
                    continue;
                }
                let flags = p.flags;
                let hidden = ParameterInfo_::ParameterFlags_::kIsHidden as i32;
                let read_only = ParameterInfo_::ParameterFlags_::kIsReadOnly as i32;
                if flags & (hidden | read_only) != 0 {
                    continue;
                }
                let value = c.getParamNormalized(p.id);
                let mut text: String128 = [0; 128];
                let display = if c.getParamStringByValue(p.id, value, &mut text) == kResultOk {
                    from_wide(&text)
                } else {
                    format!("{value:.3}")
                };
                out.push(ParamInfo {
                    id: p.id,
                    name: from_wide(&p.title),
                    units: from_wide(&p.units),
                    value,
                    default: p.defaultNormalizedValue,
                    display,
                    steps: p.stepCount,
                    automatable: flags & ParameterInfo_::ParameterFlags_::kCanAutomate as i32 != 0,
                });
            }
        }
        Ok(out)
    }

    /// Shows a parameter change in the plugin's window (the sound changes
    /// through [`PluginProcessor::set_param`]).
    #[allow(unsafe_code)]
    pub fn show_param(&self, id: u32, value: f64) {
        let me = self.clone();
        main_thread::post(move || {
            if let Ok(Parts {
                controller: Some(c),
                ..
            }) = me.parts()
            {
                // SAFETY: plain controller call on the main thread.
                unsafe { c.0.setParamNormalized(id, value.clamp(0.0, 1.0)) };
            }
        });
    }

    /// Where edits made in the plugin's own window go.
    pub fn on_edit(&self, sink: impl Fn(Edit) + Send + Sync + 'static) {
        if let Ok(mut g) = self.0.handler.sink.lock() {
            *g = Some(Box::new(sink) as EditSink);
        }
    }

    /// The controller, for opening the plugin's window.
    pub(crate) fn controller(&self) -> Option<ComPtr<IEditController>> {
        self.parts().ok()?.controller.as_ref().map(|c| c.0.clone())
    }
}

/// Channel counts of each audio bus, agreed with the plugin.
#[derive(Debug, Clone)]
struct BusShape {
    inputs: Vec<usize>,
    outputs: Vec<usize>,
}

impl BusShape {
    /// Asks for stereo everywhere, then takes what the plugin settled on.
    #[allow(unsafe_code)]
    unsafe fn negotiate(
        component: &ComPtr<IComponent>,
        processor: &ComPtr<IAudioProcessor>,
    ) -> BusShape {
        // SAFETY: plain bus queries with valid arrays.
        unsafe {
            let count = |dir: BusDirections| {
                component
                    .getBusCount(MediaTypes_::kAudio as MediaType, dir as BusDirection)
                    .max(0) as usize
            };
            let (n_in, n_out) = (
                count(BusDirections_::kInput),
                count(BusDirections_::kOutput),
            );
            let mut ins = vec![SpeakerArr::kStereo; n_in];
            let mut outs = vec![SpeakerArr::kStereo; n_out];
            processor.setBusArrangements(
                ins.as_mut_ptr(),
                n_in as i32,
                outs.as_mut_ptr(),
                n_out as i32,
            );
            let read = |dir: BusDirections, n: usize| {
                (0..n)
                    .map(|i| {
                        let mut arr = SpeakerArr::kStereo;
                        if processor.getBusArrangement(dir as BusDirection, i as i32, &mut arr)
                            != kResultOk
                        {
                            arr = SpeakerArr::kStereo;
                        }
                        channel_count(arr).max(1)
                    })
                    .collect()
            };
            BusShape {
                inputs: read(BusDirections_::kInput, n_in),
                outputs: read(BusDirections_::kOutput, n_out),
            }
        }
    }
}

/// The audio-thread side of a plugin. Everything is allocated up front;
/// its methods never allocate or lock (what the plugin does inside its own
/// `process` is up to the plugin).
pub struct PluginProcessor {
    processor: Shared<IAudioProcessor>,
    events: ComWrapper<EventList>,
    params: ComWrapper<ParameterChanges>,
    /// Sample storage per bus, channel-major (`max_block` per channel).
    in_data: Vec<Vec<f32>>,
    out_data: Vec<Vec<f32>>,
    in_ptrs: Vec<Vec<*mut f32>>,
    out_ptrs: Vec<Vec<*mut f32>>,
    in_buses: Vec<AudioBusBuffers>,
    out_buses: Vec<AudioBusBuffers>,
    context: ProcessContext,
    shape: BusShape,
    max_block: usize,
    offline: bool,
    kind: PluginKind,
    held: [bool; 128],
    // Keeps the plugin alive while it plays; dropped off the audio thread
    // with the rest of the old graph.
    _instance: Instance,
}

// SAFETY: the raw pointers point into this struct's own buffers; the
// processor is used only from the thread that owns this value.
#[allow(unsafe_code)]
unsafe impl Send for PluginProcessor {}

impl PluginProcessor {
    #[allow(unsafe_code)]
    fn new(
        processor: Shared<IAudioProcessor>,
        instance: Instance,
        shape: BusShape,
        setup: Setup,
        kind: PluginKind,
    ) -> Self {
        let max = setup.max_block.max(1);
        let in_data: Vec<Vec<f32>> = shape.inputs.iter().map(|&ch| vec![0.0; ch * max]).collect();
        let out_data: Vec<Vec<f32>> = shape
            .outputs
            .iter()
            .map(|&ch| vec![0.0; ch * max])
            .collect();
        // SAFETY: zeroed POD structs, filled before use.
        let blank_bus: AudioBusBuffers = unsafe { std::mem::zeroed() };
        let mut context: ProcessContext = unsafe { std::mem::zeroed() };
        context.sampleRate = setup.sample_rate_hz;
        context.tempo = 120.0;
        context.timeSigNumerator = 4;
        context.timeSigDenominator = 4;
        let mut me = PluginProcessor {
            processor,
            events: EventList::new(),
            params: ParameterChanges::new(),
            in_ptrs: shape
                .inputs
                .iter()
                .map(|&ch| vec![std::ptr::null_mut(); ch])
                .collect(),
            out_ptrs: shape
                .outputs
                .iter()
                .map(|&ch| vec![std::ptr::null_mut(); ch])
                .collect(),
            in_buses: vec![blank_bus; shape.inputs.len()],
            out_buses: vec![blank_bus; shape.outputs.len()],
            in_data,
            out_data,
            context,
            shape,
            max_block: max,
            offline: setup.offline,
            kind,
            held: [false; 128],
            _instance: instance,
        };
        // The Vecs never move or grow, so pointers into them stay valid.
        for (b, data) in me.in_data.iter_mut().enumerate() {
            for (c, p) in me.in_ptrs[b].iter_mut().enumerate() {
                *p = data[c * max..].as_mut_ptr();
            }
        }
        for (b, data) in me.out_data.iter_mut().enumerate() {
            for (c, p) in me.out_ptrs[b].iter_mut().enumerate() {
                *p = data[c * max..].as_mut_ptr();
            }
        }
        for (bus, ptrs) in me.in_buses.iter_mut().zip(me.in_ptrs.iter_mut()) {
            bus.numChannels = ptrs.len() as i32;
            bus.__field0.channelBuffers32 = ptrs.as_mut_ptr();
        }
        for (bus, ptrs) in me.out_buses.iter_mut().zip(me.out_ptrs.iter_mut()) {
            bus.numChannels = ptrs.len() as i32;
            bus.__field0.channelBuffers32 = ptrs.as_mut_ptr();
        }
        me
    }

    pub fn kind(&self) -> PluginKind {
        self.kind
    }

    #[allow(unsafe_code)]
    fn note_event(&self, on: bool, note: u8, velocity: f32) {
        // SAFETY: zeroed POD event, filled below.
        let mut e: Event = unsafe { std::mem::zeroed() };
        e.busIndex = 0;
        e.sampleOffset = 0;
        e.flags = Event_::EventFlags_::kIsLive as u16;
        if on {
            e.r#type = Event_::EventTypes_::kNoteOnEvent as u16;
            e.__field0.noteOn = NoteOnEvent {
                channel: 0,
                pitch: note as i16,
                tuning: 0.0,
                velocity,
                length: 0,
                noteId: -1,
            };
        } else {
            e.r#type = Event_::EventTypes_::kNoteOffEvent as u16;
            e.__field0.noteOff = NoteOffEvent {
                channel: 0,
                pitch: note as i16,
                velocity: 0.0,
                noteId: -1,
                tuning: 0.0,
            };
        }
        self.events.push(e);
    }

    /// `velocity` is 0–1. Played at the start of the next block.
    // RT-SAFE
    pub fn note_on(&mut self, note: u8, velocity: f32) {
        if let Some(h) = self.held.get_mut(note as usize) {
            *h = true;
        }
        self.note_event(true, note, velocity.clamp(0.0, 1.0));
    }

    // RT-SAFE
    pub fn note_off(&mut self, note: u8) {
        if let Some(h) = self.held.get_mut(note as usize) {
            *h = false;
        }
        self.note_event(false, note, 0.0);
    }

    /// Note-offs for every note, held or not (a new handle doesn't know
    /// what the old one left sounding).
    // RT-SAFE
    pub fn release_everything(&mut self) {
        for n in 0..128u8 {
            self.note_off(n);
        }
    }

    // RT-SAFE
    pub fn all_notes_off(&mut self) {
        for n in 0..128u8 {
            if self.held[n as usize] {
                self.note_off(n);
            }
        }
    }

    /// Sets parameter `id` (0–1) from the next block on.
    // RT-SAFE
    pub fn set_param(&mut self, id: u32, value: f64) {
        self.params.set(id, value.clamp(0.0, 1.0));
    }

    /// Tempo for tempo-synced effects.
    // RT-SAFE
    pub fn set_tempo(&mut self, bpm: f64) {
        self.context.tempo = bpm;
    }

    /// Instruments: adds the plugin's output into `left`/`right`.
    /// Effects: replaces `left`/`right` with the processed signal.
    // RT-SAFE
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut done = 0;
        while done < n {
            let len = (n - done).min(self.max_block);
            let (l, r) = (&mut left[done..done + len], &mut right[done..done + len]);
            self.process_block(l, r);
            done += len;
        }
    }

    #[allow(unsafe_code)]
    // RT-SAFE
    fn process_block(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        let max = self.max_block;
        // Inputs: the signal on the main bus (effects), silence elsewhere.
        for (b, data) in self.in_data.iter_mut().enumerate() {
            let ch = self.shape.inputs[b];
            for c in 0..ch {
                let dst = &mut data[c * max..c * max + n];
                if b == 0 && self.kind == PluginKind::Effect {
                    dst.copy_from_slice(if c == 0 { left } else { right });
                } else {
                    dst.fill(0.0);
                }
            }
        }
        for data in &mut self.out_data {
            data.fill(0.0);
        }
        let events = self
            .events
            .as_com_ref::<IEventList>()
            .map_or(std::ptr::null_mut(), |r| r.as_ptr());
        let params = self
            .params
            .as_com_ref::<IParameterChanges>()
            .map_or(std::ptr::null_mut(), |r| r.as_ptr());
        let mut data = ProcessData {
            processMode: if self.offline {
                ProcessModes_::kOffline as i32
            } else {
                ProcessModes_::kRealtime as i32
            },
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
            numSamples: n as i32,
            numInputs: self.in_buses.len() as i32,
            numOutputs: self.out_buses.len() as i32,
            inputs: if self.in_buses.is_empty() {
                std::ptr::null_mut()
            } else {
                self.in_buses.as_mut_ptr()
            },
            outputs: if self.out_buses.is_empty() {
                std::ptr::null_mut()
            } else {
                self.out_buses.as_mut_ptr()
            },
            inputParameterChanges: params,
            outputParameterChanges: std::ptr::null_mut(),
            inputEvents: events,
            outputEvents: std::ptr::null_mut(),
            processContext: &mut self.context,
        };
        // SAFETY: every pointer in `data` points into buffers this struct
        // owns, sized for max_block >= n.
        unsafe { self.processor.0.process(&mut data) };
        self.events.clear();
        self.params.clear();
        self.context.projectTimeSamples += n as i64;

        let Some(out) = self.out_data.first() else {
            return;
        };
        let ch = self.shape.outputs[0];
        let l = &out[..n];
        let r = if ch > 1 { &out[max..max + n] } else { l };
        match self.kind {
            PluginKind::Instrument => {
                for i in 0..n {
                    left[i] += l[i];
                    right[i] += r[i];
                }
            }
            PluginKind::Effect => {
                left.copy_from_slice(l);
                right.copy_from_slice(r);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_blobs_round_trip() {
        let blob = pack_state(b"abc", b"");
        assert_eq!(
            unpack_state(&blob),
            Some((b"abc".as_slice(), b"".as_slice()))
        );
        assert_eq!(unpack_state(b"junk"), None);
        assert_eq!(unpack_state(&blob[..6]), None);
    }
}
