//! Minimal safe wrapper over the JavaScriptCore C API
//! (`<JavaScriptCore/JavaScript.h>`). Only what the spike needs: evaluate
//! scripts, host functions, and a timer loop. JSC drains its microtask
//! queue itself when the outermost API call returns.

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr;
use std::time::{Duration, Instant};

pub const VERSION: &str = "webkitgtk javascriptcoregtk-4.1 (system)";

#[allow(non_camel_case_types)]
type JSContextRef = *const c_void;
type JSGlobalContextRef = *mut c_void;
type JSStringRef = *mut c_void;
type JSValueRef = *const c_void;
type JSObjectRef = *mut c_void;
type Callback = unsafe extern "C" fn(
    JSContextRef,
    JSObjectRef,
    JSObjectRef,
    usize,
    *const JSValueRef,
    *mut JSValueRef,
) -> JSValueRef;

unsafe extern "C" {
    fn JSGlobalContextCreate(global_class: *mut c_void) -> JSGlobalContextRef;
    fn JSGlobalContextRelease(ctx: JSGlobalContextRef);
    fn JSContextGetGlobalObject(ctx: JSContextRef) -> JSObjectRef;
    fn JSEvaluateScript(
        ctx: JSContextRef,
        script: JSStringRef,
        this: JSObjectRef,
        source_url: JSStringRef,
        line: i32,
        exception: *mut JSValueRef,
    ) -> JSValueRef;
    fn JSStringCreateWithUTF8CString(string: *const c_char) -> JSStringRef;
    fn JSStringRelease(string: JSStringRef);
    fn JSStringGetMaximumUTF8CStringSize(string: JSStringRef) -> usize;
    fn JSStringGetUTF8CString(string: JSStringRef, buffer: *mut c_char, size: usize) -> usize;
    fn JSValueToStringCopy(ctx: JSContextRef, value: JSValueRef, exception: *mut JSValueRef) -> JSStringRef;
    fn JSValueToNumber(ctx: JSContextRef, value: JSValueRef, exception: *mut JSValueRef) -> f64;
    fn JSValueToObject(ctx: JSContextRef, value: JSValueRef, exception: *mut JSValueRef) -> JSObjectRef;
    fn JSValueIsNull(ctx: JSContextRef, value: JSValueRef) -> bool;
    fn JSValueIsUndefined(ctx: JSContextRef, value: JSValueRef) -> bool;
    fn JSValueMakeUndefined(ctx: JSContextRef) -> JSValueRef;
    fn JSValueMakeNull(ctx: JSContextRef) -> JSValueRef;
    fn JSValueMakeBoolean(ctx: JSContextRef, value: bool) -> JSValueRef;
    fn JSValueMakeNumber(ctx: JSContextRef, value: f64) -> JSValueRef;
    fn JSValueMakeString(ctx: JSContextRef, string: JSStringRef) -> JSValueRef;
    fn JSValueProtect(ctx: JSContextRef, value: JSValueRef);
    fn JSValueUnprotect(ctx: JSContextRef, value: JSValueRef);
    fn JSObjectMakeFunctionWithCallback(ctx: JSContextRef, name: JSStringRef, callback: Callback) -> JSObjectRef;
    fn JSObjectSetProperty(
        ctx: JSContextRef,
        object: JSObjectRef,
        name: JSStringRef,
        value: JSValueRef,
        attributes: u32,
        exception: *mut JSValueRef,
    );
    fn JSObjectCallAsFunction(
        ctx: JSContextRef,
        function: JSObjectRef,
        this: JSObjectRef,
        argc: usize,
        argv: *const JSValueRef,
        exception: *mut JSValueRef,
    ) -> JSValueRef;
}

/// Like the V8/QuickJS hosts in Phase 0: console, timers, __report, __host.
pub const PRELUDE: &str = r#"
globalThis.queueMicrotask ??= (cb) => { Promise.resolve().then(cb); };
globalThis.console = {
  log: (...a) => __print(a.map(String).join(" ")), info: (...a) => __print(a.map(String).join(" ")),
  warn: (...a) => __print(a.map(String).join(" ")), error: (...a) => __print(a.map(String).join(" ")),
  debug() {}, trace() {}, time() {}, timeEnd() {}, group() {}, groupEnd() {}, dir() {}, table() {},
};
globalThis.setTimeout = (cb, ms = 0, ...args) => __setTimeout(() => cb(...args), Number(ms) || 0);
globalThis.clearTimeout = (id) => __clearTimeout(Number(id));
globalThis.setInterval = () => { throw new Error("setInterval not provided in the JSC host"); };
globalThis.__host = { readText: __readText, isFile: __isFile, isDir: __isDir };
"#;

struct Timer {
    id: u32,
    due: Instant,
    callback: JSObjectRef,
}

thread_local! {
    static REPORTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static TIMERS: RefCell<Vec<Timer>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: RefCell<u32> = const { RefCell::new(0) };
}

