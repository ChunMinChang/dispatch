mod sys;

use std::ffi::CString;
use std::os::raw::c_void;
use std::sync::Mutex;

// Queue: A wrapper around `dispatch_queue_t`.
// ------------------------------------------------------------------------------------------------
pub struct Queue(sys::dispatch_queue_t);

impl Queue {
    #[allow(clippy::mutex_atomic)] // The mutex is used to create a critical section.
    pub fn new(label: &'static str, cancel_task_after_drop: bool) -> Self {
        let q = Self(create_dispatch_queue(label, sys::DISPATCH_QUEUE_SERIAL));
        if cancel_task_after_drop {
            let is_destroying = Box::new(Mutex::new(false));
            q.set_context(is_destroying);
        }
        q
    }

    pub fn run_async<F>(&self, work: F)
    where
        F: 'static + Send + FnOnce(),
    {
        let context = self.get_context::<Mutex<bool>>();
        dispatch_async(self.0, move || {
            if context.is_none() {
                work();
                return;
            }

            // Enter a critical section to prevent this queue from being dropped while this
            // task is running.
            let is_destroying = context.unwrap().lock().unwrap();
            if *is_destroying {
                return;
            }
            work();
        });
    }

    pub fn run_sync<F>(&self, work: F)
    where
        F: 'static + Send + FnOnce(),
    {
        let context = self.get_context::<Mutex<bool>>();
        dispatch_sync(self.0, move || {
            if context.is_none() {
                work();
                return;
            }

            // Enter a critical section to prevent this queue from being dropped while this
            // task is running.
            let is_destroying = context.unwrap().lock().unwrap();
            if *is_destroying {
                return;
            }
            work();
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
        let context = self.get_context::<Mutex<bool>>();
        if context.is_none() {
            release_dispatch_queue(self.0);
            return;
        }
        // Enter a critical section to make sure this queue won't be destroyed while
        // there is a running async task.
        let mut is_destroying = context.unwrap().lock().unwrap();
        *is_destroying = true;
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
    // The tasks will be executed in order, no matter whether they can be cancelled.
    run_tasks(false);
    run_tasks(true);

    fn run_tasks(cancel_task_after_drop: bool) {
        let mut visited = Vec::<u32>::new();

        // Rust compilter doesn't allow a pointer to be passed across threads.
        // A hacky way to do that is to cast the pointer into a value, then
        // the value, which is actually an address, can be copied into threads.
        let ptr = &mut visited as *mut Vec<u32> as usize;

        fn visit(v: u32, visited_ptr: usize) {
            let visited = unsafe { &mut *(visited_ptr as *mut Vec<u32>) };
            visited.push(v);
        };

        let queue = Queue::new("Run tasks in order", cancel_task_after_drop);

        queue.run_sync(move || visit(1, ptr));
        queue.run_sync(move || visit(2, ptr));
        queue.run_async(move || visit(3, ptr));
        queue.run_async(move || visit(4, ptr));
        queue.run_sync(move || visit(5, ptr));

        assert_eq!(visited, vec![1, 2, 3, 4, 5]);
    }
}

#[test]
fn no_tasks_after_drop() {
    use std::convert::TryFrom;
    use std::{thread, time::Duration};

    const ROUNDS: u64 = 50;
    const TASK_DELAY: u64 = 1;
    const END_DELAY: u64 = ROUNDS * TASK_DELAY * 2;

    fn sleep(millis: u64) {
        let duration = Duration::from_millis(millis);
        thread::sleep(duration);
    }

    let mut visited = Vec::<u32>::new();

    // Rust compilter doesn't allow a pointer to be passed across threads.
    // A hacky way to do that is to cast the pointer into a value, then
    // the value, which is actually an address, can be copied into threads.
    let ptr = &mut visited as *mut Vec<u32> as usize;

    fn visit(v: u32, visited_ptr: usize) {
        let visited = unsafe { &mut *(visited_ptr as *mut Vec<u32>) };
        visited.push(v);
    };

    {
        let queue = Queue::new("Cancel tasks after queue is dropped", true);
        for i in 1..ROUNDS + 1 {
            queue.run_async(move || {
                visit(u32::try_from(i).unwrap(), ptr);
                sleep(TASK_DELAY);
            });
        }
    }

    visited.push(0);
    sleep(END_DELAY);
    assert_eq!(visited.last().unwrap(), &0);
}

#[test]
fn cannot_drop_while_running_task() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::{thread, time::Duration};

    const DROP_DELAY: u64 = 10;
    const TASK_DELAY: u64 = 30;
    const END_DELAY: u64 = 60;
    assert!(
        DROP_DELAY < TASK_DELAY,
        "Need to make it easier to let task is executed before queue is dropped."
    );
    assert!(
        END_DELAY > TASK_DELAY - DROP_DELAY,
        "Need to make sure the task is finished if it's executed."
    );

    fn sleep(millis: u64) {
        let duration = Duration::from_millis(millis);
        thread::sleep(duration);
    }

    let touch_1 = Arc::new(AtomicBool::new(false));
    let touch_2 = Arc::new(AtomicBool::new(false));

    {
        let queue = Queue::new("Cannot drop queue while there is a running task", true);

        let touch_1_clone = touch_1.clone();
        let touch_2_clone = touch_2.clone();
        queue.run_async(move || {
            touch_1_clone.store(true, Ordering::SeqCst);
            sleep(TASK_DELAY);
            touch_2_clone.store(true, Ordering::SeqCst);
        });
        sleep(DROP_DELAY);
    }

    sleep(END_DELAY);
    assert_eq!(
        touch_1.load(Ordering::SeqCst),
        touch_2.load(Ordering::SeqCst)
    );
}
