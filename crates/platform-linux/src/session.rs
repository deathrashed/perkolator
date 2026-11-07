use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayServer {
    Wayland,
    X11,
    Unknown,
}

/// What the environment says about the graphical session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    pub display_server: DisplayServer,
    /// `XDG_CURRENT_DESKTOP`, e.g. `KDE`.
    pub desktop: Option<String>,
    pub wayland_display: Option<String>,
    pub runtime_dir: Option<PathBuf>,
}

impl SessionInfo {
    pub fn detect() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
    }

    /// Testable constructor over an arbitrary variable lookup.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let wayland_display = lookup("WAYLAND_DISPLAY");
        let session_type = lookup("XDG_SESSION_TYPE").map(|value| value.to_ascii_lowercase());
        let display_server =
            if wayland_display.is_some() || session_type.as_deref() == Some("wayland") {
                DisplayServer::Wayland
            } else if lookup("DISPLAY").is_some() || session_type.as_deref() == Some("x11") {
                DisplayServer::X11
            } else {
                DisplayServer::Unknown
            };
        Self {
            display_server,
            desktop: lookup("XDG_CURRENT_DESKTOP"),
            wayland_display,
            runtime_dir: lookup("XDG_RUNTIME_DIR").map(PathBuf::from),
        }
    }

    pub fn is_kde(&self) -> bool {
        self.desktop.as_deref().is_some_and(|desktop| {
            desktop
                .split(':')
                .any(|part| part.eq_ignore_ascii_case("kde"))
        })
    }
}

/// Per-user socket used for single-instance toggling. Lives in
/// `XDG_RUNTIME_DIR` (mode 0700, owned by the user) when available.
pub fn instance_socket_path(session: &SessionInfo) -> PathBuf {
    let dir = session
        .runtime_dir
        .clone()
        .unwrap_or_else(std::env::temp_dir);
    dir.join("perkolator.sock")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn session(vars: &[(&str, &str)]) -> SessionInfo {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        SessionInfo::from_lookup(|name| map.get(name).cloned())
    }

    #[test]
    fn detects_kde_wayland() {
        let info = session(&[
            ("WAYLAND_DISPLAY", "wayland-1"),
            ("XDG_CURRENT_DESKTOP", "KDE"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]);
        assert_eq!(info.display_server, DisplayServer::Wayland);
        assert!(info.is_kde());
        assert_eq!(
            instance_socket_path(&info),
            PathBuf::from("/run/user/1000/perkolator.sock")
        );
    }

    #[test]
    fn x11_and_unknown() {
        assert_eq!(
            session(&[("DISPLAY", ":0")]).display_server,
            DisplayServer::X11
        );
        assert_eq!(session(&[]).display_server, DisplayServer::Unknown);
        assert!(!session(&[("XDG_CURRENT_DESKTOP", "GNOME")]).is_kde());
    }
}
