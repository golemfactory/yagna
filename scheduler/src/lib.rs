extern crate chrono;

use std::thread;
use std::time::Duration;

mod task;
mod trigger;

pub use task::Task;
pub use trigger::{Interval, Trigger};

pub struct Job {
    task: Task,
    trigger: Trigger,
}

impl Job {
    fn new(task: Task, trigger: Trigger) -> Job {
        Job {
            task: task,
            trigger: trigger,
        }
    }

    fn is_pending(&self) -> bool {
        self.trigger.is_ready()
    }

    fn execute(&mut self) {
        self.task.execute();
        self.trigger.tick();
    }
}

pub struct Scheduler {
    name: String,
    tick_time: u64,
    jobs: Vec<Job>,
}

impl<'a> Scheduler {
    pub fn new(name: String, tick_time: u64) -> Scheduler {
        Scheduler {
            name: name,
            tick_time: tick_time,
            jobs: vec![],
        }
    }

    pub fn schedule_task(&mut self, task: Task, trigger: Trigger) {
        println!("Scheduled task: {:?} with trigger: {:?}", task, trigger);
        let job = Job::new(task, trigger);
        self.jobs.push(job);
    }

    pub fn start(&mut self) {
        println!("Scheduler started");
        loop {
            self.run_pending();
            thread::sleep(Duration::from_millis(self.tick_time));
        }
    }

    fn run_pending(&mut self) {
        for job in &mut self.jobs {
            if job.is_pending() {
                job.execute();
            }
        }
    }

    pub fn shutdown(&mut self) {
        println!("Scheduler stopped");
    }

    pub fn status(&self) {
        println!("Current status of: {}", self.name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Local};
    use std::thread;
    use std::time;
    #[test]
    fn test_job_is_ready() {
        let task = Task::new(String::from("task1"), || println!("1"));
        let days = 1;
        let interval = Interval::new(days, 0, 0, 0);
        let trigger = Trigger::new(String::from("trigger1"), Local::now(), interval);
        thread::sleep(time::Duration::from_secs(1));
        let job = Job::new(task, trigger);
        assert!(job.is_pending());
    }

    #[test]
    fn test_job_is_not_ready() {
        let task = Task::new(String::from("task1"), || println!("1"));
        let days = 1;
        let interval = Interval::new(days, 0, 0, 0);
        let trigger = Trigger::new(
            String::from("trigger1"),
            Local::now() + Duration::days(2 as i64),
            interval,
        );
        let job = Job::new(task, trigger);
        assert!(!job.is_pending());
    }

    #[test]
    fn test_job_execute() {
        let task = Task::new(String::from("task1"), || println!("1"));
        let days = 1;
        let interval = Interval::new(days, 0, 0, 0);
        let trigger = Trigger::new(String::from("trigger1"), Local::now(), interval);
        let mut job = Job::new(task, trigger);
        job.execute();
        assert!(!job.is_pending());
    }

    #[test]
    fn test_scheduler_schedule_task() {
        let mut scheduler = Scheduler::new(String::from("Scheduler"), 1 as u64);
        let task = Task::new(String::from("task1"), || println!("1"));
        let interval = Interval::new(1, 0, 0, 0);
        let trigger = Trigger::new(String::from("trigger1"), Local::now(), interval);
        assert_eq!(scheduler.jobs.len(), 0);
        scheduler.schedule_task(task, trigger);
        assert_eq!(scheduler.jobs.len(), 1);
    }
}
