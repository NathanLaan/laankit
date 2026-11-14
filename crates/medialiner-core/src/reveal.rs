//! Open in Folder: the system file manager at a file, with the file
//! selected where the platform allows.
//!
//! | Platform | How |
//! |----------|-----|
//! | Windows  | `explorer.exe /select,<path>` |
//! | Linux    | The `org.freedesktop.FileManager1.ShowItems` D-Bus call, through `gdbus` or `dbus-send` |
//! | Anything else, or no answer | The file's folder, through the `open` crate |

use std::path::Path;
use std::process::Command;

/// A command to run, as its program and arguments, so tests can check it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub program: String,
    pub args: Vec<String>,
}

/// `path` as a `file://` URI, with everything but unreserved characters
/// and `/` percent-encoded.
pub fn file_uri(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::from("file://");
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The D-Bus commands to try on Linux, in order.
pub fn dbus_plans(path: &Path) -> Vec<Plan> {
    let uri = file_uri(path);
    vec![
        Plan {
            program: "gdbus".into(),
            args: vec![
                "call".into(),
                "--session".into(),
                "--dest".into(),
                "org.freedesktop.FileManager1".into(),
                "--object-path".into(),
                "/org/freedesktop/FileManager1".into(),
                "--method".into(),
                "org.freedesktop.FileManager1.ShowItems".into(),
                format!("['{uri}']"),
                String::new(),
            ],
        },
        Plan {
            program: "dbus-send".into(),
            args: vec![
                "--session".into(),
                "--print-reply".into(),
                "--dest=org.freedesktop.FileManager1".into(),
                "/org/freedesktop/FileManager1".into(),
                "org.freedesktop.FileManager1.ShowItems".into(),
                format!("array:string:{uri}"),
                "string:".into(),
            ],
        },
    ]
}

/// Shows `path` in the file manager.
pub fn reveal(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // explorer wants `/select,"C:\a b\c.jpg"` as one raw argument.
        if Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    for plan in dbus_plans(path) {
        let ok = Command::new(&plan.program)
            .args(&plan.args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            return Ok(());
        }
    }
    #[cfg(target_os = "macos")]
    if Command::new("open").arg("-R").arg(path).spawn().is_ok() {
        return Ok(());
    }
    let folder = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    open::that_detached(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_and_commands() {
        let p = Path::new("/home/me/My Photos/été #1.jpg");
        assert_eq!(
            file_uri(p),
            "file:///home/me/My%20Photos/%C3%A9t%C3%A9%20%231.jpg"
        );
        let plans = dbus_plans(p);
        assert_eq!(plans[0].program, "gdbus");
        assert!(plans[0].args.contains(&format!("['{}']", file_uri(p))));
        assert_eq!(plans[1].args[5], format!("array:string:{}", file_uri(p)));
    }
}
