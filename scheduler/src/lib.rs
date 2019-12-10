use job_scheduler::{Job, JobScheduler, Schedule};

pub struct TaskBuilder {}

impl TaskBuilder {
    pub fn build_task<'a, F>(schedule: Schedule, func: F) -> Job<'a>
    where
        F: 'a,
        F: FnMut(),
    {
        Job::new(schedule, func)
    }
}

pub struct TaskScheduler<'a> {
    scheduler: JobScheduler<'a>,
}

impl<'a> TaskScheduler<'a> {
    pub fn build() -> TaskScheduler<'a> {
        TaskScheduler {
            scheduler: JobScheduler::new(),
        }
    }

    pub fn add_task(&mut self, job: Job<'a>) {
        self.scheduler.add(job);
    }

    pub fn run(&mut self) {
        loop {
            self.scheduler.tick();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
