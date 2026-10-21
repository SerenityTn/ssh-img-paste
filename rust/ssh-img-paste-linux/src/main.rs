use adw::prelude::*;
use gtk::{gdk, gio, glib};
use ksni::TrayMethods;
use ssh_img_paste_linux::{
    LaunchRequest, TrayAction, USAGE,
    cli::{self, CliClient, CliError},
    clipboard::{ClipboardError, SystemClipboard},
    messages, parse_launch,
    staging::StagingArea,
    upload::{self, UploadOutcome},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
    thread::JoinHandle,
    time::Duration,
};

const WATCHER_UNKNOWN: u8 = 0;
const WATCHER_ONLINE: u8 = 1;
const WATCHER_OFFLINE: u8 = 2;

fn assume_sni_available() -> bool {
    true
}

#[derive(Debug)]
enum UiEvent {
    Action(TrayAction),
    TrayAvailable,
    TrayUnavailable,
    UploadFinished(UploadOutcome),
    DestinationLoaded(Destination),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Destination {
    Active { label: String, host: String },
    NotConfigured,
    Unavailable(String),
}

impl Destination {
    fn describe(&self) -> String {
        match self {
            Self::Active { label, host } => format!("Destination: {label} ({host})"),
            Self::NotConfigured => "No destination yet. Create one in a terminal with \
                                    \"ssh-img-paste profiles create\"."
                .into(),
            Self::Unavailable(message) => message.clone(),
        }
    }
}

/// Platform adapters resolved once at startup and shared with worker threads.
struct Services {
    cli: Result<CliClient, CliError>,
    clipboard: Result<SystemClipboard, ClipboardError>,
    staging: Option<StagingArea>,
}

impl Services {
    fn detect() -> Self {
        let env = |name: &str| std::env::var_os(name);
        let staging = StagingArea::locate(env);
        if let Some(staging) = &staging {
            staging.remove_stale();
        }
        Self {
            cli: CliClient::locate(env, std::env::current_exe().ok()),
            clipboard: SystemClipboard::detect(env, Some(convert_to_png)),
            staging,
        }
    }

    fn upload(&self) -> UploadOutcome {
        let cli = match &self.cli {
            Ok(cli) => cli,
            Err(error) => return UploadOutcome::Failed(messages::describe(error)),
        };
        let Some(staging) = &self.staging else {
            return UploadOutcome::Failed(
                "No private staging folder is available. Set XDG_RUNTIME_DIR or HOME.".into(),
            );
        };
        upload::upload_clipboard(cli, self.clipboard.as_ref(), staging)
    }

    fn destination(&self) -> Destination {
        let cli = match &self.cli {
            Ok(cli) => cli,
            Err(error) => return Destination::Unavailable(messages::describe(error)),
        };
        match cli::catalog(cli) {
            Ok(catalog) => match catalog.active() {
                Some(profile) => Destination::Active {
                    label: profile.label.clone(),
                    host: profile.host.clone(),
                },
                None => Destination::NotConfigured,
            },
            Err(error) if error.code == "not_configured" => Destination::NotConfigured,
            Err(error) => Destination::Unavailable(messages::describe(&error)),
        }
    }
}

/// Decodes a non-PNG clipboard image (JPEG, BMP, …) with GDK's loaders.
/// Textures are immutable, so this is safe off the main thread.
fn convert_to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let texture = gdk::Texture::from_bytes(&glib::Bytes::from(bytes)).ok()?;
    Some(texture.save_to_png_bytes().to_vec())
}

fn spawn_worker(events: &Sender<UiEvent>, work: impl FnOnce() -> UiEvent + Send + 'static) {
    let events = events.clone();
    std::thread::spawn(move || {
        let _ = events.send(work());
    });
}

#[derive(Debug)]
struct LinuxTray {
    events: Sender<UiEvent>,
    watcher_state: Arc<AtomicU8>,
}

impl ksni::Tray for LinuxTray {
    fn id(&self) -> String {
        "ssh-img-paste".into()
    }

    fn title(&self) -> String {
        "SSH Image Paste — development preview".into()
    }

    fn icon_name(&self) -> String {
        "image-x-generic-symbolic".into()
    }

