#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RunningCommand {
    pub(crate) executable: String,
    is_windows_path: bool,
}

pub(crate) fn parse_running_command(running_command: &str) -> RunningCommand {
    let command = running_command.trim();

    let (executable, is_windows_path) = if let Some(quoted) = command.strip_prefix('"') {
        let end = quoted.find('"').unwrap_or(quoted.len());
        let executable = &quoted[..end];
        (executable, looks_like_windows_executable(executable))
    } else if looks_like_windows_executable(command) {
        let end = windows_executable_end(command)
            .unwrap_or_else(|| command.find(char::is_whitespace).unwrap_or(command.len()));
        (&command[..end], true)
    } else {
        (command.split_whitespace().next().unwrap_or(""), false)
    };

    let basename = if is_windows_path {
        executable
            .rsplit(|character| character == '/' || character == '\\')
            .next()
            .unwrap_or("")
    } else {
        executable.rsplit('/').next().unwrap_or("")
    };

    let executable = if is_windows_path {
        strip_exe_suffix(basename).to_ascii_lowercase()
    } else {
        basename.to_string()
    };

    RunningCommand {
        executable,
        is_windows_path,
    }
}

pub(crate) fn is_trigger_command(
    running_command: &str,
    parsed_command: &RunningCommand,
    triggers: &[String],
) -> bool {
    triggers.iter().any(|trigger| trigger == running_command)
        || (!parsed_command.executable.is_empty()
            && triggers
                .iter()
                .filter(|trigger| is_bare_name(trigger))
                .any(|trigger| {
                    if parsed_command.is_windows_path {
                        strip_exe_suffix(trigger).eq_ignore_ascii_case(&parsed_command.executable)
                    } else {
                        trigger == &parsed_command.executable
                    }
                }))
}

fn looks_like_windows_executable(value: &str) -> bool {
    let bytes = value.as_bytes();
    let has_drive_prefix = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    if has_drive_prefix || value.starts_with(r"\\") {
        return true;
    }

    let executable_token = value.split_whitespace().next().unwrap_or("");
    let is_unix_path = executable_token.starts_with('/') || executable_token.contains('/');
    let has_windows_separator = executable_token.contains('\\');
    let has_exe_suffix = strip_exe_suffix(executable_token) != executable_token;

    has_exe_suffix && (!is_unix_path || has_windows_separator)
}

fn windows_executable_end(command: &str) -> Option<usize> {
    let lowercase = command.to_ascii_lowercase();

    lowercase.match_indices(".exe").find_map(|(start, suffix)| {
        let end = start + suffix.len();
        let is_boundary = command[end..]
            .chars()
            .next()
            .map_or(true, |character| character.is_whitespace());
        is_boundary.then_some(end)
    })
}

fn strip_exe_suffix(value: &str) -> &str {
    let suffix_start = value.len().saturating_sub(4);

    if value
        .get(suffix_start..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".exe"))
    {
        &value[..suffix_start]
    } else {
        value
    }
}

fn is_bare_name(trigger: &str) -> bool {
    !trigger.is_empty()
        && !trigger
            .chars()
            .any(|character| character.is_whitespace() || matches!(character, '/' | '\\' | '"'))
}

#[cfg(test)]
mod tests {
    use super::{is_trigger_command, parse_running_command, RunningCommand};

    fn triggers(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn extracts_unix_executables_without_changing_case() {
        assert_eq!(
            parse_running_command("/usr/bin/vim README.md"),
            RunningCommand {
                executable: "vim".to_string(),
                is_windows_path: false,
            }
        );
        assert_eq!(
            parse_running_command(r#""/opt/Vim Builds/VIM" README.md"#).executable,
            "VIM"
        );
        assert_eq!(parse_running_command("VIM").executable, "VIM");
    }

    #[test]
    fn extracts_windows_executables_with_spaces_and_arguments() {
        assert_eq!(
            parse_running_command(r"C:\Program Files\Vim\vim92\vim.EXE -Nu NONE README.md")
                .executable,
            "vim"
        );
        assert_eq!(
            parse_running_command(r#""C:\Tools\Neovim\bin\NVIM.exe" README.md"#).executable,
            "nvim"
        );
        assert_eq!(
            parse_running_command(r"C:\Tools.exe-dir\Vim\vim.EXE README.md").executable,
            "vim"
        );
        assert_eq!(
            parse_running_command(r"\\server\tools\VIM.EXE README.md").executable,
            "vim"
        );
        assert_eq!(parse_running_command("VIM.EXE README.md").executable, "vim");
        assert_eq!(
            parse_running_command(r".\VIM.exe README.md").executable,
            "vim"
        );
        assert_eq!(
            parse_running_command(r"bin\VIM.exe README.md").executable,
            "vim"
        );
    }

    #[test]
    fn does_not_apply_windows_exe_scanning_to_unix_commands() {
        assert_eq!(parse_running_command("vim README.exe").executable, "vim");
        assert_eq!(
            parse_running_command(r"wine C:\Games\doom.exe").executable,
            "wine"
        );
        assert_eq!(
            parse_running_command("/tmp/VIM.exe README.md").executable,
            "VIM.exe"
        );
        assert_eq!(
            parse_running_command("./VIM.exe README.md").executable,
            "VIM.exe"
        );
    }

    #[test]
    fn matches_windows_bare_names_case_insensitively() {
        let parsed =
            parse_running_command(r"C:\Program Files\Vim\vim92\vim.EXE -Nu NONE README.md");

        assert!(is_trigger_command(
            r"C:\Program Files\Vim\vim92\vim.EXE -Nu NONE README.md",
            &parsed,
            &triggers(&["VIM.exe"]),
        ));
    }

    #[test]
    fn preserves_case_sensitive_unix_matching() {
        let parsed = parse_running_command("VIM README.md");

        assert!(!is_trigger_command(
            "VIM README.md",
            &parsed,
            &triggers(&["vim"]),
        ));
        assert!(is_trigger_command(
            "VIM README.md",
            &parsed,
            &triggers(&["VIM"]),
        ));
    }

    #[test]
    fn keeps_qualified_triggers_exact() {
        let parsed = parse_running_command("/tmp/vim README.md");
        assert!(!is_trigger_command(
            "/tmp/vim README.md",
            &parsed,
            &triggers(&["/usr/bin/vim"]),
        ));

        let parsed = parse_running_command("vim other.txt");
        assert!(!is_trigger_command(
            "vim other.txt",
            &parsed,
            &triggers(&["vim --clean"]),
        ));

        let parsed = parse_running_command("vim --clean");
        assert!(is_trigger_command(
            "vim --clean",
            &parsed,
            &triggers(&["vim --clean"]),
        ));
    }

    #[test]
    fn does_not_match_unrelated_windows_executables() {
        let command = r"C:\Program Files\PowerShell\7\pwsh.EXE";
        let parsed = parse_running_command(command);

        assert!(!is_trigger_command(
            command,
            &parsed,
            &triggers(&["vim", "nvim.exe"]),
        ));
    }
}
