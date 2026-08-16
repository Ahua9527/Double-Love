//! 任务运行时：TaskRegistry + CancellationToken + ProgressSink。
//! 刻意不上 tokio——任务本质是「阻塞 IO + 子进程等待」，std::thread 足够且可取消。
//! 状态机：pending → running → (succeeded | failed | partial | cancelled)，终态不可逆。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::{ProgressEvent, TaskState};

/// 取消令牌：克隆廉价，跨线程共享一个原子标志。
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    flag: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

/// 进度与终态的上报出口（CLI 打 stderr；Tauri emit 事件）。
pub trait ProgressSink: Send + Sync + 'static {
    fn progress(&self, event: ProgressEvent);
    fn task_state(&self, task_id: &str, state: TaskState);
}

pub type SharedSink = Arc<dyn ProgressSink>;

/// 运行中（或已结束）任务的句柄。
pub struct TaskHandle {
    state: Arc<Mutex<TaskState>>,
    cancel_token: CancellationToken,
    join: Option<JoinHandle<()>>,
}

impl TaskHandle {
    pub fn state(&self) -> TaskState {
        *self.state.lock().expect("task state lock")
    }

    fn is_terminal(&self) -> bool {
        !matches!(self.state(), TaskState::Pending | TaskState::Running)
    }
}

/// 任务注册表：同 task_id 非终态拒绝重复，终态可被新任务替换。
#[derive(Default)]
pub struct TaskRegistry {
    tasks: Mutex<HashMap<String, TaskHandle>>,
}

impl TaskRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 启动一个任务线程。work 返回期望终态；若取消标志已置位则强制记 Cancelled。
    pub fn spawn<F>(&self, task_id: &str, sink: SharedSink, work: F) -> Result<(), String>
    where
        F: FnOnce(CancellationToken, SharedSink) -> TaskState + Send + 'static,
    {
        let mut tasks = self.tasks.lock().expect("registry lock");
        if let Some(existing) = tasks.get(task_id) {
            if !existing.is_terminal() {
                return Err(format!("任务 {task_id} 已在运行"));
            }
            tasks.remove(task_id);
        }

        let state = Arc::new(Mutex::new(TaskState::Pending));
        let cancel_token = CancellationToken::default();
        let thread_state = Arc::clone(&state);
        let thread_token = cancel_token.clone();
        let thread_sink = Arc::clone(&sink);
        let owned_id = task_id.to_string();
        let join = std::thread::spawn(move || {
            *thread_state.lock().expect("task state lock") = TaskState::Running;
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                work(thread_token.clone(), thread_sink.clone())
            }));
            let terminal = if thread_token.is_cancelled() {
                TaskState::Cancelled
            } else {
                match outcome {
                    Ok(state) => state,
                    Err(_) => TaskState::Failed, // panic 不得静默
                }
            };
            *thread_state.lock().expect("task state lock") = terminal;
            thread_sink.task_state(&owned_id, terminal);
        });

        tasks.insert(
            task_id.to_string(),
            TaskHandle {
                state,
                cancel_token,
                join: Some(join),
            },
        );
        Ok(())
    }

    pub fn state(&self, task_id: &str) -> Option<TaskState> {
        self.tasks
            .lock()
            .expect("registry lock")
            .get(task_id)
            .map(TaskHandle::state)
    }

    /// 请求取消：任务在运行则置位返回 true；不存在或已终态返回 false。
    pub fn cancel(&self, task_id: &str) -> bool {
        let tasks = self.tasks.lock().expect("registry lock");
        match tasks.get(task_id) {
            Some(handle) if !handle.is_terminal() => {
                handle.cancel_token.cancel();
                true
            }
            _ => false,
        }
    }

    /// 取消全部任务并等待线程结束（应用退出时调用）。
    pub fn shutdown(&self) {
        let mut tasks = self.tasks.lock().expect("registry lock");
        for handle in tasks.values() {
            handle.cancel_token.cancel();
        }
        for (_, mut handle) in tasks.drain() {
            if let Some(join) = handle.join.take() {
                let _ = join.join();
            }
        }
    }
}

