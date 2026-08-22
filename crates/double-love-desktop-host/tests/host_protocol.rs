use std::io::{Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use double_love_desktop_host::framing::{MAX_FRAME_BYTES, read_frame, write_frame};
use double_love_desktop_host::protocol::{
    CAPABILITIES, HostRequest, HostRequestMethod, HostResponse, HostResponseStatus, HostResult,
    PROTOCOL_VERSION, UNKNOWN_REQUEST_ID,
};

struct HostProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl HostProcess {
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_double-love-desktop-host"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn desktop host");
        let stdin = child.stdin.take().expect("host stdin");
        let stdout = child.stdout.take().expect("host stdout");
        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn send(&mut self, request: &HostRequest) {
        let body = serde_json::to_vec(request).expect("serialize request");
        write_frame(&mut self.stdin, &body).expect("write request frame");
    }

    fn send_raw(&mut self, body: &[u8]) {
        write_frame(&mut self.stdin, body).expect("write raw frame");
    }

    fn response(&mut self) -> HostResponse {
        let frame = read_frame(&mut self.stdout)
            .expect("read response frame")
            .expect("host response before EOF");
        let response: HostResponse = serde_json::from_slice(&frame).expect("deserialize response");
        assert_eq!(response.v, PROTOCOL_VERSION);
        response
    }

    fn wait(mut self) {
        drop(self.stdin);
        let status = self.child.wait().expect("wait for host");
        assert_eq!(status.code(), Some(0));

        let mut trailing_stdout = Vec::new();
        self.stdout
            .read_to_end(&mut trailing_stdout)
            .expect("read trailing stdout");
        assert!(
            trailing_stdout.is_empty(),
            "stdout must contain framed responses only"
        );

        let mut stderr = String::new();
        self.child
            .stderr
            .take()
            .expect("host stderr")
            .read_to_string(&mut stderr)
            .expect("read host stderr");
        assert!(stderr.is_empty(), "unexpected host log: {stderr}");
    }
}

fn assert_error(response: HostResponse, expected_id: &str, expected_code: &str) {
    assert_eq!(response.id, expected_id);
    match response.response {
        HostResponseStatus::Error { error } => assert_eq!(error.code, expected_code),
        other => panic!("expected {expected_code} error, got {other:?}"),
    }
}

#[test]
fn spawned_host_handles_control_protocol_and_recovers_from_request_errors() {
    let mut host = HostProcess::spawn();

    host.send_raw(br#"{"method":"#);
    assert_error(host.response(), UNKNOWN_REQUEST_ID, "MALFORMED_JSON");

    host.send_raw(br#"{"v":1,"id":"bad-shape"}"#);
    assert_error(host.response(), "bad-shape", "INVALID_REQUEST");

    host.send_raw(br#"{"v":1,"id":"future-1","method":"future_command"}"#);
    assert_error(host.response(), "future-1", "UNKNOWN_METHOD");

    host.send_raw(br#"{"v":2,"id":"wrong-version","method":"health"}"#);
    assert_error(
        host.response(),
        "wrong-version",
        "PROTOCOL_VERSION_MISMATCH",
    );

    host.send_raw(br#"{"id":"missing-version","method":"health"}"#);
    assert_error(
        host.response(),
        "missing-version",
        "PROTOCOL_VERSION_MISMATCH",
    );

    host.send_raw(br#"{"v":1,"method":"health"}"#);
    assert_error(host.response(), UNKNOWN_REQUEST_ID, "INVALID_REQUEST");

    host.send_raw(br#"{"v":1,"id":"  ","method":"health"}"#);
    assert_error(host.response(), "  ", "INVALID_REQUEST");

    host.send(&HostRequest::new(
        "handshake-version",
        HostRequestMethod::Handshake {
            client: "integration-test".to_string(),
            client_protocol: PROTOCOL_VERSION + 1,
        },
    ));
    assert_error(
        host.response(),
        "handshake-version",
        "PROTOCOL_VERSION_MISMATCH",
    );

    host.send(&HostRequest::new(
        "handshake-1",
        HostRequestMethod::Handshake {
            client: "integration-test".to_string(),
            client_protocol: PROTOCOL_VERSION,
        },
    ));
    let response = host.response();
    assert_eq!(response.id, "handshake-1");
    match response.response {
        HostResponseStatus::Ok {
            result: HostResult::Hello(hello),
        } => {
            assert_eq!(hello.protocol, PROTOCOL_VERSION);
            assert_eq!(hello.host_version, env!("CARGO_PKG_VERSION"));
            assert_eq!(hello.engine_version, double_love_engine::ENGINE_VERSION);
            assert_eq!(
                hello.capabilities,
                CAPABILITIES
                    .iter()
                    .map(|capability| (*capability).to_string())
                    .collect::<Vec<_>>()
            );
        }
        other => panic!("expected hello response, got {other:?}"),
    }

    host.send(&HostRequest::new("health-1", HostRequestMethod::Health));
    assert_eq!(
        host.response(),
        HostResponse::ok("health-1", HostResult::Health { healthy: true })
    );

    host.send(&HostRequest::new("shutdown-1", HostRequestMethod::Shutdown));
    assert_eq!(
        host.response(),
        HostResponse::ok("shutdown-1", HostResult::Shutdown)
    );
    host.wait();
}

#[test]
fn spawned_host_rejects_oversized_frame_from_prefix_alone() {
    let mut host = HostProcess::spawn();
    host.stdin
        .write_all(&(MAX_FRAME_BYTES + 1).to_be_bytes())
        .expect("write oversized frame prefix");
    host.stdin.flush().expect("flush oversized frame prefix");

    assert_error(host.response(), UNKNOWN_REQUEST_ID, "FRAME_TOO_LARGE");
    host.wait();
}

#[test]
fn spawned_host_treats_eof_between_frames_as_clean_shutdown() {
    HostProcess::spawn().wait();
}

#[test]
fn spawned_host_exits_nonzero_on_partial_frame_io_failure() {
    let mut host = HostProcess::spawn();
    host.stdin
        .write_all(&2_u32.to_be_bytes())
        .expect("write frame prefix");
    host.stdin.write_all(b"{").expect("write partial body");
    drop(host.stdin);

    let status = host.child.wait().expect("wait for host");
    assert_ne!(status.code(), Some(0));

    let mut stdout = Vec::new();
    host.stdout
        .read_to_end(&mut stdout)
        .expect("read host stdout");
    assert!(stdout.is_empty(), "IO failure must not log to stdout");

    let mut stderr = String::new();
    host.child
        .stderr
        .take()
        .expect("host stderr")
        .read_to_string(&mut stderr)
        .expect("read host stderr");
    assert!(stderr.contains("input/output failed"));
}
