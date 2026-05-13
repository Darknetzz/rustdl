//! Replace winit's file-only [`IDropTarget`] so browser drag-and-drop works on Windows:
//! `UniformResourceLocatorW`, `text/uri-list`, Firefox `text/x-moz-url`, `CF_UNICODETEXT`, and
//! [`CF_HDROP`] (including `.url` shortcuts and playlist/list files resolved in-app).

#![allow(unsafe_code)]

use std::ffi::{c_void, OsString};
use std::mem;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use windows_sys::core::{GUID, HRESULT, IUnknown};
use windows_sys::Win32::Foundation::{E_NOINTERFACE, HWND, POINTL, S_OK};
use windows_sys::Win32::System::Com::{
    IDataObject, DVASPECT_CONTENT, FORMATETC, STGMEDIUM, TYMED_HGLOBAL,
};
use windows_sys::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows_sys::Win32::System::Ole::{
    ReleaseStgMedium, CF_HDROP, CF_UNICODETEXT, DROPEFFECT_COPY, DROPEFFECT_NONE, RegisterDragDrop,
    RevokeDragDrop,
};
use windows_sys::Win32::UI::Shell::{DragFinish, DragQueryFileW, HDROP};

use crate::app_parsing;

/// `IUnknown` and `IDropTarget` IIDs (oleidl.h).
const IID_IUNKNOWN: GUID = GUID {
    data1: 0x0000_0000,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};
const IID_IDROPTARGET: GUID = GUID {
    data1: 0x0000_0122,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

#[repr(C)]
struct IUnknownVtbl {
    query_interface: unsafe extern "system" fn(
        this: IUnknown,
        riid: *const GUID,
        out: *mut *mut c_void,
    ) -> HRESULT,
    add_ref: unsafe extern "system" fn(this: IUnknown) -> u32,
    release: unsafe extern "system" fn(this: IUnknown) -> u32,
}

#[repr(C)]
struct IDropTargetVtbl {
    parent: IUnknownVtbl,
    drag_enter: unsafe extern "system" fn(
        this: *mut IDropTargetRaw,
        p_data_obj: IDataObject,
        grf_key_state: u32,
        pt: *const POINTL,
        pdw_effect: *mut u32,
    ) -> HRESULT,
    drag_over: unsafe extern "system" fn(
        this: *mut IDropTargetRaw,
        grf_key_state: u32,
        pt: *const POINTL,
        pdw_effect: *mut u32,
    ) -> HRESULT,
    drag_leave: unsafe extern "system" fn(this: *mut IDropTargetRaw) -> HRESULT,
    drop: unsafe extern "system" fn(
        this: *mut IDropTargetRaw,
        p_data_obj: IDataObject,
        grf_key_state: u32,
        pt: *const POINTL,
        pdw_effect: *mut u32,
    ) -> HRESULT,
}

#[repr(C)]
struct IDropTargetRaw {
    lp_vtbl: *const IDropTargetVtbl,
    refcount: AtomicUsize,
    queue: Arc<Mutex<Vec<String>>>,
    /// Last drag offered something we accept (for DragLeave cancel semantics).
    hover_valid: bool,
    cursor_effect: u32,
}

unsafe fn guid_eq(a: *const GUID, b: &GUID) -> bool {
    unsafe {
        let a = &*a;
        a.data1 == b.data1
            && a.data2 == b.data2
            && a.data3 == b.data3
            && a.data4 == b.data4
    }
}

/// [`IDataObject::GetData`] is vtable slot 3 after `IUnknown` (same layout as the Windows SDK).
unsafe fn idata_get_data(obj: IDataObject, fmt: &FORMATETC, med: *mut STGMEDIUM) -> HRESULT {
    type GetDataFn =
        unsafe extern "system" fn(this: IDataObject, pformatetc_in: *const FORMATETC, pmedium: *mut STGMEDIUM)
            -> HRESULT;
    let vtbl = *(obj as *const *const usize);
    let get_data = *vtbl.add(3);
    let f: GetDataFn = mem::transmute(get_data);
    f(obj, fmt, med)
}

unsafe fn register_format_w(name: &[u16]) -> u16 {
    debug_assert_eq!(name.last().copied(), Some(0));
    let id = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
    id as u16
}

unsafe fn read_hglobal_bytes(medium: &STGMEDIUM) -> Option<Vec<u8>> {
    if medium.tymed != TYMED_HGLOBAL as u32 {
        return None;
    }
    let h = unsafe { medium.u.hGlobal };
    if h.is_null() {
        return None;
    }
    let sz = unsafe { GlobalSize(h) };
    if sz == 0 {
        return None;
    }
    let p = unsafe { GlobalLock(h) };
    if p.is_null() {
        return None;
    }
    let slice = unsafe { std::slice::from_raw_parts(p.cast::<u8>(), sz) };
    let v = slice.to_vec();
    unsafe { GlobalUnlock(h) };
    Some(v)
}

unsafe fn try_get_format_bytes(data_obj: IDataObject, cf: u16) -> Option<Vec<u8>> {
    let fmt = FORMATETC {
        cfFormat: cf,
        ptd: ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT,
        lindex: -1,
        tymed: TYMED_HGLOBAL as u32,
    };
    let mut medium: STGMEDIUM = mem::zeroed();
    let hr = unsafe { idata_get_data(data_obj, &fmt, &mut medium) };
    if hr < 0 {
        return None;
    }
    let out = read_hglobal_bytes(&medium);
    unsafe { ReleaseStgMedium(&mut medium) };
    out
}

fn utf16_nul_prefix_to_string(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 2 {
        return None;
    }
    let mut words = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        words.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }
    let end = words.iter().position(|&x| x == 0).unwrap_or(words.len());
    let s = String::from_utf16_lossy(&words[..end]);
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_owned())
    }
}

