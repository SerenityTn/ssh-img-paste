//! The profile editor: add, duplicate, or edit one destination with live
//! validation, a suggested ID, and remote-home detection over SSH.

use super::app::App;
use super::{caption, label};
use adw::prelude::*;
use gtk::glib;
use ssh_img_paste_linux::cli::{self, CliError, Profile, ProfileFields, ShotMode};
use ssh_img_paste_linux::profile_rules::{self, DEFAULT_REMOTE_DIR};
use std::cell::Cell;
use std::rc::Rc;

pub enum EditorMode {
    /// A new destination, optionally prefilled from a template (Duplicate).
    Create(Option<Profile>),
    Edit(Profile),
}

struct Field {
    entry: gtk::Entry,
    hint: gtk::Label,
    default_hint: String,
}

impl Field {
    fn new(form: &gtk::Box, title: &str, hint: &str, monospace: bool) -> Self {
        let entry = gtk::Entry::builder()
            .activates_default(true)
            .hexpand(true)
            .build();
        if monospace {
            entry.add_css_class("monospace");
        }
        let hint_label = caption(hint);
        form.append(&label_for(title));
        form.append(&entry);
        form.append(&hint_label);
        Self {
            entry,
            hint: hint_label,
            default_hint: hint.into(),
        }
    }

    fn text(&self) -> String {
        self.entry.text().trim().to_owned()
    }

    /// Shows `error` in place of the hint; empty-field errors wait for Save.
    fn show(&self, error: Option<&str>, attempted: bool) {
        let visible = error.filter(|_| attempted || !self.text().is_empty());
        match visible {
            Some(error) => {
                self.hint.set_label(error);
                self.hint.add_css_class("error");
                self.entry.add_css_class("error");
            }
            None => {
                self.hint.set_label(&self.default_hint);
                self.hint.remove_css_class("error");
                self.entry.remove_css_class("error");
            }
        }
    }
}

fn label_for(text: &str) -> gtk::Label {
    let title = label(text);
    title.add_css_class("heading");
    title.set_margin_top(6);
    title
}

struct Editor {
    window: adw::Window,
    create: bool,
    original: Option<Profile>,
    existing: Vec<String>,
    label: Field,
    id: Option<Field>,
    host: Field,
    home: Field,
    dir: Field,
    restore: Field,
    shot: gtk::DropDown,
    detect: gtk::Button,
    save: gtk::Button,
    status: gtk::Label,
    attempted: Cell<bool>,
    id_edited: Cell<bool>,
    suggesting: Cell<bool>,
    busy: Cell<bool>,
}

