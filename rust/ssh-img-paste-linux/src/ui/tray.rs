//! StatusNotifierItem tray icon (KDE, GNOME with AppIndicator, most panels).
//!
//! ksni runs on its own thread with a tokio runtime; menu clicks travel to the
//! GTK main loop as [`UiEvent`]s and state changes travel back as
//! [`TrayState`] snapshots.

use super::app::UiEvent;
use ksni::TrayMethods;
use ssh_img_paste_linux::desktop::{
    DESTINATION_MENU_LABEL, DesktopAction, NO_DESTINATIONS_LABEL, PRIMARY_ACTIONS, PRODUCT_NAME,
    TrayState,
};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
    mpsc::Sender,
};
use std::thread::JoinHandle;

const WATCHER_UNKNOWN: u8 = 0;
const WATCHER_ONLINE: u8 = 1;
const WATCHER_OFFLINE: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayEvent {
    Action(DesktopAction),
    /// The user picked a destination in the Destination submenu.
    UseProfile(String),
    Available,
    Unavailable,
}

struct LinuxTray {
    events: Sender<UiEvent>,
    watcher_state: Arc<AtomicU8>,
    state: TrayState,
    icon_theme_path: String,
}

impl LinuxTray {
    fn send(&self, event: TrayEvent) {
        let _ = self.events.send(UiEvent::Tray(event));
    }
}

impl ksni::Tray for LinuxTray {
    fn id(&self) -> String {
        "ssh-img-paste".into()
    }

    fn title(&self) -> String {
        PRODUCT_NAME.into()
    }

    fn icon_name(&self) -> String {
        self.state.indicator.icon_name().into()
    }

