extern crate dispatch;
use dispatch::Queue;

fn main() {
    let queue = Queue::new("Task runner");

    queue.run_sync(|| println!("1"));
    queue.run_sync(|| println!("2"));
    queue.run_async(|| println!("3"));
    queue.run_async(|| println!("4"));
    // Call sync here to block the current thread and make sure all the tasks are done.
    queue.run_sync(|| println!("5"));
}