pub fn open(
    app: &App,
    parent: &impl IsA<gtk::Window>,
    mode: EditorMode,
    existing: Vec<String>,
    on_saved: impl Fn(String, String) + 'static,
) {
    let (create, template, title) = match &mode {
        EditorMode::Create(None) => (true, None, "Add destination".to_owned()),
        EditorMode::Create(Some(template)) => (
            true,
            Some(template.clone()),
            format!("Duplicate {}", template.display_name()),
        ),
        EditorMode::Edit(profile) => (
            false,
            Some(profile.clone()),
            format!("Edit {}", profile.display_name()),
        ),
    };
    // An invalid profile has no trustworthy values, so it starts from defaults.
    let prefill = template.clone().filter(|profile| profile.usable);

    let form = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let label = Field::new(&form, "Display name", "", false);
    let id = create.then(|| {
        Field::new(
            &form,
            "Profile ID",
            "Letters, digits, dashes, and underscores. Used as the profile file name.",
            false,
        )
    });
    let host = Field::new(
        &form,
        "SSH host or alias",
        "A Host alias from ~/.ssh/config or user@host. Keep ports, keys, and jump hosts in the SSH \
         config.",
        true,
    );
    let ssh_config = gtk::Button::builder()
        .label("Open ~/.ssh/config")
        .halign(gtk::Align::Start)
        .build();
    ssh_config.add_css_class("flat");
    ssh_config.add_css_class("caption");
    form.append(&ssh_config);

    form.append(&label_for("Remote home"));
    let home_entry = gtk::Entry::builder()
        .activates_default(true)
        .hexpand(true)
        .build();
    home_entry.add_css_class("monospace");
    let detect = gtk::Button::with_label("Detect");
    let home_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    home_row.append(&home_entry);
    home_row.append(&detect);
    form.append(&home_row);
    let home_hint_text = "The SSH account's absolute home directory on the host, such as /home/me.";
    let home_hint = caption(home_hint_text);
    form.append(&home_hint);
    let home = Field {
        entry: home_entry,
        hint: home_hint,
        default_hint: home_hint_text.into(),
    };
    let dir = Field::new(
        &form,
        "Upload folder",
        "Relative to the remote home, such as img-uploads.",
        false,
    );

    form.append(&label_for("When the clipboard has no image"));
    let shot = gtk::DropDown::from_strings(&["Capture a region", "Capture the full screen"]);
    form.append(&shot);
    let restore = Field::new(
        &form,
        "Restore clipboard after (seconds)",
        "0 keeps the path on the clipboard.",
        false,
    );

    let status = caption("");
    status.set_margin_top(10);
    let cancel = gtk::Button::with_label("Cancel");
    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(12);
    buttons.append(&cancel);
    buttons.append(&save);
    form.append(&status);
    form.append(&buttons);
    form.set_margin_top(12);
    form.set_margin_bottom(18);
    form.set_margin_start(18);
    form.set_margin_end(18);

    let scroller = gtk::ScrolledWindow::builder()
        .child(&form)
        .propagate_natural_height(true)
        .max_content_height(720)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&scroller));
    let window = adw::Window::builder()
        .title(&title)
        .default_width(560)
        .modal(true)
        .transient_for(parent)
        .content(&toolbar)
        .build();
    window.set_default_widget(Some(&save));

    // Prefill.
    match &prefill {
        Some(profile) => {
            label.entry.set_text(&if create {
                format!("{} copy", profile.label)
            } else {
                profile.label.clone()
            });
            host.entry.set_text(&profile.host);
            home.entry.set_text(&profile.remote_home);
            dir.entry.set_text(&profile.remote_dir);
            shot.set_selected(if profile.shot_mode == ShotMode::Full {
                1
            } else {
                0
            });
            restore.entry.set_text(&profile.restore_seconds.to_string());
        }
        None => {
            dir.entry.set_text(DEFAULT_REMOTE_DIR);
            restore.entry.set_text("60");
        }
    }

    let editor = Rc::new(Editor {
        window,
        create,
        original: if create { None } else { template },
        existing,
        label,
        id,
        host,
        home,
        dir,
        restore,
        shot,
        detect,
        save,
        status,
        attempted: Cell::new(false),
        id_edited: Cell::new(false),
        suggesting: Cell::new(false),
        busy: Cell::new(false),
    });
    editor.suggest_id();
    editor.validate();

    let weak = Rc::downgrade(&editor);
    let on_change = move || {
        if let Some(editor) = weak.upgrade() {
            editor.validate();
        }
    };
    for field in [&editor.host, &editor.home, &editor.dir, &editor.restore] {
        let on_change = on_change.clone();
        field.entry.connect_changed(move |_| on_change());
    }
    let weak = Rc::downgrade(&editor);
    editor.label.entry.connect_changed(move |_| {
        if let Some(editor) = weak.upgrade() {
            editor.suggest_id();
            editor.validate();
        }
    });
    if let Some(id) = &editor.id {
        let weak = Rc::downgrade(&editor);
        id.entry.connect_changed(move |_| {
            if let Some(editor) = weak.upgrade() {
                if !editor.suggesting.get() {
                    editor.id_edited.set(true);
                }
                editor.validate();
            }
        });
    }

    let window = editor.window.clone();
    cancel.connect_clicked(move |_| window.close());
    let status = editor.status.clone();
    let weak_app = app.downgrade();
    ssh_config.connect_clicked(move |_| {
        if let Some(app) = weak_app.upgrade()
            && let Err(message) = app.open_ssh_config()
        {
            status.set_label(&message);
        }
    });

    let weak = Rc::downgrade(&editor);
    let weak_app = app.downgrade();
    editor.detect.connect_clicked(move |_| {
        let (Some(editor), Some(app)) = (weak.upgrade(), weak_app.upgrade()) else {
            return;
        };
        editor.probe(&app);
    });

    let weak = Rc::downgrade(&editor);
    let weak_app = app.downgrade();
    let on_saved = Rc::new(on_saved);
    editor.save.connect_clicked(move |_| {
        let (Some(editor), Some(app)) = (weak.upgrade(), weak_app.upgrade()) else {
            return;
        };
        editor.submit(&app, on_saved.clone());
    });

    // Focus the display name when empty, else the host.
    if editor.label.text().is_empty() {
        editor.label.entry.grab_focus();
    } else {
        editor.host.entry.grab_focus();
    }
    editor.window.present();
    // The window keeps the editor alive through its signal closures.
    let keep = editor.clone();
    editor.window.connect_close_request(move |_| {
        let _ = &keep;
        glib::Propagation::Proceed
    });
}

