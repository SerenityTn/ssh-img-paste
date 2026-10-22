//! Small modal dialogs on `AdwAlertDialog`: confirm, text prompt, and choice.

use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

const CANCEL: &str = "cancel";
const OK: &str = "ok";

fn alert(heading: &str, body: &str, ok_label: &str, destructive: bool) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.add_response(CANCEL, "Cancel");
    dialog.add_response(OK, ok_label);
    dialog.set_response_appearance(
        OK,
        if destructive {
            adw::ResponseAppearance::Destructive
        } else {
            adw::ResponseAppearance::Suggested
        },
    );
    // Destructive dialogs default to Cancel, as on Windows.
    dialog.set_default_response(Some(if destructive { CANCEL } else { OK }));
    dialog.set_close_response(CANCEL);
    dialog
}

fn once<T>(f: impl FnOnce(T) + 'static) -> impl Fn(T) {
    let cell = Rc::new(RefCell::new(Some(f)));
    move |value| {
        if let Some(f) = cell.borrow_mut().take() {
            f(value);
        }
    }
}

pub fn confirm(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    body: &str,
    ok_label: &str,
    destructive: bool,
    on_confirm: impl FnOnce() + 'static,
) {
    let dialog = alert(heading, body, ok_label, destructive);
    let on_confirm = once(move |()| on_confirm());
    dialog.connect_response(None, move |_, response| {
        if response == OK {
            on_confirm(());
        }
    });
    dialog.present(Some(parent));
}

/// Asks for one line of text, validating live; OK is disabled while invalid.
pub fn prompt(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    body: &str,
    initial: &str,
    validate: impl Fn(&str) -> Option<&'static str> + 'static,
    on_ok: impl FnOnce(String) + 'static,
) {
    let dialog = alert(heading, body, "OK", false);
    let entry = gtk::Entry::builder()
        .text(initial)
        .activates_default(true)
        .build();
    let error = super::caption("");
    error.add_css_class("error");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.append(&entry);
    content.append(&error);
    dialog.set_extra_child(Some(&content));
    let check = {
        let dialog = dialog.clone();
        let error = error.clone();
        move |text: &str| {
            let problem = validate(text.trim());
            error.set_label(problem.unwrap_or(""));
            dialog.set_response_enabled(OK, problem.is_none());
        }
    };
    check(initial);
    entry.connect_changed(move |entry| check(&entry.text()));
    let on_ok = once(on_ok);
    let value = entry.clone();
    dialog.connect_response(None, move |_, response| {
        if response == OK {
            on_ok(value.text().trim().to_owned());
        }
    });
    dialog.present(Some(parent));
    entry.grab_focus();
    entry.select_region(0, -1);
}

/// Picks one of `options`; returns its index.
pub fn choose(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    body: &str,
    options: &[String],
    ok_label: &str,
    destructive: bool,
    on_ok: impl FnOnce(usize) + 'static,
) {
    let dialog = alert(heading, body, ok_label, destructive);
    let labels: Vec<&str> = options.iter().map(String::as_str).collect();
    let picker = gtk::DropDown::from_strings(&labels);
    picker.set_selected(0);
    dialog.set_extra_child(Some(&picker));
    let on_ok = once(on_ok);
    dialog.connect_response(None, move |_, response| {
        if response == OK && picker.selected() != gtk::INVALID_LIST_POSITION {
            on_ok(picker.selected() as usize);
        }
    });
    dialog.present(Some(parent));
}
