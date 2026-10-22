//! GTK 4 / libadwaita desktop shell. Every CLI call and child process runs
//! on a worker thread; results come back to the main loop.

pub mod app;
pub mod dialogs;
pub mod editor;
pub mod main_window;
pub mod profiles;
pub mod tray;
pub mod uploads;

use gtk::prelude::*;
use gtk::{gio, glib};

/// Runs blocking `work` off the main thread, then `done` on it.
pub fn background<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T) + 'static,
) {
    glib::spawn_future_local(async move {
        match gio::spawn_blocking(work).await {
            Ok(value) => done(value),
            Err(_) => eprintln!("ssh-img-paste: a background task panicked"),
        }
    });
}

pub fn label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .build()
}

pub fn dim_label(text: &str) -> gtk::Label {
    let label = label(text);
    label.add_css_class("dim-label");
    label
}

pub fn caption(text: &str) -> gtk::Label {
    let label = dim_label(text);
    label.add_css_class("caption");
    label
}

pub fn heading(text: &str) -> gtk::Label {
    let label = label(text);
    label.add_css_class("heading");
    label
}

/// Removes every row from a list box (GTK 4.10 has no `remove_all`).
pub fn clear_list(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

/// Opens a local path or URI with the desktop's default handler.
pub fn open_uri(uri: &str) -> Result<(), String> {
    gio::AppInfo::launch_default_for_uri(uri, None::<&gio::AppLaunchContext>)
        .map_err(|error| error.to_string())
}

pub fn file_uri(path: &std::path::Path) -> String {
    gio::File::for_path(path).uri().to_string()
}
