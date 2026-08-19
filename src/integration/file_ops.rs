use std::fs;
use std::io;
use std::path::Path;

pub(crate) fn remove_file_if_exists(path: &Path) -> io::Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

pub(crate) fn legacy_bash_hook_path(hook_path: &Path) -> std::path::PathBuf {
    let stem = hook_path
        .file_stem()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("innov-"))
        .unwrap_or("agent-state");
    hook_path.with_file_name(format!("herdr-{stem}.sh"))
}

pub(crate) fn remove_legacy_bash_hook_file(hook_path: &Path) -> io::Result<bool> {
    let stem = hook_path
        .file_stem()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("innov-"))
        .unwrap_or("agent-state");
    let legacy_paths = [
        hook_path.with_file_name(format!("herdr-{stem}.sh")),
        hook_path.with_file_name(format!("herdr-{stem}.ps1")),
    ];
    let mut removed = false;
    for legacy_path in legacy_paths {
        let content = match fs::read_to_string(&legacy_path) {
            Ok(content) => content,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        if content.contains("HERDR_INTEGRATION_ID=") {
            fs::remove_file(legacy_path)?;
            removed = true;
        }
    }
    Ok(removed)
}

pub(crate) fn remove_dir_all_if_exists(path: &Path) -> io::Result<bool> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

pub(crate) fn make_executable(_path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = fs::metadata(_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(_path, perms)?;
    }

    Ok(())
}
