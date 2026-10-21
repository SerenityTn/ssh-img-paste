//! "Manage Profiles": the destination list, a detail pane, and every
//! profile command (set active, test, edit, rename, delete, duplicate).

use super::app::{App, WeakApp};
use super::editor::{self, EditorMode};
use super::{caption, clear_list, dialogs, dim_label, heading, label};
use adw::prelude::*;
use ssh_img_paste_linux::cli::{self, Catalog, Profile, ShotMode};
use ssh_img_paste_linux::profile_rules;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

const STATUS_HINT: &str =
    "Destinations are stored as literal profile files and are never executed.";

pub struct Inner {
    pub window: adw::Window,
    app: WeakApp,
    stack: gtk::Stack,
    list: gtk::ListBox,
    ids: RefCell<Vec<String>>,
    selected: RefCell<Option<String>>,
    filling: Cell<bool>,
    busy: Cell<bool>,
    title: gtk::Label,
    badge: gtk::Label,
    details: adw::PreferencesGroup,
    detail_rows: RefCell<Vec<adw::ActionRow>>,
    set_active: gtk::Button,
    test: gtk::Button,
    edit: gtk::Button,
    rename: gtk::Button,
    delete: gtk::Button,
    add: gtk::Button,
    duplicate: gtk::Button,
    status: gtk::Label,
}

#[derive(Clone)]
pub struct ProfilesWindow(Rc<Inner>);

impl std::ops::Deref for ProfilesWindow {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl ProfilesWindow {
    pub fn new(app: &App) -> Self {
        let list = gtk::ListBox::new();
        // A double click (or Enter) opens the row, as on Windows; one click selects.
        list.set_activate_on_single_click(false);
        list.add_css_class("navigation-sidebar");
        let scroller = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let add = gtk::Button::with_label("Add…");
        add.add_css_class("suggested-action");
        let duplicate = gtk::Button::with_label("Duplicate…");
        let list_buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        list_buttons.set_homogeneous(true);
        list_buttons.append(&add);
        list_buttons.append(&duplicate);
        let left = gtk::Box::new(gtk::Orientation::Vertical, 10);
        left.set_width_request(270);
        left.set_margin_top(12);
        left.set_margin_bottom(12);
        left.set_margin_start(12);
        left.set_margin_end(12);
        left.append(&heading("Destinations"));
        left.append(&scroller);
        left.append(&list_buttons);

        let title = label("");
        title.add_css_class("title-2");
        let badge = dim_label("");
        let details = adw::PreferencesGroup::new();
        let set_active = gtk::Button::with_label("Set active");
        set_active.add_css_class("suggested-action");
        let test = gtk::Button::with_label("Test connection");
        let edit = gtk::Button::with_label("Edit…");
        let rename = gtk::Button::with_label("Rename…");
        let delete = gtk::Button::with_label("Delete…");
        delete.add_css_class("destructive-action");
        let actions = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .column_spacing(8)
            .row_spacing(8)
            .max_children_per_line(5)
            .build();
        for button in [&set_active, &test, &edit, &rename, &delete] {
            actions.insert(button, -1);
        }
        let right = gtk::Box::new(gtk::Orientation::Vertical, 12);
        right.set_margin_top(18);
        right.set_margin_bottom(18);
        right.set_margin_start(18);
        right.set_margin_end(18);
        right.append(&title);
        right.append(&badge);
        right.append(&actions);
        right.append(&details);
        let right_scroller = gtk::ScrolledWindow::builder()
            .child(&right)
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        let split = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        split.append(&left);
        split.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        split.append(&right_scroller);

        let add_first = gtk::Button::builder()
            .label("Add destination…")
            .halign(gtk::Align::Center)
            .build();
        add_first.add_css_class("suggested-action");
        add_first.add_css_class("pill");
        let empty_text = gtk::Box::new(gtk::Orientation::Vertical, 12);
        for text in [
            "SSH Image Paste uses your OpenSSH client and your existing SSH configuration. Keys, \
             ports, users, and jump hosts stay in ~/.ssh/config, and this app never stores \
             passwords.",
            "Before adding a destination, make sure ssh <host> works in a terminal without asking \
             for a password or passphrase.",
        ] {
            let text = gtk::Label::builder()
                .label(text)
                .wrap(true)
                .justify(gtk::Justification::Center)
                .max_width_chars(60)
                .build();
            empty_text.append(&text);
        }
        empty_text.append(&add_first);
        let empty = adw::StatusPage::builder()
            .icon_name("network-server-symbolic")
            .title("Add your first SSH destination")
            .child(&empty_text)
            .build();

        let stack = gtk::Stack::new();
        stack.add_named(&split, Some("profiles"));
        stack.add_named(&empty, Some("empty"));
        let status = caption(STATUS_HINT);
        status.set_margin_top(6);
        status.set_margin_bottom(6);
        status.set_margin_start(12);
        status.set_margin_end(12);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        stack.set_vexpand(true);
        body.append(&stack);
        body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        body.append(&status);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&body));
        let window = adw::Window::builder()
            .title("Manage Profiles — SSH Image Paste")
            .default_width(960)
            .default_height(540)
            .content(&toolbar)
            .build();
        window.set_size_request(680, 420);

