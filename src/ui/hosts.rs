//! Environments in the window: the switcher above the scopes, the Add host dialog, and what
//! the main pane shows until a host's state is in.

use rune::{El, Tx, col, icon, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit, Placement, Select, Spinner, TextInput, glyphs};

use super::dialogs::{dialog_body, field_block, note};
use super::{empty, row_gap};
use crate::app::{App, Env, RemoteState};
use crate::hosts::copy::this_machine;
use crate::hosts::install::{Health, VERSION};

const ADD_HOST: &str = "Add host…";

/// This machine, the saved hosts, and "Add host…".
pub fn switcher(s: &App, k: &Kit, layer: &mut Vec<El>) -> El {
    let mut labels = vec![this_machine().to_string()];
    labels.extend(s.local_settings().hosts.iter().map(|h| h.name.clone()));
    labels.push(ADD_HOST.to_string());
    let current = s.env_label();
    let select = Select::new("env-switch", &labels)
        .current(Some(&current))
        .label("Machine")
        .placement(Placement::BottomStart)
        .open(s.env_menu)
        .on_toggle(|s: &mut App| s.env_menu = !s.env_menu)
        .on_close(|s: &mut App| s.env_menu = false)
        .on_choose(|s: &mut App, v: &str, _: &mut Tx| {
            s.env_menu = false;
            match v {
                ADD_HOST => s.open_add_host(),
                v if v == this_machine() => s.switch_env(None),
                v => s.switch_env(Some(v)),
            }
        })
        .build(k, layer);
    row().key("env").w_full().center_y().child(select)
}

/// The main pane while a host is connecting, needs prism, or can't be reached.
pub fn pending(s: &App, k: &Kit) -> Option<El> {
    let Env::Remote { host, state } = &s.env else { return None };
    let name = &host.name;
    let body = match state {
        RemoteState::Ready => return None,
        RemoteState::Connecting => {
            let line = if s.installing_prism { format!("Installing prism {VERSION} on {name}…") } else { format!("Connecting to {name}…") };
            row().w_full().py(48.0).center().child(row_gap().child(Spinner::new().build(k)).child(text(line).font(k.ty.small).color(k.c.text_muted)))
        }
        RemoteState::NeedsInstall(health) => {
            let detail = match health {
                Health::OtherVersion(v) => format!("It has prism {v}; Install puts {VERSION} in ~/.local/bin in its place."),
                Health::Unsupported(system) => format!("No release is built for {system}; put a build there with prism hosts install {name} --binary."),
                Health::Missing | Health::Ready => format!("Install puts it in ~/.local/bin on {name}; nothing else there changes."),
            };
            let install = Button::new("Install prism")
                .variant(ButtonVariant::Contrast)
                .disabled(matches!(health, Health::Unsupported(_)))
                .on_press(|s: &mut App| s.install_prism())
                .build(k)
                .key("env-install");
            empty(&format!("prism {VERSION} isn't on {name}"), &detail, k).child(row().pt(10.0).child(install))
        }
        RemoteState::Unreachable(e) => {
            let why = e.strip_prefix(&format!("Can't reach {name}: ")).unwrap_or(e);
            let retry = Button::new("Retry").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.retry_remote()).build(k).key("env-retry");
            empty(&format!("Can't reach {name}"), why, k).child(row().pt(10.0).child(retry))
        }
    };
    Some(col().key("env-status").w_full().h_full().min_h(0.0).padding(16.0).child(body))
}

/// The note on skills.sh while a host is in view: installs land here, not there.
pub fn browse_note(s: &App, k: &Kit) -> Option<El> {
    let host = s.host()?;
    let here = if cfg!(target_os = "macos") { "this Mac" } else { "this computer" };
    Some(text(format!("skills.sh installs go to {here}; copy them to {} afterwards", host.name)).font(k.ty.caption()).color(k.c.text_muted).wrap().key("browse-host-note"))
}

/// A name and an ssh target, saved on this machine; then the window connects.
pub fn add_host(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(form) = &s.add_host else { return };
    let name = TextInput::new(&form.name)
        .label("Name")
        .placeholder("devbox")
        .width(None)
        .on_edit(|s: &mut App| &mut s.add_host.as_mut().expect("the dialog is open").name)
        .on_submit(|s: &mut App, _: &mut Tx| s.save_host())
        .build(k)
        .key("host-name");
    let ssh = TextInput::new(&form.ssh)
        .label("ssh target")
        .placeholder("me@devbox.lan")
        .width(None)
        .on_edit(|s: &mut App| &mut s.add_host.as_mut().expect("the dialog is open").ssh)
        .on_submit(|s: &mut App, _: &mut Tx| s.save_host())
        .build(k)
        .key("host-ssh");
    let mut body = dialog_body()
        .child(note("Prism connects with ssh and your keys, never a password; the host needs this version of prism, which the window can install.", k))
        .child(field_block("Name", name, k))
        .child(field_block("ssh target", ssh, k));
    if let Some(e) = &form.error {
        body = body.child(
            row()
                .key("host-error")
                .gap(6.0)
                .center_y()
                .child(icon(&glyphs::CIRCLE_X, 14.0).color(k.c.danger.icon))
                .child(text(e.clone()).font(k.ty.small).color(k.c.text_muted).wrap()),
        );
    }
    let buttons = row()
        .w_full()
        .gap(8.0)
        .justify_end()
        .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.add_host = None).build(k).key("host-cancel"))
        .child(Button::new("Add host").variant(ButtonVariant::Contrast).on_press(|s: &mut App| s.save_host()).build(k).key("host-add"));
    Dialog::new("add-host-dialog", "Add host")
        .width(480.0)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.add_host = None)
        .child(body.child(buttons))
        .build(k, layer);
}
