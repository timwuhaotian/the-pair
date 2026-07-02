use crate::types::PairState;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::process::Child;

pub struct ResourceMonitor;

impl ResourceMonitor {
    pub fn update_state(
        state: &mut PairState,
        sys: &mut System,
        active_processes: Arc<Mutex<HashMap<String, Child>>>,
    ) {
        let processes = active_processes.lock().unwrap_or_else(|e| e.into_inner());

        // Collect the specific PIDs we track for this pair.
        let mentor_key = format!("{}-mentor", state.pair_id);
        let executor_key = format!("{}-executor", state.pair_id);

        let mentor_pid = processes.get(&mentor_key).and_then(|c| c.id());
        let executor_pid = processes.get(&executor_key).and_then(|c| c.id());

        // Only refresh the processes we actually track instead of the entire
        // system — refresh_all() is far too expensive for a 1 s poll loop.
        let pids: Vec<Pid> = [mentor_pid, executor_pid]
            .into_iter()
            .flatten()
            .map(Pid::from_u32)
            .collect();
        sys.refresh_processes(ProcessesToUpdate::Some(&pids), true);

        let mut mentor_cpu = 0.0;
        let mut mentor_mem = 0.0;
        let mut executor_cpu = 0.0;
        let mut executor_mem = 0.0;

        if let Some(pid) = mentor_pid {
            if let Some(process) = sys.process(Pid::from_u32(pid)) {
                mentor_cpu = process.cpu_usage() as f64;
                mentor_mem = process.memory() as f64 / 1024.0 / 1024.0;
            }
        }

        if let Some(pid) = executor_pid {
            if let Some(process) = sys.process(Pid::from_u32(pid)) {
                executor_cpu = process.cpu_usage() as f64;
                executor_mem = process.memory() as f64 / 1024.0 / 1024.0;
            }
        }

        #[cfg(debug_assertions)]
        {
            println!(
                "[ResourceMonitor] pair={}, mentor_pid={:?}, mentor_cpu={:.2}%, mentor_mem={:.2}MB, executor_pid={:?}, executor_cpu={:.2}%, executor_mem={:.2}MB",
                state.pair_id, mentor_pid, mentor_cpu, mentor_mem, executor_pid, executor_cpu, executor_mem
            );
        }

        state.resources.mentor.cpu = mentor_cpu;
        state.resources.mentor.mem_mb = mentor_mem;
        state.resources.executor.cpu = executor_cpu;
        state.resources.executor.mem_mb = executor_mem;
        state.resources.pair_total.cpu = mentor_cpu + executor_cpu;
        state.resources.pair_total.mem_mb = mentor_mem + executor_mem;
    }
}
