//! "Uploaded Images": list a destination's uploads, open one (fetch, then the
//! default image viewer), copy its remote path, or delete them all.

use super::app::{App, WeakApp};
use super::{caption, clear_list, dialogs, file_uri, label, open_uri};
use adw::prelude::*;
use gtk::glib;
use ssh_img_paste_linux::cli::{self, Catalog, Profile, RemoteUpload, UploadListing};
use ssh_img_paste_linux::profile_rules::{format_size, plural};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

pub struct Inner {
    pub window: adw::Window,
    app: WeakApp,
    destination: gtk::DropDown,
    destinations: RefCell<Vec<Profile>>,
    chosen: RefCell<Option<String>>,
    filling: Cell<bool>,
    list: gtk::ListBox,
    uploads: RefCell<Vec<RemoteUpload>>,
    generation: Cell<u64>,
    busy: Cell<bool>,
    status: gtk::Label,
    refresh: gtk::Button,
    open: gtk::Button,
    copy: gtk::Button,
    delete_all: gtk::Button,
}

#[derive(Clone)]
pub struct UploadsWindow(Rc<Inner>);

impl std::ops::Deref for UploadsWindow {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl UploadsWindow {
    pub fn new(app: &App) -> Self {
        let destination = gtk::DropDown::from_strings(&[]);
        destination.set_hexpand(true);
        let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
        refresh.set_tooltip_text(Some("Refresh"));
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        top.append(&label("Destination"));
        top.append(&destination);
        top.append(&refresh);

        let header = column_row("Name", "Size", "Uploaded");
        header.add_css_class("heading");
        let list = gtk::ListBox::new();
        // A double click (or Enter) opens the row, as on Windows; one click selects.
        list.set_activate_on_single_click(false);
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("boxed-list");
        let scroller = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        let status = caption("");
        let open = gtk::Button::with_label("Open");
        open.add_css_class("suggested-action");
        let copy = gtk::Button::with_label("Copy remote path");
        let delete_all = gtk::Button::with_label("Delete all uploads…");
        delete_all.add_css_class("destructive-action");
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bottom.append(&open);
        bottom.append(&copy);
        bottom.append(&spacer);
        bottom.append(&delete_all);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);
        content.append(&top);
        content.append(&header);
        content.append(&scroller);
        content.append(&status);
        content.append(&bottom);
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&content));
        let window = adw::Window::builder()
            .title("Uploaded Images — SSH Image Paste")
            .default_width(760)
            .default_height(520)
            .content(&toolbar)
            .build();
        window.set_size_request(560, 360);

        let this = Self(Rc::new(Inner {
            window,
            app: app.downgrade(),
            destination,
            destinations: RefCell::new(Vec::new()),
            chosen: RefCell::new(None),
            filling: Cell::new(false),
            list,
            uploads: RefCell::new(Vec::new()),
            generation: Cell::new(0),
            busy: Cell::new(false),
            status,
            refresh,
            open,
            copy,
            delete_all,
        }));
        this.connect();
        this.catalog_changed(app.catalog().as_ref());
        this
    }

    fn weak(&self) -> Weak<Inner> {
        Rc::downgrade(&self.0)
    }

    fn connect(&self) {
        let handler = |f: fn(&UploadsWindow)| {
            let weak = self.weak();
            move |_: &gtk::Button| {
                if let Some(inner) = weak.upgrade() {
                    f(&UploadsWindow(inner));
                }
            }
        };
        self.refresh.connect_clicked(handler(UploadsWindow::reload));
        self.open
            .connect_clicked(handler(UploadsWindow::open_selected));
        self.copy
            .connect_clicked(handler(UploadsWindow::copy_selected));
        self.delete_all
            .connect_clicked(handler(UploadsWindow::delete_all));
        let weak = self.weak();
        self.destination.connect_selected_notify(move |dropdown| {
            let Some(inner) = weak.upgrade() else { return };
            if inner.filling.get() {
                return;
            }
            let id = inner
                .destinations
                .borrow()
                .get(dropdown.selected() as usize)
                .map(|profile| profile.id.clone());
            *inner.chosen.borrow_mut() = id;
            UploadsWindow(inner).reload();
        });
        let weak = self.weak();
        self.list.connect_row_selected(move |_, _| {
            if let Some(inner) = weak.upgrade() {
                UploadsWindow(inner).update_buttons();
            }
        });
        let weak = self.weak();
        self.list.connect_row_activated(move |_, _| {
            if let Some(inner) = weak.upgrade() {
                UploadsWindow(inner).open_selected();
            }
        });
    }

    /// Keeps the chosen destination, else the active one, else the first.
    pub fn catalog_changed(&self, catalog: Option<&Result<Catalog, String>>) {
        let Some(Ok(catalog)) = catalog else { return };
        let usable: Vec<Profile> = catalog.usable().cloned().collect();
        let chosen = self
            .chosen
            .borrow()
            .clone()
            .filter(|id| usable.iter().any(|profile| &profile.id == id))
            .or_else(|| catalog.active().map(|profile| profile.id.clone()))
            .or_else(|| usable.first().map(|profile| profile.id.clone()));
        let labels: Vec<String> = usable
            .iter()
            .map(|profile| format!("{}  —  {}", profile.label, profile.remote_root()))
            .collect();
        let changed = *self.destinations.borrow() != usable || *self.chosen.borrow() != chosen;
        self.filling.set(true);
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        self.destination
            .set_model(Some(&gtk::StringList::new(&labels)));
        let index = chosen
            .as_ref()
            .and_then(|id| usable.iter().position(|profile| &profile.id == id));
        self.destination
            .set_selected(index.map_or(gtk::INVALID_LIST_POSITION, |index| index as u32));
        *self.destinations.borrow_mut() = usable;
        *self.chosen.borrow_mut() = chosen;
        self.filling.set(false);
        if changed {
            self.reload();
        }
        self.update_buttons();
    }

    fn chosen_profile(&self) -> Option<Profile> {
        let id = self.chosen.borrow().clone()?;
        self.destinations
            .borrow()
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
    }

    fn selected_upload(&self) -> Option<RemoteUpload> {
        let row = self.list.selected_row()?;
        self.uploads.borrow().get(row.index() as usize).cloned()
    }

    fn update_buttons(&self) {
        let busy = self.busy.get();
        let has_destination = self.chosen_profile().is_some();
        let selected = self.selected_upload().is_some();
        self.refresh.set_sensitive(has_destination);
        self.open.set_sensitive(selected && !busy);
        self.copy.set_sensitive(selected);
        self.delete_all
            .set_sensitive(has_destination && !busy && !self.uploads.borrow().is_empty());
    }

    /// Lists the chosen destination's uploads; a newer load wins.
    pub fn reload(&self) {
        let Some(profile) = self.chosen_profile() else {
            self.show_listing(None);
            self.status
                .set_label("Add a destination to see its uploads.");
            return;
        };
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.status
            .set_label(&format!("Loading uploads from {}…", profile.label));
        let weak = self.weak();
        let id = profile.id.clone();
        app.run_cli(
            move |cli| cli::list_uploads(cli, &id),
            move |result| {
                let Some(inner) = weak.upgrade() else { return };
                if inner.generation.get() != generation {
                    return;
                }
                let this = UploadsWindow(inner);
                match result {
                    Ok(listing) => {
                        this.status.set_label(&listing_summary(&listing));
                        this.show_listing(Some(listing));
                    }
                    Err(message) => {
                        this.show_listing(None);
                        this.status.set_label(&message);
                    }
                }
            },
        );
    }

    fn show_listing(&self, listing: Option<UploadListing>) {
        clear_list(&self.list);
        let uploads = listing.map(|listing| listing.uploads).unwrap_or_default();
        for upload in &uploads {
            let uploaded = glib::DateTime::from_unix_local(upload.modified_epoch_seconds)
                .and_then(|time| time.format("%x %H:%M"))
                .map(|text| text.to_string())
                .unwrap_or_default();
            let row = column_row(&upload.name, &format_size(upload.size_bytes), &uploaded);
            row.set_margin_top(6);
            row.set_margin_bottom(6);
            self.list.append(&row);
        }
        *self.uploads.borrow_mut() = uploads;
        self.update_buttons();
    }

    fn run<T: Send + 'static>(
        &self,
        working: String,
        work: impl FnOnce(&cli::CliClient) -> Result<T, cli::CliError> + Send + 'static,
        done: impl FnOnce(&UploadsWindow, T) + 'static,
    ) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        self.busy.set(true);
        self.status.set_label(&working);
        self.update_buttons();
        let weak = self.weak();
        app.run_cli(work, move |result| {
            let Some(inner) = weak.upgrade() else { return };
            let this = UploadsWindow(inner);
            this.busy.set(false);
            match result {
                Ok(value) => done(&this, value),
                Err(message) => this.status.set_label(&message),
            }
            this.update_buttons();
        });
    }

    fn open_selected(&self) {
        let (Some(profile), Some(upload)) = (self.chosen_profile(), self.selected_upload()) else {
            return;
        };
        let name = upload.name.clone();
        self.run(
            format!("Downloading {}…", upload.name),
            move |cli| cli::fetch_upload(cli, &profile.id, &name),
            move |this, local_path| {
                let path = std::path::Path::new(&local_path);
                if !local_path.ends_with(".png") || !path.is_file() {
                    this.status
                        .set_label("The download did not produce a PNG file.");
                    return;
                }
                match open_uri(&file_uri(path)) {
                    Ok(()) => this.status.set_label(&format!("Opened {}.", upload.name)),
                    Err(error) => this
                        .status
                        .set_label(&format!("Downloaded, but no app could open it: {error}")),
                }
            },
        );
    }

    fn copy_selected(&self) {
        let (Some(upload), Some(app)) = (self.selected_upload(), self.app.upgrade()) else {
            return;
        };
        let status = self.status.clone();
        let path = upload.remote_path.clone();
        app.copy_text(upload.remote_path, move |copied| {
            status.set_label(&if copied {
                format!("Copied {path}")
            } else {
                "The clipboard is unavailable.".to_owned()
            });
        });
    }

    fn delete_all(&self) {
        let Some(profile) = self.chosen_profile() else {
            return;
        };
        let count = self.uploads.borrow().len() as i64;
        let weak = self.weak();
        dialogs::confirm(
            &self.window,
            "Delete all uploads",
            &format!(
                "Delete all {} from {} on {}?\n\nThis cannot be undone. Other files in the folder \
                 are not touched.",
                plural(count, "uploaded PNG image", "uploaded PNG images"),
                profile.remote_root(),
                profile.host
            ),
            "Delete",
            true,
            move || {
                let Some(inner) = weak.upgrade() else { return };
                let id = profile.id.clone();
                let label = profile.label.clone();
                UploadsWindow(inner).run(
                    format!("Deleting uploads on {}…", profile.host),
                    move |cli| cli::clean_uploads(cli, &id),
                    move |this, deleted| {
                        let message =
                            format!("Deleted {} on {label}.", plural(deleted, "image", "images"));
                        this.reload();
                        // Keep the result visible after the reload finishes.
                        let weak = this.weak();
                        glib::timeout_add_local_once(
                            std::time::Duration::from_millis(1500),
                            move || {
                                if let Some(inner) = weak.upgrade() {
                                    inner.status.set_label(&message);
                                }
                            },
                        );
                    },
                );
            },
        );
    }
}

