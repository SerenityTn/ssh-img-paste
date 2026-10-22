//! Application controller: owns the catalog, upload state, windows, tray,
//! and notifications, mirroring the Windows edition's `AppController`.

use super::main_window::{MainWindow, StatusKind};
use super::profiles::ProfilesWindow;
use super::tray::{Tray, TrayEvent};
use super::uploads::UploadsWindow;
use super::{background, open_uri};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use ssh_img_paste_linux::capture::{CaptureTool, SystemCapture};
use ssh_img_paste_linux::cli::{self, Catalog, CliClient, CliError};
use ssh_img_paste_linux::clipboard::{ClipboardAccess, ClipboardError, SystemClipboard};
use ssh_img_paste_linux::desktop::{APP_ID, DesktopAction, Indicator, TrayState};
use ssh_img_paste_linux::messages;
use ssh_img_paste_linux::staging::StagingArea;
use ssh_img_paste_linux::upload::{
    PendingRestore, UploadHooks, UploadOutcome, UploadRequest, UploadSource, run_upload,
};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, mpsc};
use std::time::Duration;

const APP_ICON_SVG: &[u8] = include_bytes!("../../../../assets/AppIcon.svg");
const HIDE_SETTLE: Duration = Duration::from_millis(220);
const INDICATOR_REVERT: Duration = Duration::from_secs(3);
const MONITOR_DEBOUNCE: Duration = Duration::from_millis(400);

/// Messages from worker threads and the tray thread to the main loop.
#[derive(Debug)]
pub enum UiEvent {
    Tray(TrayEvent),
    Progress(String),
    /// Hide app windows before a capture, then acknowledge.
    HideForCapture(mpsc::Sender<()>),
}

/// Platform adapters resolved once at startup and shared with workers.
pub struct Services {
    pub cli: Result<CliClient, CliError>,
    pub clipboard: Result<SystemClipboard, ClipboardError>,
    pub capture: SystemCapture,
    pub staging: Option<StagingArea>,
}

impl Services {
    fn detect() -> Self {
        let env = |name: &str| std::env::var_os(name);
        let staging = StagingArea::locate(env);
        if let Some(staging) = &staging {
            staging.remove_stale();
        }
        let scratch = staging
            .as_ref()
            .map(|staging| staging.directory().join("capture"))
            .unwrap_or_else(|| std::env::temp_dir().join("ssh-img-paste-capture"));
        Self {
            cli: CliClient::locate(env, std::env::current_exe().ok()),
            clipboard: SystemClipboard::detect(env, Some(convert_to_png)),
            capture: SystemCapture::new(CaptureTool::detect(env), scratch, Some(convert_to_png)),
            staging,
        }
    }
}

/// Decodes a non-PNG image (JPEG, BMP, …) with GDK's loaders. Textures are
/// immutable, so this is safe off the main thread.
fn convert_to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let texture = gdk::Texture::from_bytes(&glib::Bytes::from(bytes)).ok()?;
    Some(texture.save_to_png_bytes().to_vec())
}

struct WorkerHooks(mpsc::Sender<UiEvent>);

impl UploadHooks for WorkerHooks {
    fn progress(&self, message: &str) {
        let _ = self.0.send(UiEvent::Progress(message.to_owned()));
    }

    fn before_capture(&self) {
        let (ack, wait) = mpsc::channel();
        if self.0.send(UiEvent::HideForCapture(ack)).is_ok() {
            let _ = wait.recv_timeout(Duration::from_secs(2));
        }
    }
}

pub struct Inner {
    pub gtk: adw::Application,
    services: Arc<Services>,
    events: mpsc::Sender<UiEvent>,
    tray: RefCell<Option<Tray>>,
    tray_available: Cell<bool>,
    catalog: RefCell<Option<Result<Catalog, String>>>,
    refreshing: Cell<bool>,
    refresh_queued: Cell<bool>,
    first_catalog: Cell<bool>,
    background_start: Cell<bool>,
    busy: Cell<bool>,
    indicator: Cell<Indicator>,
    indicator_detail: RefCell<Option<String>>,
    indicator_generation: Cell<u64>,
    main_window: RefCell<Option<MainWindow>>,
    profiles: RefCell<Option<ProfilesWindow>>,
    uploads: RefCell<Option<UploadsWindow>>,
    hidden_for_capture: RefCell<Vec<gtk::Window>>,
    monitors: RefCell<Vec<gio::FileMonitor>>,
    monitored_root: RefCell<Option<String>>,
    monitor_timer: RefCell<Option<glib::SourceId>>,
}

