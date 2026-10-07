use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

use serde_json::{json, Value};

// ---- Python sidecar: one JSON object per line over stdin/stdout ----

pub(crate) struct Sidecar {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    _child: Child, // exits on its own when our end of stdin closes
}

#[derive(Default)]
pub(crate) struct Backend(Mutex<Option<Sidecar>>);

pub(crate) fn spawn_sidecar() -> std::io::Result<Sidecar> {
    // ponytail: dev-only paths into the source tree. For a distributable build,
    // freeze backend/ with PyInstaller and ship it via tauri.conf.json `bundle.externalBin`.
    // Run as a module from the project root so `from backend.x import ...` resolves.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut child = Command::new(root.join("backend/.venv/bin/python"))
        .args(["-m", "backend.main"])
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    Ok(Sidecar {
        stdin: child.stdin.take().unwrap(),
        stdout: BufReader::new(child.stdout.take().unwrap()),
        _child: child,
    })
}

/// One request and its reply line. An error here means the process or the pipe is broken.
fn exchange(sidecar: &mut Sidecar, method: &str, params: Option<Value>) -> Result<Value, String> {
    writeln!(sidecar.stdin, "{}", json!({ "method": method, "params": params }))
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    sidecar.stdout.read_line(&mut line).map_err(|e| e.to_string())?;
    serde_json::from_str(&line).map_err(|e| format!("bad sidecar reply {line:?}: {e}"))
}

/// A reply's `ok`, or the error the method raised.
fn answer(mut res: Value) -> Result<Value, String> {
    match res.get("error") {
        Some(err) => Err(err.to_string()),
        None => Ok(res["ok"].take()),
    }
}

#[cfg(test)]
pub(crate) fn call(sidecar: &mut Sidecar, method: &str, params: Option<Value>) -> Result<Value, String> {
    answer(exchange(sidecar, method, params)?)
}

// ponytail: one sidecar behind one lock, so a long job (graph building) would stall the
// timer tick. Give long jobs their own process when Phase 5 adds them.
pub(crate) fn sidecar_call(backend: &Backend, method: &str, params: Option<Value>) -> Result<Value, String> {
    // after a panic mid-call the pipe may hold half a reply, so that process is not used again
    let mut guard = backend.0.lock().unwrap_or_else(|poisoned| {
        let mut guard = poisoned.into_inner();
        *guard = None;
        guard
    });
    if guard.is_none() {
        *guard = Some(spawn_sidecar().map_err(|e| format!("can't start python backend: {e}"))?);
    }
    let res = exchange(guard.as_mut().unwrap(), method, params);
    if res.is_err() {
        // respawn on next call in case the process died. Not for an error a method raised:
        // the timer lives in that process, and a block in progress would go with it.
        *guard = None;
    }
    answer(res?)
}
