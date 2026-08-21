//! Python ASR sidecar：JSONL 行协议 + 进程生命周期管理。
//! stdout 读线程推事件进 channel；stderr 转发到日志文件；
//! 终止升级链：SIGTERM（500ms）→ SIGKILL（2s）→ killpg 兜底（子进程 setsid 独立进程组）。

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const SIDECAR_PROTOCOL_VERSION: u32 = 1;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// stdin 命令（每行一个 JSON）。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum SidecarCommand {
    Hello {
        version: u32,
    },
    Transcribe {
        task_id: String,
        wav_path: String,
        model: String,
        /// 已由模型管理器解析的本地绝对 ASR 权重目录；不会返回前端。
        model_dir: String,
        /// 已由模型管理器解析的本地绝对 ForcedAligner 权重目录；不会返回前端。
        aligner_dir: String,
        language: String,
        source_sample_rate: i64,
        chunk_seconds: i64,
    },
    Diarize {
        task_id: String,
        /// 16kHz 单声道准备音频；原媒体路径不会发送给说话人后端。
        wav_path: String,
        /// 已由模型管理器解析的本地 VAD 目录或 bundled runtime 标识。
        vad_model_dir: String,
        /// 已由模型管理器解析的本地 WeSpeaker 权重目录。
        speaker_model_dir: String,
        /// 将 VAD/嵌入区间转换回源素材的采样率。
        source_sample_rate: i64,
    },
    Cancel {
        task_id: String,
    },
}

/// 一个词（源采样域整数，由 sidecar 完成 16k → 源采样率的换算）。
#[derive(Debug, Clone, Deserialize)]
pub struct SidecarWord {
    pub raw_text: String,
    pub display_text: String,
    pub start_sample: i64,
    pub end_sample: i64,
    pub confidence: Option<f64>,
    pub language: Option<String>,
}

/// 本地说话人 sidecar 的一个匿名聚类区间。声纹向量单独返回，只在项目本地存储。
#[derive(Debug, Clone, Deserialize)]
pub struct SidecarSpeakerSegment {
    pub cluster_id: String,
    pub start_sample: i64,
    pub end_sample: i64,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SidecarSpeakerEmbedding {
    pub cluster_id: String,
    pub values: Vec<f32>,
}

/// stdout 事件（每行一个 JSON）。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SidecarEvent {
    Ready {
        version: u32,
        pid: u32,
        #[serde(default)]
        mock: bool,
    },
    Progress {
        task_id: String,
        completed: Option<u64>,
        total: Option<u64>,
        message: String,
    },
    Words {
        task_id: String,
        chunk: i64,
        words: Vec<SidecarWord>,
    },
    SpeakerSegments {
        task_id: String,
        segments: Vec<SidecarSpeakerSegment>,
        embeddings: Vec<SidecarSpeakerEmbedding>,
    },
    Done {
        task_id: String,
        word_count: u64,
    },
    DiarizationDone {
        task_id: String,
        segment_count: u64,
    },
    Cancelled {
        task_id: String,
    },
    Error {
        task_id: Option<String>,
        code: String,
        message: String,
        fatal: bool,
    },
}

#[derive(Debug)]
pub enum SidecarError {
    Spawn(std::io::Error),
    Handshake(String),
    ProtocolMismatch { expected: u32, got: u32 },
    Io(std::io::Error),
}

impl std::fmt::Display for SidecarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "sidecar 启动失败：{error}"),
            Self::Handshake(reason) => write!(f, "sidecar 握手失败：{reason}"),
            Self::ProtocolMismatch { expected, got } => {
                write!(f, "sidecar 协议版本不符：期望 {expected}，实际 {got}")
            }
            Self::Io(error) => write!(f, "sidecar IO 错误：{error}"),
        }
    }
}

impl std::error::Error for SidecarError {}

type EventLine = Result<SidecarEvent, String>;

/// Sidecar 的下一次读取结果。`Closed` 与 `TimedOut` 必须区分：前者说明 stdout
/// 已经结束，继续等待只会让用户看到无意义的“正在转录”。
#[derive(Debug)]
pub enum SidecarPoll {
    Event(EventLine),
    TimedOut,
    Closed,
}

fn poll_receiver(events: &Receiver<EventLine>, timeout: Duration) -> SidecarPoll {
    match events.recv_timeout(timeout) {
        Ok(event) => SidecarPoll::Event(event),
        Err(RecvTimeoutError::Timeout) => SidecarPoll::TimedOut,
        Err(RecvTimeoutError::Disconnected) => SidecarPoll::Closed,
    }
}

/// 一个运行中的 sidecar 进程。
pub struct Sidecar {
    child: Child,
    stdin: Option<ChildStdin>,
    events: Receiver<EventLine>,
    pid: u32,
}

impl Sidecar {
    /// 启动 sidecar 并完成 hello/ready 握手（最多等 10 秒）。
    /// `package_dir` 为 double_love_asr 包所在目录（sidecars/asr）。
    pub fn spawn(
        python: &Path,
        package_dir: &Path,
        mock: bool,
        log_path: &Path,
    ) -> Result<Self, SidecarError> {
        Self::spawn_module(python, package_dir, "double_love_asr", mock, log_path)
    }

