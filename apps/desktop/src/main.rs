//! Perkolator desktop shell: a resident process hosting the quick launcher.
//!
//! `perkolator`            start (or reuse) the resident instance and show the launcher
//! `perkolator --toggle`   ask the running instance to show/hide the launcher
//! `perkolator --home DIR` index DIR as the home directory (e.g. the test fixture)

mod catalog;
mod cli;
mod ipc;
mod launcher;
mod marketplace;
mod platform;
mod theme_bridge;

use gpui_kit::{App, AppContext as _, QuitMode};

use crate::catalog::Catalog;
use crate::cli::{Cli, Command};
use crate::launcher::LauncherController;
use crate::marketplace::Marketplace;

fn main() {
    let cli = match Cli::parse(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("perkolator: {message}\n\n{}", cli::USAGE);
            std::process::exit(2);
        }
    };

    match &cli.command {
        Command::Help => {
            println!("{}", cli::USAGE);
            return;
        }
        Command::PrintDesktopEntry => {
            let exe = std::env::current_exe()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| "perkolator".into());
            print!("{}", platform::desktop_entry(&exe));
            return;
        }
        Command::Run | Command::Toggle => {}
    }

    let socket = platform::instance_socket_path();
    match ipc::try_forward(&socket, cli.ipc_message()) {
        Ok(ipc::Forwarded::Delivered) => return,
        Ok(ipc::Forwarded::NoInstance) => {}
        Err(error) => eprintln!("perkolator: single-instance check failed: {error}"),
    }
    let listener = match ipc::Listener::bind(&socket) {
        Ok(listener) => Some(listener),
        Err(error) => {
            eprintln!("perkolator: cannot listen on {}: {error}", socket.display());
            None
        }
    };

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            cx.set_quit_mode(QuitMode::Explicit);
            theme_bridge::apply(cx);
            launcher::bind_keys(cx);

            let catalog = cx.new(|cx| Catalog::load(cli.home.clone(), cx));
            let marketplace = cx.new(|_| Marketplace::new());
            let controller = cx.new(|_| LauncherController::new(catalog, marketplace));
            if let Some(listener) = listener {
                ipc::serve(listener, controller.clone(), cx);
            }
            controller.update(cx, |controller, cx| controller.show(cx));
        });
}
