use std::io::Write;
use std::process::{Command, Stdio};

/// Secrets (the Google refresh token, the feed addresses) live in the macOS Keychain, reached through the
/// `security` tool that ships with macOS. `service` is the app identifier, so the dev and everyday apps
/// keep their own; `account` names the secret.
/// Our app is unsigned, so a keychain library would ask for the login password after every rebuild.
/// ponytail: any program running as the user can ask `security` for a secret; sign the app and use a
/// keychain library to tie them to this app alone.
fn keychain(args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("security").args(args).output().map_err(|_| "could not reach the Keychain".to_string())
}

pub(crate) fn keychain_get(service: &str, account: &str) -> Result<Option<String>, String> {
    let out = keychain(&["find-generic-password", "-s", service, "-a", account, "-w"])?;
    match out.status.code() {
        Some(0) => Ok(Some(String::from_utf8_lossy(&out.stdout).trim().to_string())),
        Some(44) => Ok(None), // errSecItemNotFound
        _ => Err("could not reach the Keychain".into()),
    }
}

pub(crate) fn keychain_set(service: &str, account: &str, secret: &str) -> Result<(), String> {
    // the names go into a command line for `security -i`, so nothing that could end or split it gets through;
    // the secret goes in as hex, so it can hold anything (an address has ? and & in it)
    let plain = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    if !plain(service) || !plain(account) || secret.is_empty() {
        return Err("could not reach the Keychain".into());
    }
    let hex: String = secret.bytes().map(|b| format!("{b:02x}")).collect();
    // on stdin, so the secret never shows in `ps`
    let mut child = Command::new("security")
        .arg("-i")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "could not reach the Keychain".to_string())?;
    let line = format!("add-generic-password -U -s {service} -a {account} -X {hex}\n");
    child.stdin.take().unwrap().write_all(line.as_bytes()).map_err(|_| "could not reach the Keychain".to_string())?;
    child.wait().map_err(|_| "could not reach the Keychain".to_string())?;
    // `security -i` exits 0 even when a command in it fails, so read the secret back
    if keychain_get(service, account)?.as_deref() == Some(secret) {
        Ok(())
    } else {
        Err("could not reach the Keychain".into())
    }
}

pub(crate) fn keychain_delete(service: &str, account: &str) {
    let _ = keychain(&["delete-generic-password", "-s", service, "-a", account]);
}