    /// 启动共享 JSONL 协议的某个本地 sidecar 模块。ASR 与 Speaker 都复用同一握手、
    /// 日志和进程终止规则，避免其中一个在 stdout 关闭后空等。
    pub fn spawn_module(
        python: &Path,
        package_dir: &Path,
        module: &str,
        mock: bool,
        log_path: &Path,
    ) -> Result<Self, SidecarError> {
        // `current_dir(package_dir)` 会改变相对解释器路径的语义；CLI 默认传相对的
        // sidecar 目录，因此先固定解释器绝对路径，避免启动时误找 package_dir/.venv。
        let python = python
            .canonicalize()
            .unwrap_or_else(|_| python.to_path_buf());
        let mut command = Command::new(python);
        command
            .arg("-m")
            .arg(module)
            .current_dir(package_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if mock {
            command.env("DOUBLELOVE_ASR_MOCK", "1");
            command.env("DOUBLELOVE_SPEAKER_MOCK", "1");
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // 独立进程组：终止升级链最后可 killpg 兜底整组
            unsafe {
                command.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        let mut child = command.spawn().map_err(SidecarError::Spawn)?;
        let pid = child.id();
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| SidecarError::Handshake("stdin 不可用".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| SidecarError::Handshake("stdout 不可用".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| SidecarError::Handshake("stderr 不可用".to_string()))?;

        let (tx, rx) = mpsc::channel::<EventLine>();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let event = match line {
                    Ok(text) => serde_json::from_str::<SidecarEvent>(&text)
                        .map_err(|error| format!("{error}: {text}")),
                    Err(error) => Err(format!("stdout 读取失败：{error}")),
                };
                if tx.send(event).is_err() {
                    return; // 接收端已 drop
                }
            }
        });

        // stderr → 日志文件（尽力而为，写失败不致命）
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut log) = std::fs::File::create(log_path) {
            thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                    let _ = log.write_all(line.as_bytes());
                    line.clear();
                }
            });
        }

        let mut sidecar = Self {
            child,
            stdin: Some(stdin),
            events: rx,
            pid,
        };
        sidecar.send(&SidecarCommand::Hello {
            version: SIDECAR_PROTOCOL_VERSION,
        })?;
        match sidecar.next_event(HANDSHAKE_TIMEOUT) {
            SidecarPoll::Event(Ok(SidecarEvent::Ready { version, .. }))
                if version == SIDECAR_PROTOCOL_VERSION =>
            {
                Ok(sidecar)
            }
            SidecarPoll::Event(Ok(SidecarEvent::Ready { version, .. })) => {
                Err(SidecarError::ProtocolMismatch {
                    expected: SIDECAR_PROTOCOL_VERSION,
                    got: version,
                })
            }
            SidecarPoll::Event(Ok(other)) => Err(SidecarError::Handshake(format!(
                "首个事件不是 ready：{other:?}"
            ))),
            SidecarPoll::Event(Err(reason)) => Err(SidecarError::Handshake(reason)),
            SidecarPoll::TimedOut => Err(SidecarError::Handshake(
                "10 秒内未收到 ready 事件".to_string(),
            )),
            SidecarPoll::Closed => Err(SidecarError::Handshake(
                "sidecar 在握手前关闭 stdout".to_string(),
            )),
        }
    }

    pub fn send(&mut self, command: &SidecarCommand) -> Result<(), SidecarError> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| SidecarError::Handshake("stdin 已关闭".to_string()))?;
        let mut line = serde_json::to_string(command)
            .map_err(|error| SidecarError::Handshake(format!("命令序列化失败：{error}")))?;
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.flush())
            .map_err(SidecarError::Io)
    }

    /// 等下一个事件，显式区分超时与 stdout 关闭。
    pub fn next_event(&self, timeout: Duration) -> SidecarPoll {
        poll_receiver(&self.events, timeout)
    }

    /// 终止升级链：SIGTERM → 500ms → SIGKILL → 2s → killpg 兜底。
    #[cfg(unix)]
    fn terminate(&mut self) {
        if self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        unsafe {
            libc::kill(self.pid as i32, libc::SIGTERM);
        }
        if self.wait_up_to(Duration::from_millis(500)) {
            return;
        }
        unsafe {
            libc::kill(self.pid as i32, libc::SIGKILL);
        }
        if self.wait_up_to(Duration::from_secs(2)) {
            return;
        }
        unsafe {
            libc::killpg(self.pid as i32, libc::SIGKILL); // setsid 后 pgid == pid
        }
        let _ = self.child.wait();
    }

    #[cfg(not(unix))]
    fn terminate(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn wait_up_to(&mut self, budget: Duration) -> bool {
        let start = std::time::Instant::now();
        loop {
            if self.child.try_wait().ok().flatten().is_some() {
                return true;
            }
            if start.elapsed() >= budget {
                return false;
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnected_stdout_is_not_reported_as_timeout() {
        let (sender, receiver) = mpsc::channel::<EventLine>();
        drop(sender);
        assert!(matches!(
            poll_receiver(&receiver, Duration::from_millis(1)),
            SidecarPoll::Closed
        ));
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        // 先关 stdin：sidecar 读到 EOF 自行退出（daemon worker 随之结束）
        drop(self.stdin.take());
        if self.wait_up_to(Duration::from_secs(2)) {
            return;
        }
        self.terminate();
    }
}

/// Python 解释器发现：显式覆盖 → sidecar venv → PATH 里的 python3。
pub fn resolve_python(override_path: Option<&Path>, venv_python: &Path) -> Option<PathBuf> {
    if let Some(explicit) = override_path
        && explicit.is_file()
    {
        return Some(explicit.to_path_buf());
    }
    if venv_python.is_file() {
        return Some(venv_python.to_path_buf());
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join("python3"))
            .find(|candidate| candidate.is_file())
    })
}
