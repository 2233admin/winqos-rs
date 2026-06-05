use std::env;
use std::path::{Path, PathBuf};

pub fn powershell_path() -> PathBuf {
    resolve_windows_system_executable("WindowsPowerShell/v1.0", "powershell.exe")
}

pub fn default_router_ssh_path() -> PathBuf {
    resolve_windows_system_executable("OpenSSH", "ssh.exe")
}

pub fn normalize_router_ssh_path(raw: PathBuf) -> PathBuf {
    let name_is_ssh = matches_file_name(&raw, "ssh.exe");
    if raw.is_absolute() && name_is_ssh {
        return raw;
    }
    if name_is_ssh {
        return default_router_ssh_path();
    }
    default_router_ssh_path()
}

fn resolve_windows_system_executable(system_subdir: &str, filename: &str) -> PathBuf {
    if let Ok(system_root) = env::var("SystemRoot") {
        let candidate = Path::new(&system_root)
            .join("System32")
            .join(system_subdir)
            .join(filename);
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from(filename)
}

fn matches_file_name(path: &Path, expected: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(expected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_relative_ssh_path() {
        assert_eq!(
            normalize_router_ssh_path(PathBuf::from("ssh.exe")),
            default_router_ssh_path()
        );
    }

    #[test]
    fn rejects_non_ssh_relative_path() {
        assert_eq!(
            normalize_router_ssh_path(PathBuf::from("relative\\evil.exe")),
            default_router_ssh_path()
        );
    }

    #[test]
    fn preserves_absolute_ssh_path() -> Result<(), &'static str> {
        let mut cwd = std::env::current_dir().map_err(|_| "failed to read current dir")?;
        cwd.push("ssh.exe");
        let normalized = normalize_router_ssh_path(cwd.clone());
        assert_eq!(normalized, cwd);
        Ok(())
    }

    #[test]
    fn resolves_windows_power_shell_path() {
        assert!(!powershell_path().as_os_str().is_empty());
    }
}