/// Cheap handle to the controller.
#[derive(Clone)]
pub struct App(Rc<Inner>);

impl std::ops::Deref for App {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl App {
    /// Builds the controller, the tray, and the (hidden) main window.
    pub fn start(gtk_app: &adw::Application, background_start: bool) -> Self {
        let (events, receiver) = mpsc::channel();
        let icon_theme_path = install_icon();
        if let (Some(path), Some(display)) = (&icon_theme_path, gdk::Display::default()) {
            gtk::IconTheme::for_display(&display).add_search_path(path);
        }
        gtk::Window::set_default_icon_name(APP_ID);
        let app = App(Rc::new(Inner {
            gtk: gtk_app.clone(),
            services: Arc::new(Services::detect()),
            events: events.clone(),
            tray: RefCell::new(None),
            tray_available: Cell::new(false),
            catalog: RefCell::new(None),
            refreshing: Cell::new(false),
            refresh_queued: Cell::new(false),
            first_catalog: Cell::new(true),
            background_start: Cell::new(background_start),
            busy: Cell::new(false),
            indicator: Cell::new(Indicator::Idle),
            indicator_detail: RefCell::new(None),
            indicator_generation: Cell::new(0),
            main_window: RefCell::new(None),
            profiles: RefCell::new(None),
            uploads: RefCell::new(None),
            hidden_for_capture: RefCell::new(Vec::new()),
            monitors: RefCell::new(Vec::new()),
            monitored_root: RefCell::new(None),
            monitor_timer: RefCell::new(None),
        }));
        let theme_path = icon_theme_path
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        *app.tray.borrow_mut() = Some(Tray::start(events, theme_path));
        *app.main_window.borrow_mut() = Some(MainWindow::new(&app));

        let open = gio::SimpleAction::new("open-window", None);
        open.connect_activate({
            let app = app.downgrade();
            move |_, _| {
                if let Some(app) = app.upgrade() {
                    app.show_main_window();
                }
            }
        });
        gtk_app.add_action(&open);

        let weak = app.downgrade();
        glib::timeout_add_local(Duration::from_millis(50), move || {
            let Some(app) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            loop {
                match receiver.try_recv() {
                    Ok(event) => app.handle(event),
                    Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
                }
            }
        });

        if !background_start {
            app.show_main_window();
        } else {
            // Without a tray host there is no other way in: show the window.
            let weak = app.downgrade();
            glib::timeout_add_local_once(Duration::from_millis(1_500), move || {
                if let Some(app) = weak.upgrade()
                    && !app.tray_available.get()
                {
                    app.show_main_window();
                }
            });
        }
        app.refresh_catalog();
        app
    }

    pub fn downgrade(&self) -> WeakApp {
        WeakApp(Rc::downgrade(&self.0))
    }

    pub fn services(&self) -> Arc<Services> {
        self.services.clone()
    }

    pub fn catalog(&self) -> Option<Result<Catalog, String>> {
        self.catalog.borrow().clone()
    }

    pub fn catalog_ok(&self) -> Option<Catalog> {
        self.catalog
            .borrow()
            .as_ref()
            .and_then(|result| result.as_ref().ok().cloned())
    }

    pub fn tray_available(&self) -> bool {
        self.tray_available.get()
    }

    /// Runs a CLI command on a worker; errors arrive described for users.
    pub fn run_cli<T: Send + 'static>(
        &self,
        work: impl FnOnce(&CliClient) -> Result<T, CliError> + Send + 'static,
        done: impl FnOnce(Result<T, String>) + 'static,
    ) {
        let services = self.services();
        background(
            move || match &services.cli {
                Ok(cli) => work(cli).map_err(|error| messages::describe(&error)),
                Err(error) => Err(messages::describe(error)),
            },
            done,
        );
    }

    fn handle(&self, event: UiEvent) {
        match event {
            UiEvent::Tray(TrayEvent::Action(action)) => self.perform(action),
            UiEvent::Tray(TrayEvent::UseProfile(id)) => self.use_profile(&id),
            UiEvent::Tray(TrayEvent::Available) => {
                self.tray_available.set(true);
                self.with_main(|window| window.tray_changed(true));
            }
            UiEvent::Tray(TrayEvent::Unavailable) => {
                self.tray_available.set(false);
                self.with_main(|window| window.tray_changed(false));
                // Nothing else can bring the app back, so keep a window up.
                self.show_main_window();
            }
            UiEvent::Progress(message) => {
                self.with_main(|window| window.set_status(StatusKind::Busy, &message));
            }
            UiEvent::HideForCapture(ack) => {
                let visible: Vec<gtk::Window> = self
                    .gtk
                    .windows()
                    .into_iter()
                    .filter(|window| window.is_visible())
                    .collect();
                for window in &visible {
                    window.set_visible(false);
                }
                self.hidden_for_capture.borrow_mut().extend(visible);
                glib::timeout_add_local_once(HIDE_SETTLE, move || {
                    let _ = ack.send(());
                });
            }
        }
    }