impl Drop for TaskRegistry {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[derive(Default)]
    struct VecSink {
        progress: Mutex<Vec<ProgressEvent>>,
        states: Mutex<Vec<(String, TaskState)>>,
    }

    impl ProgressSink for VecSink {
        fn progress(&self, event: ProgressEvent) {
            self.progress.lock().expect("sink lock").push(event);
        }
        fn task_state(&self, task_id: &str, state: TaskState) {
            self.states
                .lock()
                .expect("sink lock")
                .push((task_id.to_string(), state));
        }
    }

    fn wait_for_terminal(registry: &TaskRegistry, task_id: &str) -> TaskState {
        for _ in 0..200 {
            if let Some(state) = registry.state(task_id)
                && !matches!(state, TaskState::Pending | TaskState::Running)
            {
                return state;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("task {task_id} never reached a terminal state");
    }

    #[test]
    fn runs_to_success_and_reports_progress() {
        let registry = TaskRegistry::new();
        let sink = Arc::new(VecSink::default());
        registry
            .spawn("t1", sink.clone(), |_token, sink| {
                sink.progress(ProgressEvent {
                    task: "t1".to_string(),
                    phase: "work".to_string(),
                    completed: Some(1),
                    total: Some(2),
                    message: "half".to_string(),
                });
                TaskState::Succeeded
            })
            .expect("spawn");
        assert_eq!(wait_for_terminal(&registry, "t1"), TaskState::Succeeded);
        assert_eq!(sink.progress.lock().expect("sink lock").len(), 1);
        let states = sink.states.lock().expect("sink lock");
        assert_eq!(
            states.as_slice(),
            [("t1".to_string(), TaskState::Succeeded)]
        );
        registry.shutdown();
    }

    #[test]
    fn cancel_marks_cancelled_even_if_work_returns_success() {
        let registry = TaskRegistry::new();
        let sink = Arc::new(VecSink::default());
        registry
            .spawn("t2", sink, |token, _sink| {
                while !token.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                TaskState::Succeeded // 即使 work 说成功，取消标志优先
            })
            .expect("spawn");
        std::thread::sleep(Duration::from_millis(20));
        assert!(registry.cancel("t2"));
        assert_eq!(wait_for_terminal(&registry, "t2"), TaskState::Cancelled);
        assert!(!registry.cancel("t2"), "终态后再取消应无效");
        registry.shutdown();
    }

    #[test]
    fn rejects_duplicate_running_task_but_allows_replace_after_terminal() {
        let registry = TaskRegistry::new();
        let sink = Arc::new(VecSink::default());
        registry
            .spawn("t3", sink.clone(), |token, _sink| {
                while !token.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                TaskState::Cancelled
            })
            .expect("first spawn");
        assert!(
            registry
                .spawn("t3", sink.clone(), |_, _| TaskState::Succeeded)
                .is_err()
        );
        registry.cancel("t3");
        wait_for_terminal(&registry, "t3");
        registry
            .spawn("t3", sink, |_, _| TaskState::Succeeded)
            .expect("terminal task can be replaced");
        assert_eq!(wait_for_terminal(&registry, "t3"), TaskState::Succeeded);
        registry.shutdown();
    }

    #[test]
    fn panicking_work_becomes_failed() {
        let registry = TaskRegistry::new();
        let sink = Arc::new(VecSink::default());
        registry
            .spawn("t4", sink.clone(), |_, _| panic!("boom"))
            .expect("spawn");
        assert_eq!(wait_for_terminal(&registry, "t4"), TaskState::Failed);
        let states = sink.states.lock().expect("sink lock");
        assert_eq!(states.as_slice(), [("t4".to_string(), TaskState::Failed)]);
        registry.shutdown();
    }
}
