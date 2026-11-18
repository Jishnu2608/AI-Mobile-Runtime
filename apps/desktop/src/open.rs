//! AUD-P2-20 / AUD-P3-23: opens a source file at a line, or a log, for the person reading a
//! problem. Only existing text files with a known extension are opened, and never with the file
//! type's default handler (on Windows `.js` would run in Windows Script Host): VS Code when it is
//! installed (`code -g file:line`), otherwise Notepad.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Extensions the page may ask to open: source, build and config files, logs.
const TEXT: &[&str] = &[
    "kt",
    "kts",
    "java",
    "dart",
    "js",
    "jsx",
    "ts",
    "tsx",
    "mjs",
    "cjs",
    "json",
    "gradle",
    "xml",
    "yaml",
    "yml",
    "properties",
    "toml",
    "md",
    "txt",
    "log",
    "swift",
    "m",
    "h",
    "c",
    "cc",
    "cpp",
    "cmake",
];

/// Why `path` may not be opened, or `None` when it may.
#[must_use]
pub fn refusal(path: &Path) -> Option<String> {
    if !path.is_absolute() {
        return Some(format!("not a full path: {}", path.display()));
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !TEXT.contains(&ext.as_str()) {
        return Some(format!("not a text file the app opens: {}", path.display()));
    }
    if !path.is_file() {
        return Some(format!("the file does not exist: {}", path.display()));
    }
    None
}

/// Folders never searched for a source file: build output, caches, dependencies.
const SKIP: &[&str] = &[
    "build",
    ".gradle",
    "node_modules",
    ".dart_tool",
    ".git",
    "Pods",
    ".idea",
    ".cxx",
];

/// The file a problem names, inside the project `root`: a path relative to it as it is, else
/// (crash frames name only `MainActivity.kt`) the first file of that name below it.
///
/// # Errors
/// No such file in the project.
pub fn resolve(root: &Path, file: &str) -> Result<String, String> {
    let rel = Path::new(file);
    if rel.is_absolute() {
        return Ok(file.to_string());
    }
    let direct = root.join(rel);
    if direct.is_file() {
        return Ok(direct.display().to_string());
    }
    let name = rel
        .file_name()
        .ok_or_else(|| format!("no file name in {file}"))?;
    let mut stack = vec![root.to_path_buf()];
    let mut seen = 0usize;
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            seen += 1;
            if seen > 50_000 {
                break;
            }
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => {
                    let n = e.file_name();
                    if !SKIP.iter().any(|s| n.to_str() == Some(*s)) {
                        stack.push(p);
                    }
                }
                Ok(t) if t.is_file() && p.file_name() == Some(name) && p.ends_with(rel) => {
                    return Ok(p.display().to_string());
                }
                _ => {}
            }
        }
    }
    Err(format!("{file} is not in {}", root.display()))
}

/// `code.cmd` from PATH or the default per-user install folder.
fn vscode() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join("code.cmd"))
            .find(|f| f.is_file())
    });
    on_path.or_else(|| {
        let local = std::env::var_os("LOCALAPPDATA")?;
        let f = PathBuf::from(local).join(r"Programs\Microsoft VS Code\bin\code.cmd");
        f.is_file().then_some(f)
    })
}

/// Opens `path` (at `line` in VS Code). Returns which program opened it.
///
/// # Errors
/// The path is refused (see [`refusal`]) or no program could be started.
pub fn open_in_editor(path: &str, line: Option<u32>) -> Result<&'static str, String> {
    let p = PathBuf::from(path);
    if let Some(why) = refusal(&p) {
        return Err(why);
    }
    if let Some(code) = vscode() {
        let target = match line {
            Some(l) if l > 0 => format!("{}:{l}", p.display()),
            _ => p.display().to_string(),
        };
        let mut cmd = Command::new(code);
        cmd.arg("-g")
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flash
        }
        if cmd.spawn().is_ok() {
            return Ok("VS Code");
        }
    }
    Command::new("notepad.exe")
        .arg(&p)
        .spawn()
        .map(|_| "Notepad")
        .map_err(|e| format!("could not open {}: {e}", p.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_existing_text_files_are_opened() {
        let dir = std::env::temp_dir().join(format!("umr-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let kt = dir.join("Main.kt");
        std::fs::write(&kt, "fun main() {}").unwrap();
        assert_eq!(refusal(&kt), None);
        assert!(refusal(&dir.join("Missing.kt"))
            .unwrap()
            .contains("does not exist"));
        let exe = dir.join("tool.exe");
        std::fs::write(&exe, "MZ").unwrap();
        assert!(refusal(&exe).unwrap().contains("not a text file"));
        assert!(refusal(&dir.join("run.bat")).is_some());
        assert!(refusal(Path::new("src/Main.kt"))
            .unwrap()
            .contains("not a full path"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_bare_file_name_is_found_in_the_project() {
        let root = std::env::temp_dir().join(format!("umr-resolve-{}", std::process::id()));
        let src = root.join("app").join("src").join("main").join("kotlin");
        let built = root.join("app").join("build").join("tmp");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&built).unwrap();
        std::fs::write(built.join("MainActivity.kt"), "copy").unwrap();
        std::fs::write(src.join("MainActivity.kt"), "real").unwrap();
        let got = resolve(&root, "MainActivity.kt").unwrap();
        assert!(got.contains("kotlin") && !got.contains("build"), "{got}");
        let rel = resolve(&root, "app/src/main/kotlin/MainActivity.kt").unwrap();
        assert!(Path::new(&rel).is_file());
        assert!(resolve(&root, "Missing.kt").is_err());
        std::fs::remove_dir_all(&root).ok();
    }
}