fn listing_summary(listing: &UploadListing) -> String {
    if listing.uploads.is_empty() {
        return format!("No uploads in {} yet.", listing.remote_directory);
    }
    let total: i64 = listing.uploads.iter().map(|upload| upload.size_bytes).sum();
    let mut text = format!(
        "{}, {}, in {}.",
        plural(listing.uploads.len() as i64, "image", "images"),
        format_size(total),
        listing.remote_directory
    );
    if listing.truncated {
        text.push_str(" Showing the newest only.");
    }
    if listing.skipped_rows > 0 {
        text.push_str(&format!(
            " {} with unsupported names {} hidden.",
            plural(listing.skipped_rows, "file", "files"),
            if listing.skipped_rows == 1 {
                "is"
            } else {
                "are"
            }
        ));
    }
    text
}

/// Horizontal inset shared by the header and the list rows.
const ROW_INSET: i32 = 10;

fn column_row(name: &str, size: &str, uploaded: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let name = label(name);
    name.set_hexpand(true);
    name.set_wrap(false);
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    // Pixel widths, so the bold header lines up with the rows.
    let size = label(size);
    size.set_width_request(90);
    size.set_xalign(1.0);
    let uploaded = label(uploaded);
    uploaded.set_width_request(150);
    row.set_margin_start(ROW_INSET);
    row.set_margin_end(ROW_INSET);
    row.append(&name);
    row.append(&size);
    row.append(&uploaded);
    row
}