    fn icon_theme_path(&self) -> String {
        self.icon_theme_path.clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self.state.tooltip(),
            ..Default::default()
        }
    }

    fn watcher_online(&self) {
        if self.watcher_state.swap(WATCHER_ONLINE, Ordering::AcqRel) != WATCHER_ONLINE {
            self.send(TrayEvent::Available);
        }
    }

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        if self.watcher_state.swap(WATCHER_OFFLINE, Ordering::AcqRel) != WATCHER_OFFLINE {
            self.send(TrayEvent::Unavailable);
        }
        true
    }

    /// A single click uploads (clipboard, else the screenshot fallback), as on
    /// macOS and Windows.
    fn activate(&mut self, _x: i32, _y: i32) {
        if self.state.enabled(DesktopAction::UploadClipboard) {
            self.send(TrayEvent::Action(DesktopAction::UploadClipboard));
        } else {
            self.send(TrayEvent::Action(DesktopAction::OpenWindow));
        }
    }

    /// Middle click opens the window (SNI has no double-click).
    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayEvent::Action(DesktopAction::OpenWindow));
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{RadioGroup, RadioItem, StandardItem, SubMenu};

        let mut items = Vec::new();
        for action in PRIMARY_ACTIONS {
            if action.starts_group() {
                items.push(ksni::MenuItem::Separator);
            }
            if action == DesktopAction::ManageProfiles {
                items.push(destination_menu(&self.state));
            }
            items.push(
                StandardItem {
                    label: action.label().into(),
                    enabled: self.state.enabled(action),
                    icon_name: action_icon(action).into(),
                    activate: Box::new(move |tray: &mut LinuxTray| {
                        tray.send(TrayEvent::Action(action));
                    }),
                    ..Default::default()
                }
                .into(),
            );
        }
        return items;

        fn destination_menu(state: &TrayState) -> ksni::MenuItem<LinuxTray> {
            let submenu = if state.destinations.is_empty() {
                vec![
                    StandardItem {
                        label: NO_DESTINATIONS_LABEL.into(),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                ]
            } else {
                let ids: Vec<String> = state
                    .destinations
                    .iter()
                    .map(|item| item.id.clone())
                    .collect();
                vec![
                    RadioGroup {
                        // Out of range selects nothing when no destination is active.
                        selected: state.selected_destination().unwrap_or(usize::MAX),
                        select: Box::new(move |tray: &mut LinuxTray, index| {
                            if let Some(id) = ids.get(index) {
                                tray.send(TrayEvent::UseProfile(id.clone()));
                            }
                        }),
                        options: state
                            .destinations
                            .iter()
                            .map(|item| RadioItem {
                                label: item.label.replace('_', "__"),
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                ]
            };
            SubMenu {
                label: DESTINATION_MENU_LABEL.into(),
                submenu,
                ..Default::default()
            }
            .into()
        }
    }
}

fn action_icon(action: DesktopAction) -> &'static str {
    match action {
        DesktopAction::OpenWindow => "",
        DesktopAction::UploadClipboard => "edit-paste-symbolic",
        DesktopAction::CaptureRegion => "selection-mode-symbolic",
        DesktopAction::CaptureFullScreen => "view-fullscreen-symbolic",
        DesktopAction::UploadedImages => "folder-pictures-symbolic",
        DesktopAction::ManageProfiles => "preferences-system-symbolic",
        DesktopAction::Quit => "application-exit-symbolic",
    }
}

pub struct Tray {
    updates: tokio::sync::mpsc::UnboundedSender<TrayState>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Tray {
    pub fn start(events: Sender<UiEvent>, icon_theme_path: String) -> Self {
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
        let (updates_tx, mut updates_rx) = tokio::sync::mpsc::unbounded_channel::<TrayState>();
        let thread = std::thread::spawn(move || {
            let watcher_state = Arc::new(AtomicU8::new(WATCHER_UNKNOWN));
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                let _ = events.send(UiEvent::Tray(TrayEvent::Unavailable));
                return;
            };
            runtime.block_on(async move {
                let tray = LinuxTray {
                    events: events.clone(),
                    watcher_state: watcher_state.clone(),
                    state: TrayState::default(),
                    icon_theme_path,
                };
                // Waits for a late desktop watcher instead of failing.
                let handle = match tray.assume_sni_available(true).spawn().await {
                    Ok(handle) => handle,
                    Err(_) => {
                        let _ = events.send(UiEvent::Tray(TrayEvent::Unavailable));
                        return;
                    }
                };
                if watcher_state
                    .compare_exchange(
                        WATCHER_UNKNOWN,
                        WATCHER_ONLINE,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    let _ = events.send(UiEvent::Tray(TrayEvent::Available));
                }
                loop {
                    tokio::select! {
                        _ = &mut shutdown_rx => break,
                        update = updates_rx.recv() => match update {
                            Some(state) => {
                                handle.update(|tray| tray.state = state).await;
                            }
                            None => break,
                        },
                    }
                }
                handle.shutdown().await;
            });
        });
        Self {
            updates: updates_tx,
            shutdown: Some(shutdown_tx),
            thread: Some(thread),
        }
    }

    pub fn update(&self, state: TrayState) {
        let _ = self.updates.send(state);
    }

    pub fn shutdown(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksni::Tray as _;
    use ssh_img_paste_linux::desktop::DestinationItem;
    use std::sync::mpsc;

    fn tray(state: TrayState) -> (LinuxTray, mpsc::Receiver<UiEvent>) {
        let (sender, receiver) = mpsc::channel();
        (
            LinuxTray {
                events: sender,
                watcher_state: Arc::new(AtomicU8::new(WATCHER_UNKNOWN)),
                state,
                icon_theme_path: String::new(),
            },
            receiver,
        )
    }

    fn next(receiver: &mpsc::Receiver<UiEvent>) -> Option<TrayEvent> {
        match receiver.try_recv() {
            Ok(UiEvent::Tray(event)) => Some(event),
            _ => None,
        }
    }

    #[test]
    fn watcher_callbacks_report_loss_and_recovery() {
        let (tray, receiver) = tray(TrayState::default());
        assert!(tray.watcher_offline(ksni::OfflineReason::No));
        assert_eq!(next(&receiver), Some(TrayEvent::Unavailable));
        assert!(tray.watcher_offline(ksni::OfflineReason::No));
        assert_eq!(next(&receiver), None);
        tray.watcher_online();
        assert_eq!(next(&receiver), Some(TrayEvent::Available));
    }

    #[test]
    fn click_uploads_only_with_a_destination() {
        let (mut idle, receiver) = tray(TrayState::default());
        idle.activate(0, 0);
        assert_eq!(
            next(&receiver),
            Some(TrayEvent::Action(DesktopAction::OpenWindow))
        );

        let (mut ready, receiver) = tray(TrayState {
            destinations: vec![DestinationItem {
                id: "work".into(),
                label: "Work  (w)".into(),
            }],
            active_id: Some("work".into()),
            active_label: Some("Work".into()),
            ..TrayState::default()
        });
        ready.activate(0, 0);
        assert_eq!(
            next(&receiver),
            Some(TrayEvent::Action(DesktopAction::UploadClipboard))
        );
        ready.secondary_activate(0, 0);
        assert_eq!(
            next(&receiver),
            Some(TrayEvent::Action(DesktopAction::OpenWindow))
        );
    }

    #[test]
    fn menu_has_the_destination_submenu_before_manage_profiles() {
        let (tray, _receiver) = tray(TrayState::default());
        let labels: Vec<String> = tray
            .menu()
            .iter()
            .map(|item| match item {
                ksni::MenuItem::Standard(item) => item.label.clone(),
                ksni::MenuItem::SubMenu(menu) => format!("[{}]", menu.label),
                ksni::MenuItem::Separator => "-".into(),
                _ => "?".into(),
            })
            .collect();
        assert_eq!(
            labels,
            [
                "Open SSH Image Paste",
                "-",
                "Upload Clipboard Image",
                "Capture Region",
                "Capture Full Screen",
                "-",
                "Uploaded Images…",
                "[Destination]",
                "Manage Profiles…",
                "-",
                "Quit"
            ]
        );
    }
}