    fn watcher_online(&self) {
        if self.watcher_state.swap(WATCHER_ONLINE, Ordering::AcqRel) != WATCHER_ONLINE {
            let _ = self.events.send(UiEvent::TrayAvailable);
        }
    }

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        if self.watcher_state.swap(WATCHER_OFFLINE, Ordering::AcqRel) != WATCHER_OFFLINE {
            let _ = self.events.send(UiEvent::TrayUnavailable);
        }
        true
    }

    /// A single click uploads, as on macOS and Windows.
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self
            .events
            .send(UiEvent::Action(TrayAction::UploadClipboard));
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{StandardItem, SubMenu};

        vec![
            StandardItem {
                label: "Upload Clipboard Image".into(),
                icon_name: "edit-paste-symbolic".into(),
                activate: Box::new(|tray: &mut LinuxTray| {
                    let _ = tray
                        .events
                        .send(UiEvent::Action(TrayAction::UploadClipboard));
                }),
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Capture (coming soon)".into(),
                enabled: false,
                submenu: vec![
                    StandardItem {
                        label: "Region".into(),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: "Full Screen".into(),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Open SSH Image Paste…".into(),
                activate: Box::new(|tray: &mut LinuxTray| {
                    let _ = tray
                        .events
                        .send(UiEvent::Action(TrayAction::ManageProfiles));
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit-symbolic".into(),
                activate: Box::new(|tray: &mut LinuxTray| {
                    let _ = tray.events.send(UiEvent::Action(TrayAction::Quit));
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

struct TrayRuntime {
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl TrayRuntime {
    fn shutdown(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn start_tray(events: Sender<UiEvent>) -> TrayRuntime {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let thread = std::thread::spawn(move || {
        let watcher_state = Arc::new(AtomicU8::new(WATCHER_UNKNOWN));
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(_error) => {
                let _ = events.send(UiEvent::TrayUnavailable);
                return;
            }
        };
        runtime.block_on(async move {
            match (LinuxTray {
                events: events.clone(),
                watcher_state: watcher_state.clone(),
            })
            .assume_sni_available(assume_sni_available())
            .spawn()
            .await
            {
                Ok(handle) => {
                    if watcher_state
                        .compare_exchange(
                            WATCHER_UNKNOWN,
                            WATCHER_ONLINE,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        let _ = events.send(UiEvent::TrayAvailable);
                    }
                    let _ = shutdown_rx.await;
                    handle.shutdown().await;
                }
                Err(_error) => {
                    let _ = events.send(UiEvent::TrayUnavailable);
                }
            }
        });
    });
    TrayRuntime {
        shutdown: Some(shutdown_tx),
        thread: Some(thread),
    }
}

struct AppState {
    window: adw::ApplicationWindow,
    tray: TrayRuntime,
    events: Sender<UiEvent>,
}

impl AppState {
    fn shutdown(self) {
        self.tray.shutdown();
    }
}

type SharedState = Rc<RefCell<Option<AppState>>>;

/// Builds the tray and window on first use and returns the event sender.
/// Only the first start presents the window, so a shortcut-driven upload
/// does not pop it up every time.
fn ensure_started(app: &adw::Application, state: &SharedState) -> Sender<UiEvent> {
    if let Some(state) = state.borrow().as_ref() {
        return state.events.clone();
    }
    let (sender, receiver) = mpsc::channel();
    let tray = start_tray(sender.clone());
    let services = Arc::new(Services::detect());
    let (window, _event_source) = build_window(app, receiver, sender.clone(), services);
    window.present();
    *state.borrow_mut() = Some(AppState {
        window,
        tray,
        events: sender.clone(),
    });
    sender
}

fn build_window(
    app: &adw::Application,
    receiver: Receiver<UiEvent>,
    events: Sender<UiEvent>,
    services: Arc<Services>,
) -> (adw::ApplicationWindow, glib::SourceId) {
    let status = gtk::Label::builder()
        .label("Copy an image, then upload it to get its remote path.")
        .wrap(true)
        .xalign(0.0)
        .build();

    let destination = gtk::Label::builder()
        .label("Checking destination…")
        .wrap(true)
        .xalign(0.0)
        .build();
    destination.add_css_class("dim-label");

    let remote_path = gtk::Label::builder()
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::Char)
        .xalign(0.0)
        .visible(false)
        .build();
    remote_path.add_css_class("monospace");

    let tray_status = gtk::Label::builder()
        .label("Checking desktop tray support…")
        .wrap(true)
        .xalign(0.0)
        .build();
    tray_status.add_css_class("dim-label");

    let upload = gtk::Button::with_label("Upload Clipboard Image");
    upload.add_css_class("suggested-action");
    upload.connect_clicked({
        let events = events.clone();
        move |_| {
            let _ = events.send(UiEvent::Action(TrayAction::UploadClipboard));
        }
    });
    let region = gtk::Button::with_label("Capture Region");
    let full = gtk::Button::with_label("Capture Full Screen");
    for button in [&region, &full] {
        button.set_sensitive(false);
        button.set_tooltip_text(Some("Screen capture is the next Linux slice"));
    }
    let profiles = gtk::Button::with_label("Manage Profiles");
    profiles.set_sensitive(false);
    profiles.set_tooltip_text(Some(
        "Use \"ssh-img-paste profiles\" in a terminal until the profile editor lands",
    ));

    let actions = gtk::Box::new(gtk::Orientation::Vertical, 12);
    actions.append(&upload);
    actions.append(&region);
    actions.append(&full);
    actions.append(&profiles);

    let title = gtk::Label::builder()
        .label("SSH Image Paste")
        .xalign(0.0)
        .build();
    title.add_css_class("title-1");

    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&title);
    content.append(&destination);
    content.append(&status);
    content.append(&remote_path);
    content.append(&actions);
    content.append(&tray_status);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("SSH Image Paste")
        .default_width(440)
        .default_height(460)
        .content(&content)
        .build();
    let tray_available = Rc::new(Cell::new(false));
    window.connect_close_request({
        let tray_available = tray_available.clone();
        let app = app.clone();
        move |window| {
            if should_hide_window_on_close(tray_available.get()) {
                window.hide();
                glib::Propagation::Stop
            } else {
                app.quit();
                glib::Propagation::Proceed
            }
        }
    });

    let refresh_destination = {
        let events = events.clone();
        let services = services.clone();
        move || {
            let services = services.clone();
            spawn_worker(&events, move || {
                UiEvent::DestinationLoaded(services.destination())
            });
        }
    };
    refresh_destination();

    let weak_window = window.downgrade();
    let app = app.clone();
    let uploading = Cell::new(false);
    let event_source = glib::timeout_add_local(Duration::from_millis(50), move || {
        loop {
            match receiver.try_recv() {
                Ok(UiEvent::Action(TrayAction::ManageProfiles)) => {
                    if let Some(window) = weak_window.upgrade() {
                        window.present();
                    }
                }
                Ok(UiEvent::Action(TrayAction::Quit)) => {
                    app.quit();
                    return glib::ControlFlow::Break;
                }
                Ok(UiEvent::Action(TrayAction::UploadClipboard)) => {
                    if uploading.replace(true) {
                        status.set_label("An upload is already in progress.");
                        continue;
                    }
                    upload.set_sensitive(false);
                    status.set_label("Uploading the clipboard image…");
                    let services = services.clone();
                    spawn_worker(&events, move || UiEvent::UploadFinished(services.upload()));
                }
                Ok(UiEvent::Action(_)) => {
                    status.set_label("Screen capture is not connected yet.");
                }
                Ok(UiEvent::UploadFinished(outcome)) => {
                    uploading.set(false);
                    upload.set_sensitive(true);
                    status.set_label(&outcome.summary());
                    if let UploadOutcome::Uploaded {
                        remote_path: path, ..
                    } = &outcome
                    {
                        remote_path.set_label(path);
                        remote_path.set_visible(true);
                    }
                    app.send_notification(Some("upload"), &upload_notification(&outcome));
                    // The upload may have revealed a changed or removed profile.
                    refresh_destination();
                }
                Ok(UiEvent::DestinationLoaded(loaded)) => {
                    destination.set_label(&loaded.describe());
                }
                Ok(UiEvent::TrayAvailable) => {
                    tray_available.set(true);
                    tray_status
                        .set_label("Tray icon registered. Click it to upload the clipboard image.");
                }
                Ok(UiEvent::TrayUnavailable) => {
                    tray_available.set(false);
                    tray_status.set_label(
                        "Tray icon unavailable; this window remains the application fallback",
                    );
                }
                Err(TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(TryRecvError::Disconnected) => return glib::ControlFlow::Break,
            }
        }
    });

    (window, event_source)
}

fn upload_notification(outcome: &UploadOutcome) -> gio::Notification {
    let (title, body) = match outcome {
        UploadOutcome::Uploaded {
            remote_path,
            destination,
            path_copied,
        } => (
            format!("Uploaded to {destination}"),
            if *path_copied {
                remote_path.clone()
            } else {
                outcome.summary()
            },
        ),
        UploadOutcome::NoImage { .. } | UploadOutcome::NoDestination => {
            ("Nothing uploaded".to_owned(), outcome.summary())
        }
        UploadOutcome::Failed(message) => ("Upload failed".to_owned(), message.clone()),
    };
    let notification = gio::Notification::new(&title);
    notification.set_body(Some(&body));
    notification
}

fn should_hide_window_on_close(tray_available: bool) -> bool {
    tray_available
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

    let app = adw::Application::builder()
        .application_id("io.github.serenitytn.SSHImagePaste")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let state: SharedState = Rc::new(RefCell::new(None));

    app.connect_activate({
        let state = state.clone();
        move |app| {
            let existed = state.borrow().is_some();
            ensure_started(app, &state);
            if existed && let Some(state) = state.borrow().as_ref() {
                state.window.present();
            }
        }
    });
    // Runs in the primary instance, also for invocations forwarded over D-Bus.
    app.connect_command_line({
        let state = state.clone();
        move |app, command_line| {
            let arguments: Vec<String> = command_line
                .arguments()
                .iter()
                .skip(1)
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect();
            match parse_launch(&arguments) {
                Ok(LaunchRequest::Open | LaunchRequest::Help) => app.activate(),
                Ok(LaunchRequest::UploadClipboard) => {
                    let _ = ensure_started(app, &state)
                        .send(UiEvent::Action(TrayAction::UploadClipboard));
                }
                Ok(LaunchRequest::Quit) => app.quit(),
                Ok(LaunchRequest::SmokeTest) => {
                    app.activate();
                    let app = app.clone();
                    glib::timeout_add_local_once(Duration::from_millis(1_500), move || app.quit());
                }
                Err(_) => return glib::ExitCode::from(2),
            }
            glib::ExitCode::SUCCESS
        }
    });
    app.connect_shutdown({
        let state = state.clone();
        move |_| {
            if let Some(state) = state.borrow_mut().take() {
                state.shutdown();
            }
        }
    });

    app.run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksni::Tray;

    #[test]
    fn window_hides_only_while_tray_is_reachable() {
        assert!(should_hide_window_on_close(true));
        assert!(!should_hide_window_on_close(false));
    }

    #[test]
    fn tray_service_waits_for_a_late_desktop_watcher() {
        assert!(assume_sni_available());
    }

    #[test]
    fn watcher_callbacks_report_loss_and_recovery() {
        let (sender, receiver) = mpsc::channel();
        let watcher_state = Arc::new(AtomicU8::new(WATCHER_UNKNOWN));
        let tray = LinuxTray {
            events: sender,
            watcher_state: watcher_state.clone(),
        };
        assert!(tray.watcher_offline(ksni::OfflineReason::No));
        assert!(matches!(
            receiver.recv().expect("offline event"),
            UiEvent::TrayUnavailable
        ));
        assert_eq!(watcher_state.load(Ordering::Acquire), WATCHER_OFFLINE);
        assert!(tray.watcher_offline(ksni::OfflineReason::No));
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
        tray.watcher_online();
        assert!(matches!(
            receiver.recv().expect("online event"),
            UiEvent::TrayAvailable
        ));
        assert_eq!(watcher_state.load(Ordering::Acquire), WATCHER_ONLINE);
    }
}