/// Full UTF-16LE buffer (trim only trailing NUL code units). Use for `CF_UNICODETEXT` drag payloads.
fn utf16le_buffer_to_string_lossy(bytes: &[u8]) -> String {
    let mut words = Vec::with_capacity(bytes.len() / 2 + 1);
    for chunk in bytes.chunks_exact(2) {
        words.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }
    while words.last() == Some(&0) {
        words.pop();
    }
    String::from_utf16_lossy(&words)
}

fn urls_from_text_uri_list(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let text = app_parsing::strip_utf8_bom(text.trim());
    app_parsing::parse_urls_from_text_blob(text)
        .into_iter()
        .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
        .collect()
}

/// Firefox `text/x-moz-url`: UTF-16LE `URL\nTitle` or UTF-8 `URL\nTitle`.
fn urls_from_moz_url(bytes: &[u8]) -> Vec<String> {
    let as_utf16 = || -> Option<String> {
        if bytes.len() < 4 || bytes.len() % 2 != 0 {
            return None;
        }
        let s = utf16le_buffer_to_string_lossy(bytes);
        let first = s.lines().next()?.trim();
        if first.starts_with("http://") || first.starts_with("https://") {
            Some(first.to_owned())
        } else {
            None
        }
    };
    if let Some(u) = as_utf16() {
        return vec![u];
    }
    let s = String::from_utf8_lossy(bytes);
    let s = app_parsing::strip_utf8_bom(s.trim());
    let Some(first) = s.lines().next().map(str::trim) else {
        return Vec::new();
    };
    if first.starts_with("http://") || first.starts_with("https://") {
        vec![first.to_owned()]
    } else {
        Vec::new()
    }
}

fn urls_from_unicode_text(bytes: &[u8]) -> Vec<String> {
    let s = utf16le_buffer_to_string_lossy(bytes);
    s.lines()
        .map(str::trim)
        .filter(|l| l.starts_with("http://") || l.starts_with("https://"))
        .map(str::to_owned)
        .collect()
}

