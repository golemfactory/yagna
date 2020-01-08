use chrono::Local;
use ya_scheduler::{Interval, Scheduler, Task, Trigger};

fn do_void(number: i64) {
    println!("Number is: {}", number)
}

fn main() {
    let mut scheduler = Scheduler::get_scheduler(String::from("Scheduler"));
    let task1 = Task::new(String::from("do_void_1"), || do_void(69));
    let interval1 = Interval::new(0, 0, 1, 1);
    let trigger1 = Trigger::new(String::from("trigger_1"), Local::now(), interval1);
    scheduler.schedule_task(task1, trigger1);

    let interval2 = Interval::new(0, 0, 0, 1);
    let trigger2 = Trigger::new(String::from("trigger_2"), Local::now(), interval2);
    let task2 = Task::new(String::from("do_void2"), || do_void(6969));
    scheduler.schedule_task(task2, trigger2);
    scheduler.start();
}
