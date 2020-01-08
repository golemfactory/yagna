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
    pub fn get(task: Task, trigger: Trigger) -> Job {
        Job {
            task: task,
            trigger: trigger,
        }
    }

    /// Check if a task is scheduled to run again
    pub fn is_pending(&self) -> bool {
        self.trigger.is_ready()
    }

    /// Run a task and re-schedule it
    pub fn execute(&mut self) {
        self.task.execute();
        self.trigger.tick();
    }
}

pub struct Scheduler {
    name: String,
    jobs: Vec<Job>,
}

impl<'a> Scheduler {
    pub fn get_scheduler(name: String) -> Scheduler {
        Scheduler {
            name: name,
            jobs: vec![],
        }
    }

    pub fn schedule_task(&mut self, task: Task, trigger: Trigger) {
        println!("Scheduled task: {:?} with trigger: {:?}", task, trigger);
        let job = Job::get(task, trigger);
        self.jobs.push(job);
    }

    pub fn start(&mut self) {
        println!("Scheduler started");
        loop {
            self.run_pending();
            thread::sleep(Duration::from_secs(1));
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
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
