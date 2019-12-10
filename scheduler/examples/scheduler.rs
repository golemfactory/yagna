use ya_scheduler;

fn do_void(number: i64) {
    println!("Number is: {}", number)
}

fn main() {
    let mut scheduler = ya_scheduler::TaskScheduler::build();
    scheduler.add_task(ya_scheduler::TaskBuilder::build_task(
        "1/1 * * * * *".parse().unwrap(),
        || do_void(69),
    ));
    scheduler.run();
}
