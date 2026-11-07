use std::path::PathBuf;

pub const USAGE: &str = "\
Usage: perkolator [--toggle] [--home DIR] [--print-desktop-entry]

  --toggle               Show/hide the launcher of the running instance (start one if none)
  --home DIR             Index DIR as the home directory instead of $HOME
  --print-desktop-entry  Print a freedesktop .desktop entry with a Toggle action
  -h, --help             Show this help";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Run,
    Toggle,
    PrintDesktopEntry,
    Help,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cli {
    pub command: Command,
    pub home: Option<PathBuf>,
}

impl Cli {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut command = Command::Run;
        let mut home = None;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--toggle" => command = Command::Toggle,
                "--print-desktop-entry" => command = Command::PrintDesktopEntry,
                "-h" | "--help" => command = Command::Help,
                "--home" => {
                    let value = args.next().ok_or("--home needs a directory")?;
                    let path = PathBuf::from(&value);
                    let path = path
                        .canonicalize()
                        .map_err(|error| format!("--home {value}: {error}"))?;
                    home = Some(path);
                }
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        Ok(Self { command, home })
    }

    /// Message forwarded to an already running instance.
    pub fn ipc_message(&self) -> &'static str {
        match self.command {
            Command::Toggle => "toggle",
            _ => "show",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, String> {
        Cli::parse(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn parses_flags() {
        let home = std::env::temp_dir().canonicalize().unwrap();
        let cli = parse(&["--toggle", "--home", home.to_str().unwrap()]).unwrap();
        assert_eq!(cli.command, Command::Toggle);
        assert_eq!(cli.home, Some(home));
        assert_eq!(cli.ipc_message(), "toggle");
        assert_eq!(parse(&[]).unwrap().ipc_message(), "show");
    }

    #[test]
    fn rejects_unknown_and_incomplete() {
        assert!(parse(&["--bogus"]).is_err());
        assert!(parse(&["--home"]).is_err());
        assert!(parse(&["--home", "/definitely/not/here"]).is_err());
    }
}
