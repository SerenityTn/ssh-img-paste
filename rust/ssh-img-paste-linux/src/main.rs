#![allow(clippy::result_large_err)] // CliError mirrors the protocol error object; it only travels on failure.

mod ui;

use adw::prelude::*;
use gtk::{gio, glib};
use ssh_img_paste_linux::desktop::{APP_ID, DesktopAction};
use ssh_img_paste_linux::{LaunchRequest, USAGE, parse_launch};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use ui::app::App;

type SharedApp = Rc<RefCell<Option<App>>>;

/// Builds the controller on first use. A background start keeps the window
/// hidden (tray only) unless no tray host appears.
fn ensure_started(gtk_app: &adw::Application, state: &SharedApp, background: bool) -> App {
    if let Some(app) = state.borrow().as_ref() {
        return app.clone();
    }
    let app = App::start(gtk_app, background);
    *state.borrow_mut() = Some(app.clone());
    app
}

fn main() -> glib::ExitCode {
    // Invalid options and --help are answered here, before any instance is
    // contacted, so the message reaches the terminal that asked.
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match parse_launch(&arguments) {
        Ok(LaunchRequest::Help) => {
            print!("{USAGE}");
            return glib::ExitCode::SUCCESS;
        }
        Err(message) => {
            eprint!("{message}\n\n{USAGE}");
            return glib::ExitCode::from(2);
        }
        Ok(_) => {}
    }

    let gtk_app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let state: SharedApp = Rc::new(RefCell::new(None));

    gtk_app.connect_activate({
        let state = state.clone();
        move |gtk_app| ensure_started(gtk_app, &state, false).show_main_window()
    });
    // Runs in the primary instance, also for invocations forwarded over D-Bus.
    gtk_app.connect_command_line({
        let state = state.clone();
        move |gtk_app, command_line| {
            let arguments: Vec<String> = command_line
                .arguments()
                .iter()
                .skip(1)
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect();
            let action =
                |action: DesktopAction| ensure_started(gtk_app, &state, true).perform(action);
            match parse_launch(&arguments) {
                Ok(LaunchRequest::Open | LaunchRequest::Help) => gtk_app.activate(),
                Ok(LaunchRequest::Background) => {
                    ensure_started(gtk_app, &state, true);
                }
                Ok(LaunchRequest::UploadClipboard) => action(DesktopAction::UploadClipboard),
                Ok(LaunchRequest::CaptureRegion) => action(DesktopAction::CaptureRegion),
                Ok(LaunchRequest::CaptureFullScreen) => action(DesktopAction::CaptureFullScreen),
                Ok(LaunchRequest::Quit) => gtk_app.quit(),
                Ok(LaunchRequest::SmokeTest) => {
                    gtk_app.activate();
                    let gtk_app = gtk_app.clone();
                    glib::timeout_add_local_once(Duration::from_millis(1_500), move || {
                        gtk_app.quit()
                    });
                }
                Err(_) => return glib::ExitCode::from(2),
            }
            glib::ExitCode::SUCCESS
        }
    });
    gtk_app.connect_shutdown({
        let state = state.clone();
        move |_| {
            if let Some(app) = state.borrow_mut().take() {
                app.shutdown();
            }
        }
    });

    gtk_app.run()
}
