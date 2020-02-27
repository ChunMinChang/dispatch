mod sys;

use std::ffi::CString;
use std::os::raw::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

// Queue: A wrapper around `dispatch_queue_t`.
// ------------------------------------------------------------------------------------------------
pub struct Queue(sys::dispatch_queue_t);

impl Queue {
    pub fn new(label: &'static str) -> Self {
        Self(create_dispatch_queue(label, sys::DISPATCH_QUEUE_SERIAL))
    }

    pub fn run_async<F>(&self, work: F)
    where
        F: Send + FnOnce(),
    {
        let should_cancel = self.get_context::<AtomicBool>();
        dispatch_async(self.0, || {
            if should_cancel.map_or(false, |v| v.load(Ordering::SeqCst)) {
                return;
            }
            work();
        });
    }

    pub fn run_sync<F>(&self, work: F)
    where
        F: Send + FnOnce(),
    {
        let should_cancel = self.get_context::<AtomicBool>();
        dispatch_sync(self.0, || {
            if should_cancel.map_or(false, |v| v.load(Ordering::SeqCst)) {
                return;
            }
            work();
        });
    }

    pub fn run_final<F>(&self, work: F)
    where
        F: Send + FnOnce(),
    {
        self.set_context(Box::new(AtomicBool::new(false)));
        let should_cancel = self.get_context::<AtomicBool>();
        dispatch_sync(self.0, || {
            work();
            should_cancel
                .expect("dispatch context should be allocated!")
                .store(true, Ordering::SeqCst);
        });
    }

    fn get_context<T>(&self) -> Option<&mut T> {
        get_dispatch_context(self.0)
    }

    fn set_context<T>(&self, context: Box<T>) {
        set_dispatch_context(self.0, context);
    }

    // This will release the inner `dispatch_queue_t` asynchronously.
    fn destroy(&mut self) {
        release_dispatch_queue(self.0);
    }
}

impl Drop for Queue {
    fn drop(&mut self) {
        self.destroy();
    }
}

// Low-level Grand Central Dispatch (GCD) APIs
// ------------------------------------------------------------------------------------------------
fn create_dispatch_queue(
    label: &'static str,
    queue_attr: sys::dispatch_queue_attr_t,
) -> sys::dispatch_queue_t {
    let label = CString::new(label).unwrap();
    let c_string = label.as_ptr();
    unsafe { sys::dispatch_queue_create(c_string, queue_attr) }
}

// This will release the `queue` asynchronously.
fn release_dispatch_queue(queue: sys::dispatch_queue_t) {
    unsafe {
        sys::dispatch_release(queue.into());
    }
}

// The type `T` must be same as the `T` used in `set_dispatch_context`
fn get_dispatch_context<'a, T>(queue: sys::dispatch_queue_t) -> Option<&'a mut T> {
    unsafe {
        let context = sys::dispatch_get_context(queue.into()) as *mut T;
        context.as_mut()
    }
}

fn set_dispatch_context<T>(queue: sys::dispatch_queue_t, context: Box<T>) {
    unsafe {
        // Leak the context from Box.
        sys::dispatch_set_context(queue.into(), Box::into_raw(context) as *mut c_void);

        extern "C" fn finalizer<T>(context: *mut c_void) {
            // Retake the leaked context into box and then drop it.
            let _ = unsafe { Box::from_raw(context as *mut T) };
        }

        // The `finalizer` is only run if the `context` in `queue` is set (by `sys::dispatch_set_context`).
        sys::dispatch_set_finalizer_f(queue.into(), Some(finalizer::<T>));
    }
}

// Send: Types that can be transferred across thread boundaries.
// FnOnce: One-time function.
fn dispatch_async<F>(queue: sys::dispatch_queue_t, work: F)
where
    F: Send + FnOnce(),
{
    let (closure, executor) = create_closure_and_executor(work);
    unsafe {
        sys::dispatch_async_f(queue, closure, executor);
    }
}

// Send: Types that can be transferred across thread boundaries.
// FnOnce: One-time function.
fn dispatch_sync<F>(queue: sys::dispatch_queue_t, work: F)
where
    F: Send + FnOnce(),
{
    let (closure, executor) = create_closure_and_executor(work);
    unsafe {
        sys::dispatch_sync_f(queue, closure, executor);
    }
}

// Return a raw pointer to a (unboxed) closure and an executor that
// will run the closure (after re-boxing the closure) when it's called.
fn create_closure_and_executor<F>(closure: F) -> (*mut c_void, sys::dispatch_function_t)
where
    F: FnOnce(),
{
    extern "C" fn closure_executer<F>(unboxed_closure: *mut c_void)
    where
        F: FnOnce(),
    {
        // Retake the leaked closure.
        let closure: Box<F> = unsafe { Box::from_raw(unboxed_closure as *mut F) };
        // Execute the closure.
        (*closure)();
        // closure is released after finishiing this function call.
    }

    // The rust closure is a struct. We need to convert it into a function
    // pointer so it can be invoked on another thread.
    let closure: Box<F> = Box::new(closure);
    let executor: sys::dispatch_function_t = Some(closure_executer::<F>);

    (
        Box::into_raw(closure) as *mut c_void, // Leak the closure.
        executor,
    )
}

#[test]
fn run_tasks_in_order() {
    let mut visited = Vec::<u32>::new();

    // Rust compilter doesn't allow a pointer to be passed across threads.
    // A hacky way to do that is to cast the pointer into a value, then
    // the value, which is actually an address, can be copied into threads.
    let ptr = &mut visited as *mut Vec<u32> as usize;

    fn visit(v: u32, visited_ptr: usize) {
        let visited = unsafe { &mut *(visited_ptr as *mut Vec<u32>) };
        visited.push(v);
    };

    let queue = Queue::new("Run tasks in order");

    queue.run_sync(move || visit(1, ptr));
    queue.run_sync(move || visit(2, ptr));
    queue.run_async(move || visit(3, ptr));
    queue.run_async(move || visit(4, ptr));
    queue.run_sync(move || visit(5, ptr));

    assert_eq!(visited, vec![1, 2, 3, 4, 5]);
}

#[test]
fn run_final_task() {
    let mut visited = Vec::<u32>::new();

    {
        // Rust compilter doesn't allow a pointer to be passed across threads.
        // A hacky way to do that is to cast the pointer into a value, then
        // the value, which is actually an address, can be copied into threads.
        let ptr = &mut visited as *mut Vec<u32> as usize;

        fn visit(v: u32, visited_ptr: usize) {
            let visited = unsafe { &mut *(visited_ptr as *mut Vec<u32>) };
            visited.push(v);
        };

        let queue = Queue::new("Task after run_final will be cancelled");

        queue.run_sync(move || visit(1, ptr));
        queue.run_async(move || visit(2, ptr));
        queue.run_final(move || visit(3, ptr));
        queue.run_async(move || visit(4, ptr));
        queue.run_sync(move || visit(5, ptr));
    }
    // `queue` will be dropped asynchronously and then the `finalizer` of the `queue`
    // should be fired to clean up the `context` set in the `queue`.

    assert_eq!(visited, vec![1, 2, 3]);
}