impl Editor {
    fn others(&self) -> Vec<&str> {
        self.existing
            .iter()
            .map(String::as_str)
            .filter(|id| Some(*id) != self.original.as_ref().map(|profile| profile.id.as_str()))
            .collect()
    }

    fn suggest_id(&self) {
        let Some(id) = &self.id else { return };
        if self.id_edited.get() {
            return;
        }
        self.suggesting.set(true);
        id.entry.set_text(&profile_rules::suggest_id(
            &self.label.text(),
            &self.others(),
        ));
        self.suggesting.set(false);
    }

    /// Validates every field; returns whether the form can be saved.
    fn validate(&self) -> bool {
        let attempted = self.attempted.get();
        let checks = [
            (
                &self.label,
                profile_rules::validate_label(&self.label.text()),
            ),
            (&self.host, profile_rules::validate_host(&self.host.text())),
            (
                &self.home,
                profile_rules::validate_remote_home(&self.home.text()),
            ),
            (
                &self.dir,
                profile_rules::validate_remote_dir(&self.dir.text()),
            ),
            (
                &self.restore,
                profile_rules::validate_restore_seconds(&self.restore.text()),
            ),
        ];
        let mut valid = true;
        for (field, error) in checks {
            field.show(error, attempted);
            valid &= error.is_none();
        }
        if let Some(id) = &self.id {
            let error = profile_rules::validate_id(&id.text(), &self.others());
            id.show(error, attempted);
            valid &= error.is_none();
        }
        let home_ok = profile_rules::validate_remote_home(&self.home.text()).is_none();
        let dir_ok = profile_rules::validate_remote_dir(&self.dir.text()).is_none();
        if home_ok && dir_ok {
            self.dir.hint.set_label(&format!(
                "Uploads land in {}/{}. The folder is created on first upload.",
                self.home.text().trim_end_matches('/'),
                self.dir.text().trim_end_matches('/')
            ));
        }
        self.detect.set_sensitive(
            !self.busy.get() && profile_rules::validate_host(&self.host.text()).is_none(),
        );
        self.save.set_sensitive(!self.busy.get());
        valid
    }

    fn set_status(&self, text: &str, error: bool) {
        self.status.set_label(text);
        if error {
            self.status.add_css_class("error");
        } else {
            self.status.remove_css_class("error");
        }
    }

    fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        self.validate();
    }

    fn probe(self: &Rc<Self>, app: &App) {
        let host = self.host.text();
        self.set_status(&format!("Connecting to {host}…"), false);
        self.set_busy(true);
        let weak = Rc::downgrade(self);
        let probe_host = host.clone();
        app.run_cli(
            move |cli| cli::probe_host(cli, &probe_host),
            move |result| {
                let Some(editor) = weak.upgrade() else { return };
                editor.set_busy(false);
                match result {
                    Ok(home) => {
                        editor.home.entry.set_text(&home);
                        editor
                            .set_status(&format!("Connected. {host} reports home {home}."), false);
                    }
                    Err(message) => editor.set_status(&message, true),
                }
            },
        );
    }

    fn fields(&self) -> ProfileFields {
        ProfileFields {
            label: Some(self.label.text()),
            host: Some(self.host.text()),
            remote_home: Some(self.home.text()),
            remote_dir: Some(self.dir.text()),
            shot_mode: Some(if self.shot.selected() == 1 {
                ShotMode::Full
            } else {
                ShotMode::Region
            }),
            restore_seconds: self.restore.text().parse().ok(),
        }
    }

    /// Only the fields that differ from the stored profile; at least the label.
    fn changed_fields(&self) -> ProfileFields {
        let mut fields = self.fields();
        if let Some(original) = self.original.as_ref().filter(|profile| profile.usable) {
            let same = |value: &Option<String>, current: &str| value.as_deref() == Some(current);
            if same(&fields.host, &original.host) {
                fields.host = None;
            }
            if same(&fields.remote_home, &original.remote_home) {
                fields.remote_home = None;
            }
            if same(&fields.remote_dir, &original.remote_dir) {
                fields.remote_dir = None;
            }
            if fields.shot_mode == Some(original.shot_mode) {
                fields.shot_mode = None;
            }
            if fields.restore_seconds == Some(original.restore_seconds) {
                fields.restore_seconds = None;
            }
            let anything_else = fields.host.is_some()
                || fields.remote_home.is_some()
                || fields.remote_dir.is_some()
                || fields.shot_mode.is_some()
                || fields.restore_seconds.is_some();
            if anything_else && same(&fields.label, &original.label) {
                fields.label = None;
            }
        }
        fields
    }

    fn submit(self: &Rc<Self>, app: &App, on_saved: Rc<dyn Fn(String, String)>) {
        self.attempted.set(true);
        if !self.validate() {
            self.set_status("Fix the highlighted fields.", true);
            return;
        }
        self.set_status("Saving…", false);
        self.set_busy(true);
        let label = self.label.text();
        let weak = Rc::downgrade(self);
        let done = move |result: Result<(String, String), (String, Option<String>)>| {
            let Some(editor) = weak.upgrade() else { return };
            editor.set_busy(false);
            match result {
                Ok((id, message)) => {
                    editor.window.close();
                    on_saved(id, message);
                }
                Err((message, field)) => {
                    editor.set_status(&message, true);
                    if let Some(field) = editor.field_named(field.as_deref()) {
                        field.entry.grab_focus();
                    }
                }
            }
        };
        let services = app.services();
        if self.create {
            let id = self.id.as_ref().map(Field::text).unwrap_or_default();
            let fields = self.fields();
            super::background(
                move || match &services.cli {
                    Ok(cli) => cli::create_profile(cli, &id, &fields)
                        .map(|activated| {
                            let message = if activated {
                                format!("Added {label}. It is now the active destination.")
                            } else {
                                format!("Added {label}.")
                            };
                            (id, message)
                        })
                        .map_err(describe_with_field),
                    Err(error) => Err(describe_with_field(error.clone())),
                },
                done,
            );
        } else {
            let id = self
                .original
                .as_ref()
                .map(|profile| profile.id.clone())
                .unwrap_or_default();
            let fields = self.changed_fields();
            super::background(
                move || match &services.cli {
                    Ok(cli) => cli::update_profile(cli, &id, &fields)
                        .map(|()| (id, format!("Saved {label}.")))
                        .map_err(describe_with_field),
                    Err(error) => Err(describe_with_field(error.clone())),
                },
                done,
            );
        }
    }

    fn field_named(&self, name: Option<&str>) -> Option<&Field> {
        match name? {
            "label" => Some(&self.label),
            "host" => Some(&self.host),
            "remote_home" => Some(&self.home),
            "remote_dir" => Some(&self.dir),
            "restore_seconds" => Some(&self.restore),
            _ => None,
        }
    }
}

fn describe_with_field(error: CliError) -> (String, Option<String>) {
    (
        ssh_img_paste_linux::messages::describe(&error),
        error.field.clone(),
    )
}