    pub fn perform(&self, action: DesktopAction) {
        match action {
            DesktopAction::OpenWindow => self.show_main_window(),
            DesktopAction::UploadClipboard => self.upload(UploadSource::Auto),
            DesktopAction::CaptureRegion => self.upload(UploadSource::Region),
            DesktopAction::CaptureFullScreen => self.upload(UploadSource::FullScreen),
            DesktopAction::UploadedImages => self.open_uploads(),
            DesktopAction::ManageProfiles => self.open_profiles(false),
            DesktopAction::Quit => self.gtk.quit(),
        }
    }

    fn with_main(&self, f: impl FnOnce(&MainWindow)) {
        if let Some(window) = self.main_window.borrow().as_ref() {
            f(window);
        }
    }

    pub fn show_main_window(&self) {
        self.with_main(|window| window.window.present());
    }

    pub fn open_profiles(&self, start_add: bool) {
        let existing = self
            .profiles
            .borrow()
            .as_ref()
            .map(|window| window.window.clone());
        let window = match existing {
            Some(window) => window,
            None => {
                let profiles = ProfilesWindow::new(self);
                let window = profiles.window.clone();
                *self.profiles.borrow_mut() = Some(profiles);
                let weak = self.downgrade();
                window.connect_close_request(move |_| {
                    if let Some(app) = weak.upgrade() {
                        app.profiles.borrow_mut().take();
                    }
                    glib::Propagation::Proceed
                });
                window
            }
        };
        window.present();
        if start_add && let Some(profiles) = self.profiles.borrow().as_ref() {
            profiles.start_add();
        }
    }

    pub fn open_uploads(&self) {
        let existing = self
            .uploads
            .borrow()
            .as_ref()
            .map(|window| window.window.clone());
        match existing {
            Some(window) => {
                window.present();
                if let Some(uploads) = self.uploads.borrow().as_ref() {
                    uploads.reload();
                }
            }
            None => {
                let uploads = UploadsWindow::new(self);
                let window = uploads.window.clone();
                *self.uploads.borrow_mut() = Some(uploads);
                let weak = self.downgrade();
                window.connect_close_request(move |_| {
                    if let Some(app) = weak.upgrade() {
                        app.uploads.borrow_mut().take();
                    }
                    glib::Propagation::Proceed
                });
                window.present();
                if let Some(uploads) = self.uploads.borrow().as_ref() {
                    uploads.reload();
                }
            }
        }
    }

    // ---- catalog -------------------------------------------------------

    /// Reloads the catalog; overlapping requests coalesce into one more run.
    pub fn refresh_catalog(&self) {
        if self.refreshing.replace(true) {
            self.refresh_queued.set(true);
            return;
        }
        let app = self.clone();
        self.run_cli(cli::catalog, move |result| {
            app.refreshing.set(false);
            *app.catalog.borrow_mut() = Some(result);
            app.catalog_changed();
            if app.refresh_queued.replace(false) {
                app.refresh_catalog();
            }
        });
    }

    fn catalog_changed(&self) {
        let catalog = self.catalog();
        self.push_tray_state();
        self.with_main(|window| window.catalog_changed(catalog.as_ref(), self.busy.get()));
        if let Some(profiles) = self.profiles.borrow().as_ref() {
            profiles.catalog_changed(catalog.as_ref());
        }
        if let Some(uploads) = self.uploads.borrow().as_ref() {
            uploads.catalog_changed(catalog.as_ref());
        }
        if let Some(Ok(catalog)) = &catalog {
            self.watch_config_root(&catalog.config_root);
            if self.first_catalog.replace(false) && catalog.usable().next().is_none() {
                self.with_main(|window| {
                    window.set_status(StatusKind::Idle, "Add an SSH destination to get started.")
                });
                if self.background_start.get() {
                    self.notify(
                        "SSH Image Paste",
                        "Add an SSH destination to start uploading. Open SSH Image Paste from \
                         the tray icon.",
                    );
                } else {
                    self.open_profiles(true);
                }
            }
        }
    }

