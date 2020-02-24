#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_void};

pub enum dispatch_queue_s {}
pub enum dispatch_queue_attr_s {}

#[repr(C)]
pub union dispatch_object_s {
    _dq: *mut dispatch_queue_s,
}

impl From<*mut dispatch_queue_s> for dispatch_object_s {
    fn from(dq: *mut dispatch_queue_s) -> Self {
        Self { _dq: dq }
    }
}

pub type dispatch_function_t = Option<extern "C" fn(*mut c_void)>;
pub type dispatch_object_t = dispatch_object_s;
pub type dispatch_queue_attr_t = *mut dispatch_queue_attr_s;
pub type dispatch_queue_t = *mut dispatch_queue_s;

pub const DISPATCH_QUEUE_SERIAL: dispatch_queue_attr_t = 0 as dispatch_queue_attr_t;

#[link(name = "System", kind = "dylib")] // Link to a dynamic library in Dispatch framework.
extern "C" {
    pub fn dispatch_queue_create(
        label: *const c_char,
        attr: dispatch_queue_attr_t,
    ) -> dispatch_queue_t;

    pub fn dispatch_release(object: dispatch_object_t);

    pub fn dispatch_get_context(object: dispatch_object_t) -> *mut c_void;

    pub fn dispatch_set_context(object: dispatch_object_t, context: *mut c_void);

    pub fn dispatch_set_finalizer_f(object: dispatch_object_t, finalizer: dispatch_function_t);

    pub fn dispatch_async_f(
        queue: dispatch_queue_t,
        context: *mut c_void,
        work: dispatch_function_t,
    );

    pub fn dispatch_sync_f(
        queue: dispatch_queue_t,
        context: *mut c_void,
        work: dispatch_function_t,
    );
}
