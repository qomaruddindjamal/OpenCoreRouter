//! Open Core Router (OpenCR) — opcr-core
//! Version: RINJANI_1_0_3
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Bare-metal Rust `#![no_std]` Kernel Executive

#![no_std]

pub const OS_CODENAME: &str = "RINJANI";
pub const OS_VERSION: &str = "1.0.3";
pub const OS_MARKER: &str = "RINJANI_1_0_3";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionState {
    Idle,
    Ready,
    Running,
    Suspended,
    Halted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartitionDescriptor {
    pub partition_id: u32,
    pub period_us: u64,
    pub duration_us: u64,
    pub state: PartitionState,
}

impl PartitionDescriptor {
    pub const fn new(partition_id: u32, period_us: u64, duration_us: u64) -> Self {
        Self {
            partition_id,
            period_us,
            duration_us,
            state: PartitionState::Idle,
        }
    }
}

pub struct KernelExecutive {
    active_partitions: [Option<PartitionDescriptor>; 16],
    pub current_partition_idx: usize,
}

impl KernelExecutive {
    pub const fn new() -> Self {
        Self {
            active_partitions: [None; 16],
            current_partition_idx: 0,
        }
    }

    pub fn register_partition(&mut self, partition: PartitionDescriptor) -> Result<usize, &'static str> {
        let mut i = 0;
        while i < self.active_partitions.len() {
            if self.active_partitions[i].is_none() {
                self.active_partitions[i] = Some(partition);
                return Ok(i);
            }
            i += 1;
        }
        Err("Partition table full")
    }

    pub fn get_partition(&self, slot: usize) -> Option<PartitionDescriptor> {
        if slot < self.active_partitions.len() {
            self.active_partitions[slot]
        } else {
            None
        }
    }

    pub fn version_info(&self) -> &'static str {
        OS_MARKER
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kernel_executive_registration() {
        let mut exec = KernelExecutive::new();
        let p0 = PartitionDescriptor::new(1, 10_000, 2_000);
        let slot = exec.register_partition(p0).expect("Registration should succeed");
        assert_eq!(slot, 0);

        let retrieved = exec.get_partition(slot).expect("Should exist");
        assert_eq!(retrieved.partition_id, 1);
        assert_eq!(retrieved.period_us, 10_000);
        assert_eq!(exec.version_info(), "RINJANI_1_0_3");
    }
}