    /// Picks up profile edits made in a terminal or another instance.
    fn watch_config_root(&self, root: &str) {
        if root.is_empty() || self.monitored_root.borrow().as_deref() == Some(root) {
            return;
        }
        *self.monitored_root.borrow_mut() = Some(root.to_owned());
        let mut monitors = Vec::new();
        for directory in [PathBuf::from(root), PathBuf::from(root).join("profiles")] {
            let Ok(monitor) = gio::File::for_path(&directory).monitor_directory(
                gio::FileMonitorFlags::WATCH_MOVES,
                None::<&gio::Cancellable>,
            ) else {
                continue;
            };
            let weak = self.downgrade();
            monitor.connect_changed(move |_, file, _, _| {
                let name = file
                    .basename()
                    .map(|name| name.to_string_lossy().into_owned());
                if name
                    .as_deref()
                    .is_some_and(|name| name == "logs" || name.ends_with(".lock"))
                {
                    return;
                }
                if let Some(app) = weak.upgrade() {
                    app.schedule_refresh();
                }
            });
            monitors.push(monitor);
        }
        *self.monitors.borrow_mut() = monitors;
    }

    fn schedule_refresh(&self) {
        if let Some(timer) = self.monitor_timer.borrow_mut().take() {
            timer.remove();
        }
        let weak = self.downgrade();
        let timer = glib::timeout_add_local_once(MONITOR_DEBOUNCE, move || {
            if let Some(app) = weak.upgrade() {
                app.monitor_timer.borrow_mut().take();
                app.refresh_catalog();
            }
        });
        *self.monitor_timer.borrow_mut() = Some(timer);
    }

    fn push_tray_state(&self) {
        let catalog = self.catalog_ok();
        let mut state = TrayState::from_catalog(catalog.as_ref());
        state.busy = self.busy.get();
        state.indicator = self.indicator.get();
        state.detail = self.indicator_detail.borrow().clone();
        if let Some(tray) = self.tray.borrow().as_ref() {
            tray.update(state);
        }
    }

    /// Busy stays until reset; success and error revert to idle after 3s.
    fn set_indicator(&self, indicator: Indicator, detail: Option<String>) {
        self.indicator.set(indicator);
        *self.indicator_detail.borrow_mut() = detail;
        let generation = self.indicator_generation.get() + 1;
        self.indicator_generation.set(generation);
        self.push_tray_state();
        if matches!(indicator, Indicator::Success | Indicator::Error) {
            let weak = self.downgrade();
            glib::timeout_add_local_once(INDICATOR_REVERT, move || {
                if let Some(app) = weak.upgrade()
                    && app.indicator_generation.get() == generation
                {
                    app.set_indicator(Indicator::Idle, None);
                }
            });
        }
    }

    // ---- destinations --------------------------------------------------

    pub fn use_profile(&self, id: &str) {
        let label = self
            .catalog_ok()
            .and_then(|catalog| catalog.find(id).map(|profile| profile.label.clone()))
            .unwrap_or_else(|| id.to_owned());
        let id = id.to_owned();
        let app = self.clone();
        self.run_cli(
            move |cli| cli::use_profile(cli, &id),
            move |result| {
                match result {
                    Ok(()) => app.with_main(|window| {
                        window.set_status(StatusKind::Idle, &format!("Uploads now go to {label}."))
                    }),
                    Err(message) => app.report_failure(&message),
                }
                app.refresh_catalog();
            },
        );
    }

    // ---- uploads -------------------------------------------------------

    pub fn upload(&self, source: UploadSource) {
        if self.busy.get() {
            self.notify("SSH Image Paste", "An upload is already in progress.");
            return;
        }
        self.busy.set(true);
        self.set_indicator(Indicator::Busy, Some("Uploading…".into()));
        self.with_main(|window| {
            window.set_busy(true);
            window.set_status(StatusKind::Busy, "Checking destination…");
        });
        let services = self.services();
        let hooks = WorkerHooks(self.events.clone());
        let app = self.clone();
        background(
            move || {
                let (cli, staging) = match (&services.cli, &services.staging) {
                    (Err(error), _) => {
                        return (UploadOutcome::Failed(messages::describe(error)), None);
                    }
                    (_, None) => {
                        return (
                            UploadOutcome::Failed(
                                "No private staging folder is available. Set XDG_RUNTIME_DIR or HOME."
                                    .into(),
                            ),
                            None,
                        );
                    }
                    (Ok(cli), Some(staging)) => (cli, staging),
                };
                run_upload(
                    UploadRequest {
                        source,
                        runner: cli,
                        clipboard: services.clipboard.as_ref(),
                        capture: &services.capture,
                        staging,
                    },
                    &hooks,
                )
            },
            move |(outcome, restore)| app.finish_upload(outcome, restore),
        );
    }

