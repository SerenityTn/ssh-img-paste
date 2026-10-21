//! The main window: destination picker, upload and capture buttons, the
//! status card with the last remote path, and start-at-sign-in.

use super::app::App;
use super::{caption, dim_label, label};
use adw::prelude::*;
use gtk::glib;
use ssh_img_paste_linux::cli::Catalog;
use ssh_img_paste_linux::desktop::{APP_ID, DesktopAction, autostart_entry, autostart_path};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Idle,
    Busy,
    Success,
    Error,
}

impl StatusKind {
    fn icon(self) -> &'static str {
        match self {
            Self::Idle => "dialog-information-symbolic",
            Self::Busy => "emblem-synchronizing-symbolic",
            Self::Success => "emblem-ok-symbolic",
            Self::Error => "dialog-error-symbolic",
        }
    }
}

pub struct MainWindow {
    pub window: adw::ApplicationWindow,
    destination: adw::ComboRow,
    destination_ids: Rc<RefCell<Vec<String>>>,
    /// Set while the combo is repopulated, so it does not switch profiles.
    syncing: Rc<Cell<bool>>,
    upload: gtk::Button,
    region: gtk::Button,
    full: gtk::Button,
    uploads: gtk::Button,
    status_icon: gtk::Image,
    status: gtk::Label,
    path_row: gtk::Box,
    path: gtk::Entry,
    has_destination: Cell<bool>,
    busy: Cell<bool>,
    tray_note: gtk::Label,
}

