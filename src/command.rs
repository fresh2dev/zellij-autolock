pub(crate) fn running_command_executable(running_command: &str) -> String {
    let command = running_command.trim();
    let lowercase = command.to_ascii_lowercase();

    // Native Windows reports an unquoted full executable path followed by
    // arguments, even when the path contains spaces. The .exe suffix is the
    // only reliable boundary in that representation.
    let executable = if command.starts_with('"') {
        let end = command[1..]
            .find('"')
            .map(|index| index + 1)
            .unwrap_or(command.len());
        &lowercase[1..end]
    } else if let Some(end) = lowercase.match_indices(".exe").find_map(|(start, suffix)| {
        let end = start + suffix.len();
        let is_boundary = lowercase[end..]
            .chars()
            .next()
            .map_or(true, |character| character.is_whitespace());
        is_boundary.then_some(end)
    }) {
        &lowercase[..end]
    } else {
        lowercase.split_whitespace().next().unwrap_or("")
    };

    let basename = executable
        .rsplit(|character| character == '/' || character == '\\')
        .next()
        .unwrap_or("");

    basename
        .strip_suffix(".exe")
        .unwrap_or(basename)
        .to_string()
}

pub(crate) fn is_trigger_command(running_command: &str, triggers: &[String]) -> bool {
    let executable = running_command_executable(running_command);

    triggers.iter().any(|trigger| {
        trigger.eq_ignore_ascii_case(running_command)
            || (!executable.is_empty() && running_command_executable(trigger) == executable)
    })
}

#[cfg(test)]
mod tests {
    use super::{is_trigger_command, running_command_executable};

    #[test]
    fn extracts_unix_executables() {
        assert_eq!(running_command_executable("/usr/bin/vim README.md"), "vim");
        assert_eq!(
            running_command_executable(r#""/opt/Vim Builds/vim" README.md"#),
            "vim"
        );
        assert_eq!(running_command_executable("nvim"), "nvim");
    }

    #[test]
    fn extracts_windows_executables_with_spaces_and_arguments() {
        assert_eq!(
            running_command_executable(r"C:\Program Files\Vim\vim92\vim.EXE -Nu NONE README.md"),
            "vim"
        );
        assert_eq!(
            running_command_executable(r#""C:\Tools\Neovim\bin\nvim.exe" README.md"#),
            "nvim"
        );
        assert_eq!(
            running_command_executable(r"C:\Tools.exe-dir\Vim\vim.EXE README.md"),
            "vim"
        );
    }

    #[test]
    fn matches_normalized_executables_against_triggers() {
        let triggers = vec!["vim".to_string(), "nvim.exe".to_string()];

        assert!(is_trigger_command(
            r"C:\Program Files\Vim\vim92\vim.EXE README.md",
            &triggers
        ));
        assert!(is_trigger_command(
            r#""C:\Tools\Neovim\bin\NVIM.exe" README.md"#,
            &triggers
        ));
        assert!(!is_trigger_command(
            r"C:\Program Files\PowerShell\7\pwsh.EXE",
            &triggers
        ));
    }
}
