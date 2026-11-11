//! Environments in the window: the switcher above the scopes, the Add host dialog, and what
//! the main pane shows until a host's state is in.

use rune::{El, Tx, col, row};
use rune_kit::{Kit, Placement, Select, SelectAppearance, TextInput};

use crate::app::{App, Env, RemoteState};
use crate::ds::{self, Card, DialogFrame, DialogKind, Place, field, size, space, text};
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
        // A field, the list's control height (`Place::List`); across the sidebar, not its 280.
        .appearance(SelectAppearance::Field)
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
        .build(k, layer)
        .w_full();
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
            row().w_full().py(space::S6).center().child(ds::loading(k, line))
        }
        RemoteState::NeedsInstall(health) => {
            let detail = match health {
                Health::OtherVersion(v, _) => format!("It has prism {v}; Install puts {VERSION} in ~/.local/bin in its place."),
                Health::Unsupported(system) => format!("No release is built for {system}; put a build there with prism hosts install {name} --binary."),
                Health::Missing(Some(why)) => format!("Its prism doesn't run ({why}); Install puts {VERSION} in ~/.local/bin in its place."),
                Health::Missing(None) | Health::Ready => format!("Install puts it in ~/.local/bin on {name}; nothing else there changes."),
            };
            let install = ds::primary("Install prism", Place::List).disabled(matches!(health, Health::Unsupported(_))).on_press(|s: &mut App| s.install_prism()).build(k).key("env-install");
            ds::empty_action(k, format!("prism {VERSION} isn't on {name}"), detail, install)
        }
        RemoteState::Unreachable(e) => {
            let why = e.strip_prefix(&format!("Can't reach {name}: ")).unwrap_or(e);
            let retry = ds::button("Retry", Place::List).on_press(|s: &mut App| s.retry_remote()).build(k).key("env-retry");
            ds::empty_action(k, format!("Can't reach {name}"), why, retry)
        }
    };
    Some(col().key("env-status").w_full().h_full().min_h(0.0).padding(size::PANE_EDGE).child(body))
}

/// The note on skills.sh while a host is in view: installs land here, not there.
pub fn browse_note(s: &App, k: &Kit) -> Option<El> {
    let host = s.host()?;
    let here = if cfg!(target_os = "macos") { "this Mac" } else { "this computer" };
    Some(ds::text::hint(k, format!("skills.sh installs go to {here}; copy them to {} afterwards", host.name)).key("browse-host-note"))
}

/// Cancel in a dialog's footer.
fn cancel(key: &str, k: &Kit, on: impl Fn(&mut App) + 'static) -> El {
    ds::button("Cancel", Place::Bar).on_press(on).build(k).key(key.to_string())
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
    let fields = Card::new("add-host-fields").row(field(k, "Name", name)).row(field(k, "ssh target", ssh));
    let add = ds::primary("Add host", Place::Bar).on_press(|s: &mut App| s.save_host()).build(k).key("host-add");
    DialogFrame::new("add-host", "Add host", DialogKind::Form)
        .body(text::body(k, "Prism connects with ssh and your keys, never a password; the host needs this version of prism, which the window can install."))
        .body(fields.build(k).expect("two fields"))
        .error(form.error.clone())
        .error_key("host-error")
        .footer(cancel("host-cancel", k, |s| s.add_host = None), add)
        .build(k, |s: &mut App| s.add_host = None, layer);
}

/// Add project… on a host: the folder's path on that machine, typed, since no folder picker
/// reaches its disk.
pub fn add_project(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(form) = &s.host_project else { return };
    let name = s.env_label();
    let path = TextInput::new(&form.path)
        .label("Folder")
        .placeholder("/home/me/projects/site")
        .width(None)
        .invalid(form.error.is_some())
        .on_edit(|s: &mut App| &mut s.host_project.as_mut().expect("the dialog is open").path)
        .on_submit(|s: &mut App, _: &mut Tx| s.save_host_project())
        .build(k)
        .key("host-project-path");
    let fields = Card::new("host-project-fields").row(field(k, "Folder", path));
    let add = ds::primary("Add project", Place::Bar).on_press(|s: &mut App| s.save_host_project()).build(k).key("host-project-add");
    // The frame keys its error line `host-project-error`.
    DialogFrame::new("host-project", format!("Add a project on {name}"), DialogKind::Form)
        .body(text::body(k, format!("Type the project's folder on {name}. Its skills and MCP servers show once {name} lists it.")))
        .body(fields.build(k).expect("one field"))
        .error(form.error.clone())
        .footer(cancel("host-project-cancel", k, |s| s.host_project = None), add)
        .build(k, |s: &mut App| s.host_project = None, layer);
}
