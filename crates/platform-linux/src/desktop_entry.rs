/// Stable application id used for the Wayland `app_id`, the desktop entry
/// file name and xdg-activation. Provisional; see docs/adr/0002.
pub const APP_ID: &str = "dev.perkolator.Perkolator";

/// Command a desktop shortcut runs to show/hide the launcher of the running
/// instance (or start one).
pub fn toggle_command() -> &'static str {
    "perkolator --toggle"
}

/// Freedesktop desktop entry content. The `Toggle` action lets KDE's
/// Shortcuts settings bind a global key without the portal.
pub fn desktop_entry(exec: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Perkolator\n\
         Comment=Search, preview and deploy AI resources\n\
         Exec={exec}\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=Development;Utility;\n\
         StartupWMClass={APP_ID}\n\
         Actions=Toggle;\n\
         \n\
         [Desktop Action Toggle]\n\
         Name=Toggle Quick Launcher\n\
         Exec={exec} --toggle\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_has_toggle_action_and_app_id() {
        let entry = desktop_entry("/usr/bin/perkolator");
        assert!(entry.contains("StartupWMClass=dev.perkolator.Perkolator"));
        assert!(entry.contains("[Desktop Action Toggle]"));
        assert!(entry.contains("Exec=/usr/bin/perkolator --toggle"));
    }
}