pub fn report(value: String) {
    REPORTS.with(|r| r.borrow_mut().push(value));
}

pub fn take_reports() -> Vec<String> {
    REPORTS.with(|r| std::mem::take(&mut *r.borrow_mut()))
}

fn js_string(text: &str) -> JSStringRef {
    let c = CString::new(text.replace('\0', "")).expect("no interior NUL");
    // SAFETY: c is a valid NUL-terminated string for the duration of the call.
    unsafe { JSStringCreateWithUTF8CString(c.as_ptr()) }
}

fn rust_string(ctx: JSContextRef, value: JSValueRef) -> String {
    // SAFETY: ctx and value come from JSC callbacks/evaluation on this thread.
    unsafe {
        let s = JSValueToStringCopy(ctx, value, ptr::null_mut());
        if s.is_null() {
            return String::new();
        }
        let size = JSStringGetMaximumUTF8CStringSize(s);
        let mut buffer = vec![0 as c_char; size.max(1)];
        JSStringGetUTF8CString(s, buffer.as_mut_ptr(), size);
        JSStringRelease(s);
        CStr::from_ptr(buffer.as_ptr()).to_string_lossy().into_owned()
    }
}

fn arg(args: *const JSValueRef, argc: usize, i: usize) -> Option<JSValueRef> {
    // SAFETY: JSC passes `argc` valid values in `args`.
    (i < argc).then(|| unsafe { *args.add(i) })
}

unsafe extern "C" fn host_report(
    ctx: JSContextRef, _f: JSObjectRef, _t: JSObjectRef, argc: usize, args: *const JSValueRef, _e: *mut JSValueRef,
) -> JSValueRef {
    if let Some(value) = arg(args, argc, 0) {
        report(rust_string(ctx, value));
    }
    // SAFETY: valid context.
    unsafe { JSValueMakeUndefined(ctx) }
}

unsafe extern "C" fn host_print(
    ctx: JSContextRef, _f: JSObjectRef, _t: JSObjectRef, argc: usize, args: *const JSValueRef, _e: *mut JSValueRef,
) -> JSValueRef {
    if let Some(value) = arg(args, argc, 0) {
        eprintln!("{}", rust_string(ctx, value));
    }
    // SAFETY: valid context.
    unsafe { JSValueMakeUndefined(ctx) }
}

unsafe extern "C" fn host_set_timeout(
    ctx: JSContextRef, _f: JSObjectRef, _t: JSObjectRef, argc: usize, args: *const JSValueRef, _e: *mut JSValueRef,
) -> JSValueRef {
    // SAFETY: values come from JSC on this thread; the callback is protected
    // from GC until it runs or is cleared.
    unsafe {
        let Some(callback) = arg(args, argc, 0) else {
            return JSValueMakeUndefined(ctx);
        };
        let ms = arg(args, argc, 1).map_or(0.0, |v| JSValueToNumber(ctx, v, ptr::null_mut()));
        let callback = JSValueToObject(ctx, callback, ptr::null_mut());
        JSValueProtect(ctx, callback);
        let id = NEXT_ID.with(|n| {
            *n.borrow_mut() += 1;
            *n.borrow()
        });
        let delay = Duration::from_secs_f64(if ms.is_finite() { ms.max(0.0) / 1000.0 } else { 0.0 });
        TIMERS.with(|t| t.borrow_mut().push(Timer { id, due: Instant::now() + delay, callback }));
        JSValueMakeNumber(ctx, f64::from(id))
    }
}

unsafe extern "C" fn host_clear_timeout(
    ctx: JSContextRef, _f: JSObjectRef, _t: JSObjectRef, argc: usize, args: *const JSValueRef, _e: *mut JSValueRef,
) -> JSValueRef {
    // SAFETY: as above.
    unsafe {
        if let Some(value) = arg(args, argc, 0) {
            let id = JSValueToNumber(ctx, value, ptr::null_mut()) as u32;
            let removed = TIMERS.with(|t| {
                let mut timers = t.borrow_mut();
                timers.iter().position(|x| x.id == id).map(|i| timers.remove(i))
            });
            if let Some(timer) = removed {
                JSValueUnprotect(ctx, timer.callback);
            }
        }
        JSValueMakeUndefined(ctx)
    }
}

unsafe extern "C" fn host_read_text(
    ctx: JSContextRef, _f: JSObjectRef, _t: JSObjectRef, argc: usize, args: *const JSValueRef, _e: *mut JSValueRef,
) -> JSValueRef {
    let path = arg(args, argc, 0).map(|v| rust_string(ctx, v)).unwrap_or_default();
    // SAFETY: valid context; the string is released after conversion.
    unsafe {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let s = js_string(&text);
                let value = JSValueMakeString(ctx, s);
                JSStringRelease(s);
                value
            }
            Err(_) => JSValueMakeNull(ctx),
        }
    }
}

