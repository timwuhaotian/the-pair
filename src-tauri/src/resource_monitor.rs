use crate::process_spawner::ActiveProcessMap;
use crate::types::{PairResources, ResourceInfo};
use sysinfo::{Pid, ProcessesToUpdate, System};

pub struct ResourceMonitor;

impl ResourceMonitor {
    /// Sample CPU/memory for the pair's live mentor and executor processes.
    /// Takes no pair-state lock: the caller writes the result back itself, so
    /// the sysinfo refresh never runs while the global broker state is held.
    pub fn sample(
        pair_id: &str,
        sys: &mut System,
        active_processes: &ActiveProcessMap,
    ) -> PairResources {
        let (mentor_pid, executor_pid) = {
            let processes = active_processes.lock().unwrap_or_else(|e| e.into_inner());
            let pid_for = |role: &str| {
                processes
                    .get(&format!("{}-{}", pair_id, role))
                    .and_then(|process| process.child.id())
            };
            (pid_for("mentor"), pid_for("executor"))
        };

        // Only refresh the processes we actually track instead of the entire
        // system — refresh_all() is far too expensive for a periodic poll.
        let pids: Vec<Pid> = [mentor_pid, executor_pid]
            .into_iter()
            .flatten()
            .map(Pid::from_u32)
            .collect();
        if !pids.is_empty() {
            sys.refresh_processes(ProcessesToUpdate::Some(&pids), true);
        }

        let usage = |pid: Option<u32>| -> ResourceInfo {
            pid.and_then(|pid| sys.process(Pid::from_u32(pid)))
                .map(|process| ResourceInfo {
                    cpu: process.cpu_usage() as f64,
                    mem_mb: process.memory() as f64 / 1024.0 / 1024.0,
                })
                .unwrap_or(ResourceInfo {
                    cpu: 0.0,
                    mem_mb: 0.0,
                })
        };
        let mentor = usage(mentor_pid);
        let executor = usage(executor_pid);

        #[cfg(debug_assertions)]
        {
            println!(
                "[ResourceMonitor] pair={}, mentor_pid={:?}, mentor_cpu={:.2}%, mentor_mem={:.2}MB, executor_pid={:?}, executor_cpu={:.2}%, executor_mem={:.2}MB",
                pair_id, mentor_pid, mentor.cpu, mentor.mem_mb, executor_pid, executor.cpu, executor.mem_mb
            );
        }

        PairResources {
            pair_total: ResourceInfo {
                cpu: mentor.cpu + executor.cpu,
                mem_mb: mentor.mem_mb + executor.mem_mb,
            },
            mentor,
            executor,
        }
    }
}
