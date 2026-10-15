//! Open Core Router (OpenCR) — asl-sched
//! Version: RINJANI_1_0_3
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Fixed-Priority Preemptive & Cyclic Scheduler

#![no_std]

pub const OS_MARKER: &str = "RINJANI_1_0_3";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskControlBlock {
    pub task_id: u32,
    pub priority: u8,
    pub execution_budget_us: u32,
    pub remaining_budget_us: u32,
}

impl TaskControlBlock {
    pub const fn new(task_id: u32, priority: u8, budget_us: u32) -> Self {
        Self {
            task_id,
            priority,
            execution_budget_us: budget_us,
            remaining_budget_us: budget_us,
        }
    }
}

pub struct PreemptiveScheduler {
    tasks: [Option<TaskControlBlock>; 32],
}

impl PreemptiveScheduler {
    pub const fn new() -> Self {
        Self { tasks: [None; 32] }
    }

    pub fn add_task(&mut self, task: TaskControlBlock) -> Result<(), &'static str> {
        let mut i = 0;
        while i < self.tasks.len() {
            if self.tasks[i].is_none() {
                self.tasks[i] = Some(task);
                return Ok(());
            }
            i += 1;
        }
        Err("Task capacity exceeded")
    }

    pub fn pick_next_task(&self) -> Option<TaskControlBlock> {
        let mut highest: Option<TaskControlBlock> = None;
        let mut i = 0;
        while i < self.tasks.len() {
            if let Some(t) = self.tasks[i] {
                if t.remaining_budget_us > 0 {
                    match highest {
                        None => highest = Some(t),
                        Some(h) => {
                            if t.priority > h.priority {
                                highest = Some(t);
                            }
                        }
                    }
                }
            }
            i += 1;
        }
        highest
    }

    pub fn consume_budget(&mut self, task_id: u32, elapsed_us: u32) {
        let mut i = 0;
        while i < self.tasks.len() {
            if let Some(ref mut t) = self.tasks[i] {
                if t.task_id == task_id {
                    if t.remaining_budget_us > elapsed_us {
                        t.remaining_budget_us -= elapsed_us;
                    } else {
                        t.remaining_budget_us = 0;
                    }
                    return;
                }
            }
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preemptive_scheduling() {
        let mut sched = PreemptiveScheduler::new();
        sched.add_task(TaskControlBlock::new(101, 10, 500)).unwrap();
        sched.add_task(TaskControlBlock::new(102, 50, 200)).unwrap();
        sched.add_task(TaskControlBlock::new(103, 30, 300)).unwrap();

        let chosen = sched.pick_next_task().expect("Should pick a task");
        assert_eq!(chosen.task_id, 102);
        assert_eq!(chosen.priority, 50);
    }

    #[test]
    fn test_scheduler_budget_exhaustion() {
        let mut sched = PreemptiveScheduler::new();
        sched
            .add_task(TaskControlBlock::new(201, 100, 200))
            .unwrap();
        sched.add_task(TaskControlBlock::new(202, 50, 500)).unwrap();

        // 201 terpilih awal
        assert_eq!(sched.pick_next_task().unwrap().task_id, 201);

        // Habiskan budget 201
        sched.consume_budget(201, 250);

        // Sekarang 202 harus terpilih karena 201 habis budget
        assert_eq!(sched.pick_next_task().unwrap().task_id, 202);
    }

    #[test]
    fn test_scheduler_capacity_limit() {
        let mut sched = PreemptiveScheduler::new();
        for i in 0..32 {
            assert!(sched.add_task(TaskControlBlock::new(i, 10, 100)).is_ok());
        }
        assert!(sched.add_task(TaskControlBlock::new(99, 10, 100)).is_err());
    }
}