fn collect_url_formats(data_obj: IDataObject) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let loc_w = register_format_w(&utf16_nul("UniformResourceLocatorW"));
        let uri_list = register_format_w(&utf16_nul("text/uri-list"));
        let moz = register_format_w(&utf16_nul("text/x-moz-url"));
        if let Some(b) = try_get_format_bytes(data_obj, loc_w) {
            if let Some(u) = utf16_nul_prefix_to_string(&b) {
                out.push(u);
            }
        }
        if let Some(b) = try_get_format_bytes(data_obj, uri_list) {
            out.extend(urls_from_text_uri_list(&b));
        }
        if let Some(b) = try_get_format_bytes(data_obj, moz) {
            out.extend(urls_from_moz_url(&b));
        }
        if out.is_empty() {
            if let Some(b) = try_get_format_bytes(data_obj, CF_UNICODETEXT) {
                out.extend(urls_from_unicode_text(&b));
            }
        }
    }
    app_parsing::dedupe_preserve_order_strings(out)
}

fn utf16_nul(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn hdrop_item_count(data_obj: IDataObject) -> u32 {
    let fmt = FORMATETC {
        cfFormat: CF_HDROP,
        ptd: ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT,
        lindex: -1,
        tymed: TYMED_HGLOBAL as u32,
    };
    let mut medium: STGMEDIUM = mem::zeroed();
    let hr = idata_get_data(data_obj, &fmt, &mut medium);
    if hr < 0 {
        return 0;
    }
    let hdrop = unsafe { medium.u.hGlobal as HDROP };
    let n = if hdrop == 0 {
        0
    } else {
        unsafe { DragQueryFileW(hdrop, 0xffff_ffff, ptr::null_mut(), 0) }
    };
    if hdrop != 0 {
        unsafe { DragFinish(hdrop) };
    }
    unsafe { ReleaseStgMedium(&mut medium) };
    n
}

unsafe fn consume_hdrop_paths<F: FnMut(PathBuf)>(data_obj: IDataObject, mut f: F) {
    let fmt = FORMATETC {
        cfFormat: CF_HDROP,
        ptd: ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT,
        lindex: -1,
        tymed: TYMED_HGLOBAL as u32,
    };
    let mut medium: STGMEDIUM = mem::zeroed();
    let hr = idata_get_data(data_obj, &fmt, &mut medium);
    if hr < 0 {
        return;
    }
    let hdrop = unsafe { medium.u.hGlobal as HDROP };
    if hdrop == 0 {
        unsafe { ReleaseStgMedium(&mut medium) };
        return;
    }
    let item_count = unsafe { DragQueryFileW(hdrop, 0xffff_ffff, ptr::null_mut(), 0) };
    for i in 0..item_count {
        let character_count = unsafe { DragQueryFileW(hdrop, i, ptr::null_mut(), 0) as usize };
        let str_len = character_count + 1;
        let mut path_buf = Vec::with_capacity(str_len);
        unsafe {
            DragQueryFileW(hdrop, i, path_buf.as_mut_ptr(), str_len as u32);
            path_buf.set_len(str_len);
        }
        let path = OsString::from_wide(&path_buf[..character_count]).into();
        f(path);
    }
    unsafe { DragFinish(hdrop) };
    unsafe { ReleaseStgMedium(&mut medium) };
}

fn harvest_drop_urls(data_obj: IDataObject, resolve_shortcuts: bool) -> Vec<String> {
    let mut out = collect_url_formats(data_obj);
    unsafe {
        consume_hdrop_paths(data_obj, |path| {
            if resolve_shortcuts {
                if let Some(urls) = app_parsing::urls_from_dropped_os_path(&path) {
                    out.extend(urls);
                }
            }
        });
    }
    app_parsing::dedupe_preserve_order_strings(out)
}

fn drop_accepts(data_obj: IDataObject) -> bool {
    !collect_url_formats(data_obj).is_empty() || unsafe { hdrop_item_count(data_obj) > 0 }
}

unsafe extern "system" fn dt_query_interface(
    this: IUnknown,
    riid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if this.is_null() || riid.is_null() || out.is_null() {
        return E_NOINTERFACE;
    }
    unsafe {
        *out = ptr::null_mut();
        if guid_eq(riid, &IID_IUNKNOWN) || guid_eq(riid, &IID_IDROPTARGET) {
            (*this.cast::<IDropTargetRaw>())
                .refcount
                .fetch_add(1, Ordering::Relaxed);
            *out = this.cast();
            S_OK
        } else {
            E_NOINTERFACE
        }
    }
}

unsafe extern "system" fn dt_add_ref(this: IUnknown) -> u32 {
    unsafe {
        (*this.cast::<IDropTargetRaw>())
            .refcount
            .fetch_add(1, Ordering::Relaxed) as u32
            + 1
    }
}

unsafe extern "system" fn dt_release(this: IUnknown) -> u32 {
    unsafe {
        let raw = this.cast::<IDropTargetRaw>();
        let count = (*raw).refcount.fetch_sub(1, Ordering::Release) - 1;
        if count == 0 {
            drop(Box::from_raw(raw));
        }
        count as u32
    }
}

unsafe extern "system" fn dt_drag_enter(
    this: *mut IDropTargetRaw,
    p_data_obj: IDataObject,
    _grf_key_state: u32,
    _pt: *const POINTL,
    pdw_effect: *mut u32,
) -> HRESULT {
    let raw = unsafe { &mut *this };
    raw.hover_valid = !p_data_obj.is_null() && drop_accepts(p_data_obj);
    raw.cursor_effect = if raw.hover_valid {
        DROPEFFECT_COPY
    } else {
        DROPEFFECT_NONE
    };
    unsafe {
        *pdw_effect = raw.cursor_effect;
    }
    S_OK
}

unsafe extern "system" fn dt_drag_over(
    this: *mut IDropTargetRaw,
    _grf_key_state: u32,
    _pt: *const POINTL,
    pdw_effect: *mut u32,
) -> HRESULT {
    let raw = unsafe { &*this };
    unsafe {
        *pdw_effect = raw.cursor_effect;
    }
    S_OK
}

unsafe extern "system" fn dt_drag_leave(this: *mut IDropTargetRaw) -> HRESULT {
    let raw = unsafe { &mut *this };
    if raw.hover_valid {
        raw.hover_valid = false;
    }
    raw.cursor_effect = DROPEFFECT_NONE;
    S_OK
}

unsafe extern "system" fn dt_drop(
    this: *mut IDropTargetRaw,
    p_data_obj: IDataObject,
    _grf_key_state: u32,
    _pt: *const POINTL,
    pdw_effect: *mut u32,
) -> HRESULT {
    let raw = unsafe { &mut *this };
    raw.hover_valid = false;
    raw.cursor_effect = DROPEFFECT_NONE;
    unsafe {
        *pdw_effect = DROPEFFECT_NONE;
    }
    if !p_data_obj.is_null() {
        let urls = harvest_drop_urls(p_data_obj, true);
        if !urls.is_empty() {
            if let Ok(mut g) = raw.queue.lock() {
                g.extend(urls);
            }
        }
    }
    S_OK
}

static DROP_TARGET_VTBL: IDropTargetVtbl = IDropTargetVtbl {
    parent: IUnknownVtbl {
        query_interface: dt_query_interface,
        add_ref: dt_add_ref,
        release: dt_release,
    },
    drag_enter: dt_drag_enter,
    drag_over: dt_drag_over,
    drag_leave: dt_drag_leave,
    drop: dt_drop,
};

/// Installs our drop target once (revokes winit's file-only handler first).
pub fn install_once(hwnd: HWND, queue: Arc<Mutex<Vec<String>>>) -> Result<(), &'static str> {
    if hwnd == 0 {
        return Err("null hwnd");
    }
    unsafe {
        let _ = RevokeDragDrop(hwnd);
        let boxed = Box::new(IDropTargetRaw {
            lp_vtbl: &DROP_TARGET_VTBL,
            refcount: AtomicUsize::new(1),
            queue,
            hover_valid: false,
            cursor_effect: DROPEFFECT_NONE,
        });
        let iface: *mut IDropTargetRaw = Box::into_raw(boxed);
        let hr = RegisterDragDrop(hwnd, iface.cast());
        if hr == S_OK {
            // Ole32 holds a reference; release our construction refcount.
            let _ = dt_release(iface.cast());
            Ok(())
        } else {
            let _ = Box::from_raw(iface);
            Err("RegisterDragDrop failed")
        }
    }
}
