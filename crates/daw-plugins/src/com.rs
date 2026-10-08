//! The objects a VST3 host hands to plugins: a byte stream for saving and
//! loading state, messages between a plugin's two halves, the host
//! application, the edit handler, and the real-time event and parameter
//! queues.

use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::sync::Mutex;

use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper, Interface};

pub(crate) fn guid_of(tuid: &TUID) -> [u8; 16] {
    tuid.map(|b| b as u8)
}

/// A COM pointer moved between threads. VST3 says which thread may call
/// what; the wrappers here only carry the pointer.
pub(crate) struct Shared<I: Interface>(pub ComPtr<I>);

// SAFETY: VST3 objects are reference counted with atomic counters and may be
// handed between threads; callers follow VST3's threading rules for calls.
#[allow(unsafe_code)]
unsafe impl<I: Interface> Send for Shared<I> {}
// SAFETY: as above.
#[allow(unsafe_code)]
unsafe impl<I: Interface> Sync for Shared<I> {}

/// Reads a NUL-terminated UTF-16 string.
pub(crate) fn from_wide(s: &[TChar]) -> String {
    let len = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf16_lossy(&s[..len].iter().map(|&c| c as u16).collect::<Vec<_>>())
}

/// Reads a NUL-terminated byte string.
pub(crate) fn from_c(s: &[c_char]) -> String {
    let bytes: Vec<u8> = s
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn write_wide(src: &str, dst: &mut [TChar]) {
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

/// An in-memory `IBStream` for plugin state.
pub(crate) struct MemoryStream {
    inner: Mutex<(Vec<u8>, usize)>,
}

impl Class for MemoryStream {
    type Interfaces = (IBStream,);
}

impl MemoryStream {
    pub fn new(bytes: Vec<u8>) -> ComWrapper<MemoryStream> {
        ComWrapper::new(MemoryStream {
            inner: Mutex::new((bytes, 0)),
        })
    }
    pub fn bytes(&self) -> Vec<u8> {
        self.inner.lock().map(|g| g.0.clone()).unwrap_or_default()
    }
}

#[allow(unsafe_code)]
impl IBStreamTrait for MemoryStream {
    unsafe fn read(&self, buffer: *mut c_void, num_bytes: int32, num_read: *mut int32) -> tresult {
        let Ok(mut g) = self.inner.lock() else {
            return kResultFalse;
        };
        let (data, pos) = &mut *g;
        let n = (num_bytes.max(0) as usize).min(data.len().saturating_sub(*pos));
        // SAFETY: the plugin gives a buffer of at least num_bytes bytes.
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr().add(*pos), buffer as *mut u8, n) };
        *pos += n;
        if !num_read.is_null() {
            // SAFETY: non-null out-pointer from the plugin.
            unsafe { *num_read = n as int32 };
        }
        kResultOk
    }
    unsafe fn write(
        &self,
        buffer: *mut c_void,
        num_bytes: int32,
        num_written: *mut int32,
    ) -> tresult {
        let Ok(mut g) = self.inner.lock() else {
            return kResultFalse;
        };
        let (data, pos) = &mut *g;
        let n = num_bytes.max(0) as usize;
        if data.len() < *pos + n {
            data.resize(*pos + n, 0);
        }
        // SAFETY: the plugin gives num_bytes readable bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(buffer as *const u8, data.as_mut_ptr().add(*pos), n)
        };
        *pos += n;
        if !num_written.is_null() {
            // SAFETY: non-null out-pointer from the plugin.
            unsafe { *num_written = n as int32 };
        }
        kResultOk
    }
    unsafe fn seek(&self, pos: int64, mode: int32, result: *mut int64) -> tresult {
        let Ok(mut g) = self.inner.lock() else {
            return kResultFalse;
        };
        let base = match mode as IBStream_::IStreamSeekMode {
            IBStream_::IStreamSeekMode_::kIBSeekSet => 0,
            IBStream_::IStreamSeekMode_::kIBSeekCur => g.1 as i64,
            IBStream_::IStreamSeekMode_::kIBSeekEnd => g.0.len() as i64,
            _ => return kInvalidArgument,
        };
        let new = (base + pos).max(0) as usize;
        g.1 = new;
        if !result.is_null() {
            // SAFETY: non-null out-pointer from the plugin.
            unsafe { *result = new as int64 };
        }
        kResultOk
    }
    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        let Ok(g) = self.inner.lock() else {
            return kResultFalse;
        };
        if !pos.is_null() {
            // SAFETY: non-null out-pointer from the plugin.
            unsafe { *pos = g.1 as int64 };
        }
        kResultOk
    }
}