        let this = Self(Rc::new(Inner {
            window,
            app: app.downgrade(),
            stack,
            list,
            ids: RefCell::new(Vec::new()),
            selected: RefCell::new(None),
            filling: Cell::new(false),
            busy: Cell::new(false),
            title,
            badge,
            details,
            detail_rows: RefCell::new(Vec::new()),
            set_active,
            test,
            edit,
            rename,
            delete,
            add,
            duplicate,
            status,
        }));
        this.connect(&add_first);
        this.catalog_changed(app.catalog().as_ref());
        this
    }

    fn weak(&self) -> Weak<Inner> {
        Rc::downgrade(&self.0)
    }

    fn connect(&self, add_first: &gtk::Button) {
        let handler = |f: fn(&ProfilesWindow)| {
            let weak = self.weak();
            move |_: &gtk::Button| {
                if let Some(inner) = weak.upgrade() {
                    f(&ProfilesWindow(inner));
                }
            }
        };
        self.add.connect_clicked(handler(ProfilesWindow::start_add));
        add_first.connect_clicked(handler(ProfilesWindow::start_add));
        self.duplicate
            .connect_clicked(handler(ProfilesWindow::start_duplicate));
        self.set_active
            .connect_clicked(handler(ProfilesWindow::activate_selected));
        self.test
            .connect_clicked(handler(ProfilesWindow::test_selected));
        self.edit
            .connect_clicked(handler(ProfilesWindow::edit_selected));
        self.rename
            .connect_clicked(handler(ProfilesWindow::rename_selected));
        self.delete
            .connect_clicked(handler(ProfilesWindow::delete_selected));

        let weak = self.weak();
        self.list.connect_row_selected(move |_, row| {
            let Some(inner) = weak.upgrade() else { return };
            if inner.filling.get() {
                return;
            }
            let id = row.and_then(|row| inner.ids.borrow().get(row.index() as usize).cloned());
            *inner.selected.borrow_mut() = id;
            ProfilesWindow(inner).show_selected();
        });
        let weak = self.weak();
        self.list.connect_row_activated(move |_, _| {
            if let Some(inner) = weak.upgrade()
                && inner.edit.is_sensitive()
            {
                ProfilesWindow(inner).edit_selected();
            }
        });
    }

    fn catalog(&self) -> Option<Catalog> {
        self.app.upgrade().and_then(|app| app.catalog_ok())
    }

    fn selected_profile(&self) -> Option<Profile> {
        let id = self.selected.borrow().clone()?;
        self.catalog()?.find(&id).cloned()
    }

    pub fn catalog_changed(&self, catalog: Option<&Result<Catalog, String>>) {
        let catalog = match catalog {
            Some(Ok(catalog)) => catalog.clone(),
            Some(Err(message)) => {
                self.status.set_label(message);
                return;
            }
            None => return,
        };
        self.stack
            .set_visible_child_name(if catalog.profiles.is_empty() {
                "empty"
            } else {
                "profiles"
            });
        // Keep the selection, else fall back to the active profile, then the first.
        let keep = self
            .selected
            .borrow()
            .clone()
            .filter(|id| catalog.find(id).is_some())
            .or_else(|| catalog.active().map(|profile| profile.id.clone()))
            .or_else(|| catalog.profiles.first().map(|profile| profile.id.clone()));
        self.filling.set(true);
        clear_list(&self.list);
        let mut ids = Vec::new();
        for profile in &catalog.profiles {
            self.list.append(&profile_row(profile, &catalog));
            ids.push(profile.id.clone());
        }
        let index = keep
            .as_ref()
            .and_then(|keep| ids.iter().position(|id| id == keep));
        *self.ids.borrow_mut() = ids;
        *self.selected.borrow_mut() = keep;
        if let Some(row) = index.and_then(|index| self.list.row_at_index(index as i32)) {
            self.list.select_row(Some(&row));
        }
        self.filling.set(false);
        self.show_selected();
    }

    fn show_selected(&self) {
        for row in self.detail_rows.borrow_mut().drain(..) {
            self.details.remove(&row);
        }
        let catalog = self.catalog();
        let profile = self.selected_profile();
        let busy = self.busy.get();
        self.add.set_sensitive(!busy);
        let Some(profile) = profile else {
            self.title.set_label("");
            self.badge.set_label("");
            for button in [
                &self.set_active,
                &self.test,
                &self.edit,
                &self.rename,
                &self.delete,
                &self.duplicate,
            ] {
                button.set_sensitive(false);
            }
            return;
        };
        self.title.set_label(profile.display_name());
        let badge = if !profile.usable {
            format!(
                "This profile cannot be used: {}.",
                profile
                    .problem
                    .as_ref()
                    .map(|problem| problem.describe())
                    .unwrap_or_default()
            )
        } else {
            let mut badge = if profile.active {
                "Active destination — uploads go here.".to_owned()
            } else {
                "Not active.".to_owned()
            };
            if !profile.editable {
                badge.push_str(" Written by hand, so it is read-only here.");
            }
            badge
        };
        self.badge.set_label(&badge);

        let mut rows = vec![("Profile ID", profile.id.clone())];
        if profile.usable {
            rows.extend([
                ("SSH host or alias", profile.host.clone()),
                ("Remote home", profile.remote_home.clone()),
                ("Upload folder", profile.remote_dir.clone()),
                ("Uploads land in", profile.remote_root()),
                (
                    "No clipboard image",
                    match profile.shot_mode {
                        ShotMode::Full => "Capture the full screen".into(),
                        ShotMode::Region => "Capture a region".into(),
                    },
                ),
                (
                    "Restore clipboard after",
                    if profile.restore_seconds == 0 {
                        "Never — keep the path".into()
                    } else {
                        format!("{} seconds", profile.restore_seconds)
                    },
                ),
            ]);
        }
        if (!profile.usable || !profile.editable)
            && let Some(catalog) = &catalog
        {
            rows.push((
                "Stored in",
                format!(
                    "{}/profiles/{}.env",
                    catalog.config_root.trim_end_matches('/'),
                    profile.id
                ),
            ));
        }
        let mut widgets = Vec::new();
        for (title, value) in rows {
            let row = adw::ActionRow::builder()
                .title(title)
                .subtitle(glib_escape(&value))
                .subtitle_selectable(true)
                .build();
            row.add_css_class("property");
            self.details.add(&row);
            widgets.push(row);
        }
        *self.detail_rows.borrow_mut() = widgets;

        let invalid_field = profile
            .problem
            .as_ref()
            .is_some_and(|problem| problem.code == "invalid_field");
        self.set_active
            .set_sensitive(!busy && profile.usable && !profile.active);
        self.test.set_sensitive(!busy && profile.usable);
        self.edit
            .set_sensitive(!busy && (profile.editable || invalid_field));
        self.rename.set_sensitive(!busy);
        self.delete.set_sensitive(!busy);
        self.duplicate.set_sensitive(!busy && profile.usable);
    }

    /// Runs one CLI command with the busy state, status text, and a refresh.
    fn run<T: Send + 'static>(
        &self,
        working: String,
        work: impl FnOnce(&cli::CliClient) -> Result<T, cli::CliError> + Send + 'static,
        message: impl FnOnce(T) -> (String, Option<String>) + 'static,
    ) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        self.busy.set(true);
        self.status.set_label(&working);
        self.window.set_cursor_from_name(Some("progress"));
        self.show_selected();
        let weak = self.weak();
        app.run_cli(work, move |result| {
            let Some(inner) = weak.upgrade() else { return };
            let this = ProfilesWindow(inner);
            this.busy.set(false);
            this.window.set_cursor_from_name(None);
            match result {
                Ok(value) => {
                    let (text, select) = message(value);
                    this.status.set_label(&text);
                    if let Some(select) = select {
                        *this.selected.borrow_mut() = Some(select);
                    }
                }
                Err(error) => this.status.set_label(&error),
            }
            this.show_selected();
            if let Some(app) = this.app.upgrade() {
                app.refresh_catalog();
            }
        });
    }

    pub fn start_add(&self) {
        self.open_editor(EditorMode::Create(None));
    }

    fn start_duplicate(&self) {
        if let Some(profile) = self.selected_profile() {
            self.open_editor(EditorMode::Create(Some(profile)));
        }
    }

    fn edit_selected(&self) {
        if let Some(profile) = self.selected_profile() {
            self.open_editor(EditorMode::Edit(profile));
        }
    }

    fn open_editor(&self, mode: EditorMode) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let existing: Vec<String> = self
            .catalog()
            .map(|catalog| {
                catalog
                    .profiles
                    .iter()
                    .map(|profile| profile.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let weak = self.weak();
        editor::open(&app, &self.window, mode, existing, move |id, message| {
            if let Some(inner) = weak.upgrade() {
                *inner.selected.borrow_mut() = Some(id);
                inner.status.set_label(&message);
                if let Some(app) = inner.app.upgrade() {
                    app.refresh_catalog();
                }
            }
        });
    }

    fn activate_selected(&self) {
        let Some(profile) = self.selected_profile() else {
            return;
        };
        let label = profile.label.clone();
        let id = profile.id.clone();
        self.run(
            format!("Activating {label}…"),
            move |cli| cli::use_profile(cli, &id),
            move |()| (format!("Uploads now go to {label}."), None),
        );
    }

    fn test_selected(&self) {
        let Some(profile) = self.selected_profile() else {
            return;
        };
        let id = profile.id.clone();
        self.run(
            format!("Connecting to {}…", profile.host),
            move |cli| cli::test_profile(cli, &id),
            |test| {
                let text = if test.home_matches {
                    format!(
                        "Connection OK. {} reports home {}, which matches.",
                        test.host, test.detected_home
                    )
                } else {
                    format!(
                        "Connected, but {} reports home {} while this destination uses {}. Edit \
                         the remote home if uploads land in the wrong place.",
                        test.host, test.detected_home, test.configured_home
                    )
                };
                (text, None)
            },
        );
    }

    fn rename_selected(&self) {
        let Some(profile) = self.selected_profile() else {
            return;
        };
        let existing: Vec<String> = self
            .catalog()
            .map(|catalog| {
                catalog
                    .profiles
                    .iter()
                    .filter(|other| other.id != profile.id)
                    .map(|other| other.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let weak = self.weak();
        let old = profile.id.clone();
        dialogs::prompt(
            &self.window,
            "Rename profile",
            &format!(
                "New profile ID for {}. IDs use letters, digits, dashes, and underscores.",
                profile.display_name()
            ),
            &profile.id,
            move |value| {
                let existing: Vec<&str> = existing.iter().map(String::as_str).collect();
                profile_rules::validate_id(value, &existing)
            },
            move |new| {
                let Some(inner) = weak.upgrade() else { return };
                if new == old {
                    return;
                }
                let (from, to) = (old.clone(), new.clone());
                ProfilesWindow(inner).run(
                    "Renaming…".into(),
                    move |cli| cli::rename_profile(cli, &from, &to),
                    move |()| (format!("Renamed {old} to {new}."), Some(new)),
                );
            },
        );
    }

    fn delete_selected(&self) {
        let Some(profile) = self.selected_profile() else {
            return;
        };
        let Some(catalog) = self.catalog() else {
            return;
        };
        let name = profile.display_name().to_owned();
        let weak = self.weak();
        let id = profile.id.clone();
        let run_delete = move |switch_to: Option<String>| {
            let Some(inner) = weak.upgrade() else { return };
            let name = name.clone();
            let id = id.clone();
            ProfilesWindow(inner).run(
                "Deleting…".into(),
                move |cli| cli::delete_profile(cli, &id, switch_to.as_deref()),
                move |()| (format!("Deleted {name}."), None),
            );
        };
        if profile.usable && profile.active {
            let others: Vec<Profile> = catalog
                .usable()
                .filter(|other| other.id != profile.id)
                .cloned()
                .collect();
            if others.is_empty() {
                self.status.set_label(
                    "You cannot delete your only working destination. Add another one first.",
                );
                return;
            }
            let labels: Vec<String> = others.iter().map(Profile::menu_label).collect();
            dialogs::choose(
                &self.window,
                "Delete active destination",
                &format!(
                    "{} is the active destination. Choose where uploads should go instead, then \
                     delete it.",
                    profile.label
                ),
                &labels,
                "Delete",
                true,
                move |index| run_delete(others.get(index).map(|other| other.id.clone())),
            );
        } else {
            dialogs::confirm(
                &self.window,
                "Delete destination",
                &format!(
                    "Delete the destination \"{}\"? This removes its profile file. Uploaded \
                     images on the host are not touched.",
                    profile.display_name()
                ),
                "Delete",
                true,
                move || run_delete(None),
            );
        }
    }
}

fn profile_row(profile: &Profile, catalog: &Catalog) -> gtk::Box {
    let title = label(profile.display_name());
    title.add_css_class("heading");
    title.set_hexpand(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let active =
        catalog.active_profile_id.as_deref() == Some(profile.id.as_str()) && profile.usable;
    let badge_text = if !profile.usable {
        "Needs attention"
    } else if active && !profile.editable {
        "Active · Read-only"
    } else if active {
        "Active"
    } else if !profile.editable {
        "Read-only"
    } else {
        ""
    };
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    top.append(&title);
    if !badge_text.is_empty() {
        let badge = caption(badge_text);
        badge.add_css_class(if profile.usable { "accent" } else { "warning" });
        top.append(&badge);
    }
    let detail = if profile.usable {
        format!("{}  ·  {}", profile.id, profile.host)
    } else {
        format!(
            "{}  ·  {}",
            profile.id,
            profile
                .problem
                .as_ref()
                .map(|problem| problem.describe())
                .unwrap_or_default()
        )
    };
    let subtitle = caption(&detail);
    subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let row = gtk::Box::new(gtk::Orientation::Vertical, 2);
    row.set_margin_top(6);
    row.set_margin_bottom(6);
    row.append(&top);
    row.append(&subtitle);
    row
}

/// Action row subtitles are Pango markup.
fn glib_escape(value: &str) -> String {
    gtk::glib::markup_escape_text(value).to_string()
}
