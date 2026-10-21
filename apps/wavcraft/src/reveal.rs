//! Reveal a file in the system file manager.
//! Windows: explorer /select,C:\path\to\file
//! Mac: open -R /path/to/file
//! Linux: xdg-open /path/to/parent/folder
//!
//! This module provides functionality to reveal files in the system's default file manager.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Linux,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevealCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// Returns the command to reveal a file in the system file manager.
pub fn reveal_command(path: &std::path::Path, os: Os) -> RevealCommand {
    match os {
        Os::Windows => {
            let args = vec![format!("/select,{}", path.to_string_lossy())];
            RevealCommand { program: "explorer".to_string(), args }
        }
        Os::Mac => {
            let args = vec!["-R".to_string(), path.to_string_lossy().into_owned()];
            RevealCommand { program: "open".to_string(), args }
        }
        Os::Linux => {
            let parent = path.parent().unwrap_or(path);
            let parent_str = if parent.as_os_str().is_empty() { ".".to_string() } else { parent.to_string_lossy().into_owned() };
            let args = vec![parent_str];
            RevealCommand { program: "xdg-open".to_string(), args }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(program: &str, args: &[&str]) -> RevealCommand {
        RevealCommand { program: program.to_string(), args: args.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn windows_plain_path() {
        assert_eq!(reveal_command(std::path::Path::new(r"C:\Audio\take1.wav"), Os::Windows), cmd("explorer", &["/select,C:\\Audio\\take1.wav"]));
    }

    #[test]
    fn windows_path_with_spaces() {
        assert_eq!(
            reveal_command(std::path::Path::new(r"D:\My Recordings\take 2.wav"), Os::Windows),
            cmd("explorer", &["/select,D:\\My Recordings\\take 2.wav"])
        );
    }

    #[test]
    fn windows_non_ascii_path() {
        assert_eq!(
            reveal_command(std::path::Path::new(r"C:\Users\Zoë\Musik\Größe – Æon.wav"), Os::Windows),
            cmd("explorer", &["/select,C:\\Users\\Zoë\\Musik\\Größe – Æon.wav"])
        );
    }

    #[test]
    fn macos_plain_path() {
        assert_eq!(reveal_command(std::path::Path::new("/Users/test/take1.wav"), Os::Mac), cmd("open", &["-R", "/Users/test/take1.wav"]));
    }

    #[test]
    fn macos_path_with_spaces() {
        assert_eq!(
            reveal_command(std::path::Path::new("/Users/test/My Recordings/take 2.wav"), Os::Mac),
            cmd("open", &["-R", "/Users/test/My Recordings/take 2.wav"])
        );
    }

    #[test]
    fn macos_non_ascii_path() {
        assert_eq!(
            reveal_command(std::path::Path::new("/Users/test/Música/Größe – Æon.wav"), Os::Mac),
            cmd("open", &["-R", "/Users/test/Música/Größe – Æon.wav"])
        );
    }

    #[test]
    fn linux_plain_path() {
        assert_eq!(reveal_command(std::path::Path::new("/home/test/take1.wav"), Os::Linux), cmd("xdg-open", &["/home/test"]));
    }

    #[test]
    fn linux_path_with_spaces() {
        assert_eq!(
            reveal_command(std::path::Path::new("/home/test/My Recordings/take 2.wav"), Os::Linux),
            cmd("xdg-open", &["/home/test/My Recordings"])
        );
    }

    #[test]
    fn linux_non_ascii_path() {
        assert_eq!(reveal_command(std::path::Path::new("/home/test/Música/Größe – Æon.wav"), Os::Linux), cmd("xdg-open", &["/home/test/Música"]));
    }

    #[test]
    fn linux_file_in_root() {
        assert_eq!(reveal_command(std::path::Path::new("/take1.wav"), Os::Linux), cmd("xdg-open", &["/"]));
    }

    #[test]
    fn linux_relative_file_name() {
        assert_eq!(reveal_command(std::path::Path::new("take1.wav"), Os::Linux), cmd("xdg-open", &["."]));
    }

    #[test]
    fn linux_nested_non_ascii_folder() {
        assert_eq!(
            reveal_command(std::path::Path::new("/srv/Ünïcode dir/sub folder/x.wav"), Os::Linux),
            cmd("xdg-open", &["/srv/Ünïcode dir/sub folder"])
        );
    }
}