unsafe extern "C" fn host_is_file(
    ctx: JSContextRef, _f: JSObjectRef, _t: JSObjectRef, argc: usize, args: *const JSValueRef, _e: *mut JSValueRef,
) -> JSValueRef {
    let path = arg(args, argc, 0).map(|v| rust_string(ctx, v)).unwrap_or_default();
    // SAFETY: valid context.
    unsafe { JSValueMakeBoolean(ctx, std::path::Path::new(&path).is_file()) }
}

unsafe extern "C" fn host_is_dir(
    ctx: JSContextRef, _f: JSObjectRef, _t: JSObjectRef, argc: usize, args: *const JSValueRef, _e: *mut JSValueRef,
) -> JSValueRef {
    let path = arg(args, argc, 0).map(|v| rust_string(ctx, v)).unwrap_or_default();
    // SAFETY: valid context.
    unsafe { JSValueMakeBoolean(ctx, std::path::Path::new(&path).is_dir()) }
}

pub struct Engine {
    ctx: JSGlobalContextRef,
}

impl Engine {
    pub fn new() -> Self {
        // SAFETY: a null class creates a default global object.
        let ctx = unsafe { JSGlobalContextCreate(ptr::null_mut()) };
        let engine = Self { ctx };
        let host: [(&str, Callback); 7] = [
            ("__report", host_report),
            ("__print", host_print),
            ("__setTimeout", host_set_timeout),
            ("__clearTimeout", host_clear_timeout),
            ("__readText", host_read_text),
            ("__isFile", host_is_file),
            ("__isDir", host_is_dir),
        ];
        for (name, callback) in host {
            let js_name = js_string(name);
            // SAFETY: ctx is live; js_name is released after use.
            unsafe {
                let function = JSObjectMakeFunctionWithCallback(ctx, js_name, callback);
                JSObjectSetProperty(ctx, JSContextGetGlobalObject(ctx), js_name, function, 0, ptr::null_mut());
                JSStringRelease(js_name);
            }
        }
        engine
    }

    /// Evaluates a classic script; returns its completion value as a string.
    pub fn eval(&mut self, name: &str, source: &str) -> Result<String, String> {
        let script = js_string(source);
        let url = js_string(name);
        let mut exception: JSValueRef = ptr::null();
        // SAFETY: ctx is live; strings are released after the call.
        let value = unsafe {
            let value = JSEvaluateScript(self.ctx, script, ptr::null_mut(), url, 1, &mut exception);
            JSStringRelease(script);
            JSStringRelease(url);
            value
        };
        if !exception.is_null() {
            return Err(format!("{name}: {}", self.describe(exception)));
        }
        Ok(if value.is_null() { String::new() } else { rust_string(self.ctx, value) })
    }

    fn describe(&self, exception: JSValueRef) -> String {
        let stack = self
            .eval_value_property(exception, "stack")
            .unwrap_or_default();
        format!("{} {}", rust_string(self.ctx, exception), stack)
    }

    fn eval_value_property(&self, value: JSValueRef, property: &str) -> Option<String> {
        // SAFETY: ctx is live.
        unsafe {
            if JSValueIsNull(self.ctx, value) || JSValueIsUndefined(self.ctx, value) {
                return None;
            }
            let object = JSValueToObject(self.ctx, value, ptr::null_mut());
            if object.is_null() {
                return None;
            }
            let getter = js_string(&format!("(function (o) {{ return String(o[{property:?}]); }})"));
            let mut exception: JSValueRef = ptr::null();
            let f = JSEvaluateScript(self.ctx, getter, ptr::null_mut(), ptr::null_mut(), 1, &mut exception);
            JSStringRelease(getter);
            let f = JSValueToObject(self.ctx, f, ptr::null_mut());
            let args = [value];
            let result = JSObjectCallAsFunction(self.ctx, f, ptr::null_mut(), 1, args.as_ptr(), &mut exception);
            (!result.is_null()).then(|| rust_string(self.ctx, result))
        }
    }

    /// Runs timers in due order until none are left. Microtasks queued by a
    /// callback are drained by JSC when the call returns.
    pub fn run_event_loop(&mut self) -> Result<(), String> {
        loop {
            let next = TIMERS.with(|t| {
                let mut timers = t.borrow_mut();
                let index = (0..timers.len()).min_by_key(|&i| (timers[i].due, timers[i].id))?;
                Some(timers.remove(index))
            });
            let Some(timer) = next else { return Ok(()) };
            let now = Instant::now();
            if timer.due > now {
                std::thread::sleep(timer.due - now);
            }
            let mut exception: JSValueRef = ptr::null();
            // SAFETY: the callback was protected when scheduled.
            unsafe {
                JSObjectCallAsFunction(self.ctx, timer.callback, ptr::null_mut(), 0, ptr::null(), &mut exception);
                JSValueUnprotect(self.ctx, timer.callback);
            }
            if !exception.is_null() {
                return Err(format!("timer callback threw: {}", self.describe(exception)));
            }
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: releases the context created in `new`.
        unsafe { JSGlobalContextRelease(self.ctx) }
    }
}
