use adw::prelude::*;
use gtk::glib;
use ksni::TrayMethods;
use ssh_img_paste_linux::TrayAction;
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

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self
            .events
            .send(UiEvent::Action(TrayAction::ManageProfiles));
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{StandardItem, SubMenu};

        vec![
            StandardItem {
                label: "Upload Clipboard Image (not connected)".into(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Capture (not connected)".into(),
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
}

impl AppState {
    fn shutdown(self) {
        self.tray.shutdown();
    }
}

fn build_window(
    app: &adw::Application,
    receiver: Receiver<UiEvent>,
) -> (adw::ApplicationWindow, glib::SourceId) {
    let status = gtk::Label::builder()
        .label("Development preview — upload and capture adapters are not connected yet")
        .wrap(true)
        .xalign(0.0)
        .build();
    status.add_css_class("dim-label");

    let tray_status = gtk::Label::builder()
        .label("Checking desktop tray support…")
        .wrap(true)
        .xalign(0.0)
        .build();

    let upload = gtk::Button::with_label("Upload Clipboard Image");
    upload.add_css_class("suggested-action");
    let region = gtk::Button::with_label("Capture Region");
    let full = gtk::Button::with_label("Capture Full Screen");
    for button in [&upload, &region, &full] {
        button.set_sensitive(false);
        button.set_tooltip_text(Some("Adapter under development"));
    }
    let profiles = gtk::Button::with_label("Manage Profiles");
    profiles.connect_clicked({
        let status = status.clone();
        move |_| status.set_label("Profile editor is the next desktop slice")
    });

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
    content.append(&status);
    content.append(&tray_status);
    content.append(&actions);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("SSH Image Paste")
        .default_width(440)
        .default_height(410)
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

    let weak_window = window.downgrade();
    let app = app.clone();
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
                Ok(UiEvent::Action(_)) => {
                    status.set_label("This preview action is not connected yet");
                }
                Ok(UiEvent::TrayAvailable) => {
                    tray_available.set(true);
                    tray_status.set_label("Tray icon registered with this desktop");
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

fn should_hide_window_on_close(tray_available: bool) -> bool {
    tray_available
}

fn main() {
    let smoke_test = std::env::args().any(|argument| argument == "--smoke-test");
    let app = adw::Application::builder()
        .application_id("io.github.serenitytn.SSHImagePaste")
        .build();
    let state: Rc<RefCell<Option<AppState>>> = Rc::new(RefCell::new(None));

    app.connect_activate({
        let state = state.clone();
        move |app| {
            if let Some(state) = state.borrow().as_ref() {
                state.window.present();
                return;
            }

            let (sender, receiver) = mpsc::channel();
            let tray = start_tray(sender);
            let (window, _event_source) = build_window(app, receiver);
            window.present();
            *state.borrow_mut() = Some(AppState { window, tray });

            if smoke_test {
                let app = app.clone();
                glib::timeout_add_local_once(Duration::from_millis(1_500), move || app.quit());
            }
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

    app.run_with_args::<&str>(&[]);
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