impl MainWindow {
    pub fn new(app: &App) -> Self {
        let icon = gtk::Image::from_icon_name(APP_ID);
        icon.set_pixel_size(56);
        let title = label("SSH Image Paste");
        title.add_css_class("title-2");
        let subtitle =
            dim_label("Upload an image, then paste its remote path into your SSH session.");
        let titles = gtk::Box::new(gtk::Orientation::Vertical, 4);
        titles.set_valign(gtk::Align::Center);
        titles.append(&title);
        titles.append(&subtitle);
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        header.append(&icon);
        header.append(&titles);

        let destination = adw::ComboRow::builder().title("Uploads go to").build();
        let manage = gtk::Button::builder()
            .label("Manage…")
            .valign(gtk::Align::Center)
            .build();
        manage.add_css_class("flat");
        let group = adw::PreferencesGroup::builder()
            .title("Destination")
            .build();
        group.set_header_suffix(Some(&manage));
        group.add(&destination);

        let upload_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        upload_content.set_halign(gtk::Align::Center);
        upload_content.append(&gtk::Label::new(Some("Upload clipboard image")));
        let shortcut = gtk::Label::new(Some("Ctrl+V"));
        shortcut.add_css_class("dim-label");
        upload_content.append(&shortcut);
        let upload = gtk::Button::builder().child(&upload_content).build();
        upload.add_css_class("suggested-action");
        upload.add_css_class("pill");

        let region = gtk::Button::with_label("Capture region");
        let full = gtk::Button::with_label("Capture full screen");
        let captures = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        captures.set_homogeneous(true);
        captures.append(&region);
        captures.append(&full);
        let uploads = gtk::Button::with_label("Uploaded images…");

        let status_icon = gtk::Image::from_icon_name(StatusKind::Idle.icon());
        let status = label("Ready.");
        status.set_hexpand(true);
        let status_line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        status_line.append(&status_icon);
        status_line.append(&status);
        let path = gtk::Entry::builder().editable(false).hexpand(true).build();
        path.add_css_class("monospace");
        let copy = gtk::Button::with_label("Copy");
        let path_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        path_row.append(&path);
        path_row.append(&copy);
        path_row.set_visible(false);
        let card = gtk::Box::new(gtk::Orientation::Vertical, 10);
        card.add_css_class("card");
        for margin in [
            gtk::Box::set_margin_top,
            gtk::Box::set_margin_bottom,
            gtk::Box::set_margin_start,
            gtk::Box::set_margin_end,
        ] {
            margin(&status_line, 12);
        }
        path_row.set_margin_start(12);
        path_row.set_margin_end(12);
        path_row.set_margin_bottom(12);
        card.append(&status_line);
        card.append(&path_row);

        let sign_in = adw::SwitchRow::builder()
            .title("Start when I sign in")
            .build();
        let autostart = autostart_path(|name| std::env::var_os(name));
        sign_in.set_active(autostart.as_ref().is_some_and(|path| path.is_file()));
        sign_in.set_sensitive(autostart.is_some());
        let footer_group = adw::PreferencesGroup::new();
        footer_group.add(&sign_in);
        let tray_note = caption("");
        tray_note.set_visible(false);
        let version = caption(&format!("v{}", env!("CARGO_PKG_VERSION")));
        version.set_xalign(1.0);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
        content.set_margin_top(18);
        content.set_margin_bottom(18);
        content.set_margin_start(18);
        content.set_margin_end(18);
        content.append(&header);
        content.append(&group);
        content.append(&upload);
        content.append(&captures);
        content.append(&uploads);
        content.append(&card);
        content.append(&footer_group);
        content.append(&tray_note);
        content.append(&version);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&content));
        let window = adw::ApplicationWindow::builder()
            .application(&app.gtk)
            .title("SSH Image Paste")
            .icon_name(APP_ID)
            .default_width(500)
            .resizable(false)
            .content(&toolbar)
            .build();

        let this = Self {
            window,
            destination,
            destination_ids: Rc::default(),
            syncing: Rc::default(),
            upload,
            region,
            full,
            uploads,
            status_icon,
            status,
            path_row,
            path,
            has_destination: Cell::new(false),
            busy: Cell::new(false),
            tray_note,
        };
        this.connect(app, &manage, &copy, &sign_in);
        this.update_sensitivity();
        this
    }

    fn connect(
        &self,
        app: &App,
        manage: &gtk::Button,
        copy: &gtk::Button,
        sign_in: &adw::SwitchRow,
    ) {
        let weak = app.downgrade();
        let action = move |action: DesktopAction| {
            let weak = weak.clone();
            move |_: &gtk::Button| {
                if let Some(app) = weak.upgrade() {
                    app.perform(action);
                }
            }
        };
        self.upload
            .connect_clicked(action(DesktopAction::UploadClipboard));
        self.region
            .connect_clicked(action(DesktopAction::CaptureRegion));
        self.full
            .connect_clicked(action(DesktopAction::CaptureFullScreen));
        self.uploads
            .connect_clicked(action(DesktopAction::UploadedImages));
        manage.connect_clicked(action(DesktopAction::ManageProfiles));

        // Ctrl+V uploads; the window has no editable text field to steal it.
        let shortcuts = gtk::ShortcutController::new();
        let upload = self.upload.clone();
        shortcuts.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("<Control>v"),
            Some(gtk::CallbackAction::new(move |_, _| {
                if upload.is_sensitive() {
                    upload.emit_clicked();
                }
                glib::Propagation::Stop
            })),
        ));
        self.window.add_controller(shortcuts);

        let weak = app.downgrade();
        let (path, status) = (self.path.clone(), self.status.clone());
        copy.connect_clicked(move |_| {
            let Some(app) = weak.upgrade() else { return };
            let status = status.clone();
            app.copy_text(path.text().to_string(), move |copied| {
                status.set_label(if copied {
                    "Remote path copied again."
                } else {
                    "The clipboard is unavailable. Select the path and copy it."
                });
            });
        });

        let weak = app.downgrade();
        let ids = self.destination_ids.clone();
        let syncing = self.syncing.clone();
        self.destination.connect_selected_notify(move |row| {
            if syncing.get() {
                return;
            }
            let Some(id) = ids.borrow().get(row.selected() as usize).cloned() else {
                return;
            };
            if let Some(app) = weak.upgrade() {
                app.use_profile(&id);
            }
        });

        let status = self.status.clone();
        sign_in.connect_active_notify(move |row| {
            if let Err(error) = set_autostart(row.is_active()) {
                eprintln!("ssh-img-paste: autostart: {error}");
                status.set_label("Could not change the sign-in setting.");
            }
        });

        let weak = app.downgrade();
        self.window.connect_close_request(move |window| {
            match weak.upgrade() {
                // With a tray icon, closing hides to the tray.
                Some(app) if app.tray_available() => {
                    window.set_visible(false);
                    glib::Propagation::Stop
                }
                Some(app) => {
                    app.gtk.quit();
                    glib::Propagation::Proceed
                }
                None => glib::Propagation::Proceed,
            }
        });
    }

    pub fn catalog_changed(&self, catalog: Option<&Result<Catalog, String>>, busy: bool) {
        self.busy.set(busy);
        let usable: Vec<(String, String, String)> = match catalog {
            Some(Ok(catalog)) => catalog
                .usable()
                .map(|profile| {
                    (
                        profile.id.clone(),
                        profile.menu_label(),
                        profile.remote_root(),
                    )
                })
                .collect(),
            _ => Vec::new(),
        };
        let active = match catalog {
            Some(Ok(catalog)) => catalog.active().map(|profile| profile.id.clone()),
            _ => None,
        };
        self.syncing.set(true);
        let labels: Vec<&str> = usable.iter().map(|(_, label, _)| label.as_str()).collect();
        self.destination
            .set_model(Some(&gtk::StringList::new(&labels)));
        if let Some(index) = active
            .as_ref()
            .and_then(|active| usable.iter().position(|(id, _, _)| id == active))
        {
            self.destination.set_selected(index as u32);
            self.destination.set_subtitle(&usable[index].2);
        } else {
            self.destination.set_selected(gtk::INVALID_LIST_POSITION);
            self.destination.set_subtitle("");
        }
        *self.destination_ids.borrow_mut() = usable.into_iter().map(|(id, _, _)| id).collect();
        self.syncing.set(false);
        self.has_destination.set(active.is_some());
        match catalog {
            Some(Err(message)) => self.set_status(StatusKind::Error, message),
            Some(Ok(catalog)) if catalog.usable().next().is_none() && !busy => self.set_status(
                StatusKind::Idle,
                "No destination yet. Choose Manage… to add your SSH host.",
            ),
            _ => {}
        }
        self.update_sensitivity();
    }

    pub fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        self.update_sensitivity();
    }

    fn update_sensitivity(&self) {
        let ready = self.has_destination.get() && !self.busy.get();
        for button in [&self.upload, &self.region, &self.full] {
            button.set_sensitive(ready);
        }
        self.uploads.set_sensitive(self.has_destination.get());
        self.destination
            .set_sensitive(self.destination_ids.borrow().len() > 1 && !self.busy.get());
    }

    pub fn set_status(&self, kind: StatusKind, text: &str) {
        self.status_icon.set_icon_name(Some(kind.icon()));
        self.status.set_label(text);
        for class in ["success", "error"] {
            self.status_icon.remove_css_class(class);
        }
        match kind {
            StatusKind::Success => self.status_icon.add_css_class("success"),
            StatusKind::Error => self.status_icon.add_css_class("error"),
            _ => {}
        }
    }

    pub fn show_path(&self, path: &str) {
        self.path.set_text(path);
        self.path_row.set_visible(true);
    }

    pub fn tray_changed(&self, available: bool) {
        self.tray_note.set_visible(!available);
        self.tray_note.set_label(
            "The tray icon is unavailable. Closing this window quits SSH Image Paste. On GNOME, \
             install the AppIndicator extension for a tray icon.",
        );
    }
}

fn set_autostart(enabled: bool) -> std::io::Result<()> {
    let path = autostart_path(|name| std::env::var_os(name))
        .ok_or_else(|| std::io::Error::other("no config directory"))?;
    if enabled {
        let executable = std::env::current_exe()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, autostart_entry(&executable))
    } else {
        match std::fs::remove_file(&path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}