#[derive(Clone)]
enum Attr {
    Int(i64),
    Float(f64),
    Str(Vec<TChar>),
    Bin(Vec<u8>),
}

/// Key/value attributes carried by a message.
pub(crate) struct AttributeList {
    values: Mutex<HashMap<String, Attr>>,
}

impl Class for AttributeList {
    type Interfaces = (IAttributeList,);
}

#[allow(unsafe_code)]
fn attr_key(id: *const c_char) -> Option<String> {
    if id.is_null() {
        return None;
    }
    // SAFETY: attribute ids are NUL-terminated C strings.
    Some(unsafe { CStr::from_ptr(id) }.to_string_lossy().into_owned())
}

impl AttributeList {
    fn set(&self, id: *const c_char, v: Attr) -> tresult {
        match (attr_key(id), self.values.lock()) {
            (Some(k), Ok(mut m)) => {
                m.insert(k, v);
                kResultOk
            }
            _ => kInvalidArgument,
        }
    }
    fn get(&self, id: *const c_char) -> Option<Attr> {
        let k = attr_key(id)?;
        self.values.lock().ok()?.get(&k).cloned()
    }
}

#[allow(unsafe_code)]
impl IAttributeListTrait for AttributeList {
    unsafe fn setInt(&self, id: IAttributeList_::AttrID, value: int64) -> tresult {
        self.set(id, Attr::Int(value))
    }
    unsafe fn getInt(&self, id: IAttributeList_::AttrID, value: *mut int64) -> tresult {
        match self.get(id) {
            Some(Attr::Int(v)) if !value.is_null() => {
                // SAFETY: non-null out-pointer.
                unsafe { *value = v };
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setFloat(&self, id: IAttributeList_::AttrID, value: f64) -> tresult {
        self.set(id, Attr::Float(value))
    }
    unsafe fn getFloat(&self, id: IAttributeList_::AttrID, value: *mut f64) -> tresult {
        match self.get(id) {
            Some(Attr::Float(v)) if !value.is_null() => {
                // SAFETY: non-null out-pointer.
                unsafe { *value = v };
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setString(&self, id: IAttributeList_::AttrID, string: *const TChar) -> tresult {
        if string.is_null() {
            return kInvalidArgument;
        }
        let mut s = Vec::new();
        // SAFETY: NUL-terminated UTF-16 from the plugin.
        unsafe {
            let mut p = string;
            while *p != 0 {
                s.push(*p);
                p = p.add(1);
            }
        }
        s.push(0);
        self.set(id, Attr::Str(s))
    }
    unsafe fn getString(
        &self,
        id: IAttributeList_::AttrID,
        string: *mut TChar,
        size_in_bytes: uint32,
    ) -> tresult {
        match self.get(id) {
            Some(Attr::Str(s)) if !string.is_null() => {
                let cap = size_in_bytes as usize / std::mem::size_of::<TChar>();
                let n = s.len().min(cap);
                // SAFETY: the plugin's buffer holds size_in_bytes bytes.
                unsafe {
                    std::ptr::copy_nonoverlapping(s.as_ptr(), string, n);
                    if cap > 0 {
                        *string.add(n.min(cap - 1)) = 0;
                    }
                }
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setBinary(
        &self,
        id: IAttributeList_::AttrID,
        data: *const c_void,
        size_in_bytes: uint32,
    ) -> tresult {
        if data.is_null() && size_in_bytes > 0 {
            return kInvalidArgument;
        }
        let bytes = if size_in_bytes == 0 {
            Vec::new()
        } else {
            // SAFETY: the plugin gives size_in_bytes readable bytes.
            unsafe { std::slice::from_raw_parts(data as *const u8, size_in_bytes as usize) }
                .to_vec()
        };
        self.set(id, Attr::Bin(bytes))
    }
    unsafe fn getBinary(
        &self,
        id: IAttributeList_::AttrID,
        data: *mut *const c_void,
        size_in_bytes: *mut uint32,
    ) -> tresult {
        // The pointer must stay valid while the message lives, so hand out
        // the stored buffer itself.
        let Some(k) = attr_key(id) else {
            return kInvalidArgument;
        };
        let Ok(m) = self.values.lock() else {
            return kResultFalse;
        };
        match m.get(&k) {
            Some(Attr::Bin(b)) if !data.is_null() && !size_in_bytes.is_null() => {
                // SAFETY: non-null out-pointers; the Vec outlives this call
                // as long as the attribute isn't replaced.
                unsafe {
                    *data = b.as_ptr() as *const c_void;
                    *size_in_bytes = b.len() as uint32;
                }
                kResultOk
            }
            _ => kResultFalse,
        }
    }
}

/// A message between a plugin's processor and controller.
pub(crate) struct Message {
    id: Mutex<std::ffi::CString>,
    attributes: ComWrapper<AttributeList>,
}

impl Class for Message {
    type Interfaces = (IMessage,);
}

#[allow(unsafe_code)]
impl IMessageTrait for Message {
    unsafe fn getMessageID(&self) -> FIDString {
        // The CString's buffer lives as long as the message.
        self.id.lock().map_or(std::ptr::null(), |id| id.as_ptr())
    }
    unsafe fn setMessageID(&self, id: FIDString) {
        if id.is_null() {
            return;
        }
        // SAFETY: NUL-terminated id from the plugin.
        let s = unsafe { CStr::from_ptr(id) }.to_owned();
        if let Ok(mut g) = self.id.lock() {
            *g = s;
        }
    }
    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        // Borrowed pointer: VST3 doesn't add a reference here.
        self.attributes
            .as_com_ref::<IAttributeList>()
            .map_or(std::ptr::null_mut(), |r| r.as_ptr())
    }
}

/// Who we are, and a factory for messages and attribute lists (plugins
/// built with JUCE need these to connect their two halves).
pub(crate) struct HostApplication;

impl Class for HostApplication {
    type Interfaces = (IHostApplication,);
}

#[allow(unsafe_code)]
impl IHostApplicationTrait for HostApplication {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        if name.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: non-null String128 out-pointer.
        write_wide("Nunc Pro Tune", unsafe { &mut *name });
        kResultOk
    }
    unsafe fn createInstance(
        &self,
        cid: *mut TUID,
        iid: *mut TUID,
        obj: *mut *mut c_void,
    ) -> tresult {
        if cid.is_null() || iid.is_null() || obj.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: non-null 16-byte ids.
        let cid = guid_of(unsafe { &*cid });
        let new: Option<ComPtr<FUnknown>> = if cid == IMessage::IID {
            ComWrapper::new(Message {
                id: Mutex::new(std::ffi::CString::default()),
                attributes: ComWrapper::new(AttributeList {
                    values: Mutex::new(HashMap::new()),
                }),
            })
            .to_com_ptr()
        } else if cid == IAttributeList::IID {
            ComWrapper::new(AttributeList {
                values: Mutex::new(HashMap::new()),
            })
            .to_com_ptr()
        } else {
            None
        };
        let Some(new) = new else {
            // SAFETY: non-null out-pointer.
            unsafe { *obj = std::ptr::null_mut() };
            return kNoInterface;
        };
        let p = new.as_ptr();
        // SAFETY: p is a live FUnknown; queryInterface adds the reference
        // the caller owns.
        unsafe { ((*(*p).vtbl).queryInterface)(p, iid, obj) }
    }
}

/// A parameter the user moved in the plugin's own window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Edit {
    /// A drag starts (one undo step until `End`).
    Begin(u32),
    /// The parameter's new normalized value (0–1).
    Perform(u32, f64),
    End(u32),
    /// The plugin changed things wholesale (e.g. picked a preset): reread
    /// parameters and state.
    Restart,
}

pub(crate) type EditSink = Box<dyn Fn(Edit) + Send + Sync>;

/// Receives edits from the plugin's window and passes them on.
pub(crate) struct ComponentHandler {
    pub sink: Mutex<Option<EditSink>>,
}

impl Class for ComponentHandler {
    type Interfaces = (IComponentHandler,);
}

impl ComponentHandler {
    fn send(&self, e: Edit) -> tresult {
        if let Ok(g) = self.sink.lock()
            && let Some(f) = g.as_ref()
        {
            f(e);
        }
        kResultOk
    }
}

#[allow(unsafe_code)]
impl IComponentHandlerTrait for ComponentHandler {
    unsafe fn beginEdit(&self, id: ParamID) -> tresult {
        self.send(Edit::Begin(id))
    }
    unsafe fn performEdit(&self, id: ParamID, value: ParamValue) -> tresult {
        self.send(Edit::Perform(id, value))
    }
    unsafe fn endEdit(&self, id: ParamID) -> tresult {
        self.send(Edit::End(id))
    }
    unsafe fn restartComponent(&self, _flags: int32) -> tresult {
        self.send(Edit::Restart)
    }
}

/// Most events one block can carry; extra notes in the same block are
/// dropped rather than allocating.
pub(crate) const MAX_EVENTS: usize = 512;

/// The note events for one `process` call. Only the audio thread touches
/// it: we fill it, then the plugin reads it during `process`.
pub(crate) struct EventList {
    events: UnsafeCell<(Vec<Event>, usize)>,
}

// SAFETY: used by one thread at a time (the audio thread, and the plugin
// synchronously inside our process call).
#[allow(unsafe_code)]
unsafe impl Sync for EventList {}

impl Class for EventList {
    type Interfaces = (IEventList,);
}

#[allow(unsafe_code)]
impl EventList {
    pub fn new() -> ComWrapper<EventList> {
        // SAFETY: Event is plain data; all-zero is a valid (note-on) event.
        let blank: Event = unsafe { std::mem::zeroed() };
        ComWrapper::new(EventList {
            events: UnsafeCell::new((vec![blank; MAX_EVENTS], 0)),
        })
    }
    // RT-SAFE
    pub fn clear(&self) {
        // SAFETY: see the Sync note; no other reference is live.
        unsafe { (*self.events.get()).1 = 0 };
    }
    // RT-SAFE
    pub fn push(&self, e: Event) {
        // SAFETY: see the Sync note.
        let (v, n) = unsafe { &mut *self.events.get() };
        if let Some(slot) = v.get_mut(*n) {
            *slot = e;
            *n += 1;
        }
    }
    pub fn len(&self) -> usize {
        // SAFETY: see the Sync note.
        unsafe { (*self.events.get()).1 }
    }
}

#[allow(unsafe_code)]
impl IEventListTrait for EventList {
    unsafe fn getEventCount(&self) -> int32 {
        self.len() as int32
    }
    unsafe fn getEvent(&self, index: int32, e: *mut Event) -> tresult {
        // SAFETY: see the Sync note.
        let (v, n) = unsafe { &*self.events.get() };
        match (index >= 0 && (index as usize) < *n, e.is_null()) {
            (true, false) => {
                // SAFETY: non-null out-pointer.
                unsafe { *e = v[index as usize] };
                kResultOk
            }
            _ => kInvalidArgument,
        }
    }
    unsafe fn addEvent(&self, e: *mut Event) -> tresult {
        if e.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: non-null event from the plugin.
        self.push(unsafe { *e });
        kResultOk
    }
}

/// Points one parameter queue can hold per block.
const MAX_POINTS: usize = 16;
/// Parameters that can change in one block.
pub(crate) const MAX_PARAM_QUEUES: usize = 64;

/// A parameter id, its (sample offset, value) points, and how many are used.
type QueueData = (ParamID, [(i32, f64); MAX_POINTS], usize);

pub(crate) struct ParamQueue {
    data: UnsafeCell<QueueData>,
}

// SAFETY: as EventList.
#[allow(unsafe_code)]
unsafe impl Sync for ParamQueue {}

impl Class for ParamQueue {
    type Interfaces = (IParamValueQueue,);
}

#[allow(unsafe_code)]
impl IParamValueQueueTrait for ParamQueue {
    unsafe fn getParameterId(&self) -> ParamID {
        // SAFETY: see the Sync note.
        unsafe { (*self.data.get()).0 }
    }
    unsafe fn getPointCount(&self) -> int32 {
        // SAFETY: see the Sync note.
        unsafe { (*self.data.get()).2 as int32 }
    }
    unsafe fn getPoint(&self, index: int32, offset: *mut int32, value: *mut ParamValue) -> tresult {
        // SAFETY: see the Sync note.
        let (_, points, n) = unsafe { &*self.data.get() };
        if index < 0 || index as usize >= *n || offset.is_null() || value.is_null() {
            return kInvalidArgument;
        }
        let (o, v) = points[index as usize];
        // SAFETY: non-null out-pointers.
        unsafe {
            *offset = o;
            *value = v;
        }
        kResultOk
    }
    unsafe fn addPoint(&self, offset: int32, value: ParamValue, index: *mut int32) -> tresult {
        // SAFETY: see the Sync note.
        let (_, points, n) = unsafe { &mut *self.data.get() };
        if *n >= MAX_POINTS {
            return kResultFalse;
        }
        points[*n] = (offset, value);
        if !index.is_null() {
            // SAFETY: non-null out-pointer.
            unsafe { *index = *n as int32 };
        }
        *n += 1;
        kResultOk
    }
}

/// Parameter changes for one `process` call, preallocated.
pub(crate) struct ParameterChanges {
    queues: Vec<ComWrapper<ParamQueue>>,
    used: UnsafeCell<usize>,
}

// SAFETY: as EventList.
#[allow(unsafe_code)]
unsafe impl Sync for ParameterChanges {}

impl Class for ParameterChanges {
    type Interfaces = (IParameterChanges,);
}

#[allow(unsafe_code)]
impl ParameterChanges {
    pub fn new() -> ComWrapper<ParameterChanges> {
        let queues = (0..MAX_PARAM_QUEUES)
            .map(|_| {
                ComWrapper::new(ParamQueue {
                    data: UnsafeCell::new((0, [(0, 0.0); MAX_POINTS], 0)),
                })
            })
            .collect();
        ComWrapper::new(ParameterChanges {
            queues,
            used: UnsafeCell::new(0),
        })
    }
    // RT-SAFE
    pub fn clear(&self) {
        // SAFETY: see the Sync note.
        unsafe { *self.used.get() = 0 };
    }
    pub fn count(&self) -> usize {
        // SAFETY: see the Sync note.
        unsafe { *self.used.get() }
    }
    /// The queue for `id`, created if there's room.
    // RT-SAFE
    fn queue_for(&self, id: ParamID) -> Option<usize> {
        let used = self.count();
        for i in 0..used {
            // SAFETY: see the Sync note.
            if unsafe { (*self.queues[i].data.get()).0 } == id {
                return Some(i);
            }
        }
        if used >= self.queues.len() {
            return None;
        }
        // SAFETY: see the Sync note.
        unsafe {
            let d = &mut *self.queues[used].data.get();
            d.0 = id;
            d.2 = 0;
            *self.used.get() = used + 1;
        }
        Some(used)
    }
    /// Sets a parameter at the start of the block (later sets in the same
    /// block replace the value).
    // RT-SAFE
    pub fn set(&self, id: ParamID, value: f64) {
        if let Some(i) = self.queue_for(id) {
            // SAFETY: see the Sync note.
            let d = unsafe { &mut *self.queues[i].data.get() };
            d.1[0] = (0, value);
            d.2 = 1;
        }
    }
}

#[allow(unsafe_code)]
impl IParameterChangesTrait for ParameterChanges {
    unsafe fn getParameterCount(&self) -> int32 {
        self.count() as int32
    }
    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        if index < 0 || index as usize >= self.count() {
            return std::ptr::null_mut();
        }
        self.queues[index as usize]
            .as_com_ref::<IParamValueQueue>()
            .map_or(std::ptr::null_mut(), |r| r.as_ptr())
    }
    unsafe fn addParameterData(
        &self,
        id: *const ParamID,
        index: *mut int32,
    ) -> *mut IParamValueQueue {
        if id.is_null() {
            return std::ptr::null_mut();
        }
        // SAFETY: non-null id from the plugin.
        let Some(i) = self.queue_for(unsafe { *id }) else {
            return std::ptr::null_mut();
        };
        if !index.is_null() {
            // SAFETY: non-null out-pointer.
            unsafe { *index = i as int32 };
        }
        self.queues[i]
            .as_com_ref::<IParamValueQueue>()
            .map_or(std::ptr::null_mut(), |r| r.as_ptr())
    }
}