    fn finish_upload(&self, outcome: UploadOutcome, restore: Option<PendingRestore>) {
        self.busy.set(false);
        for window in self.hidden_for_capture.borrow_mut().drain(..) {
            window.present();
        }
        self.with_main(|window| window.set_busy(false));
        match &outcome {
            UploadOutcome::Uploaded { remote_path, .. } => {
                self.with_main(|window| {
                    window.set_status(StatusKind::Success, &outcome.summary());
                    window.show_path(remote_path);
                });
                self.set_indicator(Indicator::Success, Some(outcome.title()));
                self.notify(&outcome.title(), &outcome.notification_body());
                if let Some(uploads) = self.uploads.borrow().as_ref() {
                    uploads.reload();
                }
            }
            UploadOutcome::Cancelled => {
                self.with_main(|window| window.set_status(StatusKind::Idle, &outcome.summary()));
                self.set_indicator(Indicator::Idle, None);
            }
            UploadOutcome::NoDestination => {
                self.with_main(|window| window.set_status(StatusKind::Error, &outcome.summary()));
                self.set_indicator(Indicator::Idle, None);
                let none_usable = self
                    .catalog_ok()
                    .is_none_or(|catalog| catalog.usable().next().is_none());
                self.open_profiles(none_usable);
            }
            UploadOutcome::Failed(message) => self.report_failure(message),
        }
        if let Some(restore) = restore {
            self.schedule_restore(restore);
        }
        self.refresh_catalog();
    }

    fn report_failure(&self, message: &str) {
        self.with_main(|window| window.set_status(StatusKind::Error, message));
        self.set_indicator(Indicator::Error, Some(message.to_owned()));
        self.notify("Upload failed", message);
    }

    fn schedule_restore(&self, restore: PendingRestore) {
        let services = self.services();
        glib::timeout_add_local_once(restore.after, move || {
            background(
                move || match &services.clipboard {
                    Ok(clipboard) => restore.complete(clipboard),
                    Err(error) => Err(error.clone()),
                },
                |result| match result {
                    Ok(true) => eprintln!("ssh-img-paste: restored the previous clipboard"),
                    Ok(false) => {}
                    Err(error) => eprintln!(
                        "ssh-img-paste: clipboard restore skipped: {}",
                        error.describe()
                    ),
                },
            );
        });
    }

    /// Copies text through the clipboard tool so it outlives the app.
    pub fn copy_text(&self, text: String, done: impl FnOnce(bool) + 'static) {
        let services = self.services();
        background(
            move || match &services.clipboard {
                Ok(clipboard) => clipboard.set_text(&text).is_ok(),
                Err(_) => false,
            },
            done,
        );
    }

    /// Desktop notification; suppressed while the main window has focus.
    pub fn notify(&self, title: &str, body: &str) {
        let focused = self
            .main_window
            .borrow()
            .as_ref()
            .is_some_and(|window| window.window.is_visible() && window.window.is_active());
        if focused {
            return;
        }
        let notification = gio::Notification::new(title);
        notification.set_body(Some(body));
        notification.set_default_action("app.open-window");
        self.gtk
            .send_notification(Some("ssh-img-paste-status"), &notification);
    }

    pub fn open_ssh_config(&self) -> Result<(), String> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("HOME is not set")?;
        let config = home.join(".ssh").join("config");
        let target = if config.is_file() {
            config
        } else if home.join(".ssh").is_dir() {
            home.join(".ssh")
        } else {
            return Err(format!(
                "No SSH config yet. Create {} to define host aliases.",
                config.display()
            ));
        };
        open_uri(&super::file_uri(&target))
    }

    pub fn shutdown(&self) {
        if let Some(tray) = self.tray.borrow_mut().take() {
            tray.shutdown();
        }
    }
}

#[derive(Clone)]
pub struct WeakApp(std::rc::Weak<Inner>);

impl WeakApp {
    pub fn upgrade(&self) -> Option<App> {
        self.0.upgrade().map(App)
    }
}

/// Writes the app icon into a private icon theme so windows, notifications,
/// and the tray can use it by name before any package installs it.
fn install_icon() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    let root = base.join("ssh-img-paste").join("icons");
    let directory = root.join("hicolor").join("scalable").join("apps");
    std::fs::create_dir_all(&directory).ok()?;
    std::fs::write(directory.join(format!("{APP_ID}.svg")), APP_ICON_SVG).ok()?;
    Some(root)
}
