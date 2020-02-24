extern crate dispatch;
use dispatch::Queue;

use std::{thread, time::Duration};

// END_DELAY must be larger than SWITCH_DELAY to show the tasks are dropped or not.
const SWITCH_DELAY: u64 = 10;
const END_DELAY: u64 = 50;

fn main() {
    println!("\n1) All the tasks will be executed.");
    run_tasks(false);

    println!("\n2) After queue is dropped. All the pending tasks will be cancelled.");
    run_tasks(true);
}

fn run_tasks(cancel_task_after_drop: bool) {
    {
        let queue = Queue::new("Task Runner", cancel_task_after_drop);
        queue.run_async(|| println!("1"));
        queue.run_async(|| println!("2"));
        queue.run_sync(|| println!("3"));
        queue.run_sync(|| println!("4"));
        queue.run_async(|| {
            println!(
                "5: Force context switch to let `queue` be dropped, by sleep {} milliseconds.",
                SWITCH_DELAY
            );
            sleep(SWITCH_DELAY);
        });
        queue.run_async(move || {
            println!("6");
            assert!(!cancel_task_after_drop, "this should be cancelled.");
        });
    }
    println!(
        "queue is dropped. Sleep {} milliseconds to terminate.",
        END_DELAY
    );
    sleep(END_DELAY);
}

fn sleep(millis: u64) {
    let duration = Duration::from_millis(millis);
    thread::sleep(duration);
}
