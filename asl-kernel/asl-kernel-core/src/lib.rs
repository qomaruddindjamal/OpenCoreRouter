//! ASL Core Kernel — asl-kernel-core
//! Version: v1.0.0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Bare-metal Rust `#![no_std]` Kernel Executive

#![no_std]

pub const KERNEL_NAME: &str = "ASL Core Kernel";
pub const KERNEL_VERSION: &str = "v1.0.0";
pub const OS_MARKER: &str = "ASL_CORE_KERNEL_v1_0_0";

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

    pub fn register_partition(
        &mut self,
        partition: PartitionDescriptor,
    ) -> Result<usize, &'static str> {
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

    pub fn count_partitions(&self) -> usize {
        let mut count = 0;
        let mut i = 0;
        while i < self.active_partitions.len() {
            if self.active_partitions[i].is_some() {
                count += 1;
            }
            i += 1;
        }
        count
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
        let slot = exec
            .register_partition(p0)
            .expect("Registration should succeed");
        assert_eq!(slot, 0);

        let retrieved = exec.get_partition(slot).expect("Should exist");
        assert_eq!(retrieved.partition_id, 1);
        assert_eq!(retrieved.period_us, 10_000);
        assert_eq!(exec.version_info(), "ASL_CORE_KERNEL_v1_0_0");
    }

    #[test]
    fn test_kernel_executive_capacity_limit() {
        let mut exec = KernelExecutive::new();
        for i in 0..16 {
            let p = PartitionDescriptor::new(i as u32, 5000, 1000);
            assert!(exec.register_partition(p).is_ok());
        }
        let overflow = PartitionDescriptor::new(99, 5000, 1000);
        assert!(exec.register_partition(overflow).is_err());
        assert_eq!(exec.count_partitions(), 16);
    }

    #[test]
    fn test_partition_descriptor_states() {
        let mut p = PartitionDescriptor::new(5, 20_000, 5_000);
        assert_eq!(p.state, PartitionState::Idle);
        p.state = PartitionState::Running;
        assert_eq!(p.state, PartitionState::Running);
    }
}
