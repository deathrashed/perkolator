//! Thin per-OS seam for the desktop shell. Platform logic lives in the
//! platform crates; this module only selects them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use perkolator_core::{FeatureSupport, PlatformFeature};

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use perkolator_platform_linux::{DisplayServer, PortalStatus, SessionInfo};

    pub use perkolator_platform_linux::{APP_ID, desktop_entry};

    pub fn instance_socket_path() -> PathBuf {
        perkolator_platform_linux::instance_socket_path(&SessionInfo::detect())
    }

    pub fn features() -> BTreeMap<PlatformFeature, FeatureSupport> {
        perkolator_platform_linux::probe_features(&SessionInfo::detect(), &PortalStatus::NotProbed)
    }

    /// Use a layer-shell overlay for the launcher on Wayland unless disabled
    /// with `PERKOLATOR_LAYER_SHELL=0`.
    pub fn prefers_layer_shell() -> bool {
        SessionInfo::detect().display_server == DisplayServer::Wayland
            && std::env::var("PERKOLATOR_LAYER_SHELL").as_deref() != Ok("0")
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::*;

    pub const APP_ID: &str = "dev.perkolator.Perkolator";

    pub fn desktop_entry(_exec: &str) -> String {
        "Desktop entries are a Linux concept.\n".into()
    }

    pub fn instance_socket_path() -> PathBuf {
        std::env::temp_dir().join("perkolator.sock")
    }

    pub fn features() -> BTreeMap<PlatformFeature, FeatureSupport> {
        PlatformFeature::ALL
            .into_iter()
            .map(|feature| (feature, FeatureSupport::Unknown))
            .collect()
    }

    pub fn prefers_layer_shell() -> bool {
        false
    }
}

pub use imp::*;
