//! The Linux tray: a StatusNotifierItem (KDE Plasma, GNOME's AppIndicator
//! extension, waybar, …) served by `ksni` instead of tray-icon.
//!
//! tray-icon's Linux backend is libappindicator, which marks its item
//! `ItemIsMenu` and never reports clicks: every host opens the menu on BOTH
//! buttons, so the left-click on/off toggle the Windows tray has could not
//! exist. A plain SNI item gets `Activate` (left click) and shows the
//! DBusMenu on right click.
//!
//! State flows one way. Clicks only send a [`TrayCommand`] to the tray app;
//! the tray shows whatever `method`/`enabled` it was last told through
//! [`SniTray::set_state`] — the same observer path that repaints the Windows
//! tray — so the icon can never claim a state the app did not reach.

use super::icons::{decode_rgba, greyscale_rgba};
use super::tooltip::get_tooltip;
use super::tray_command::TrayCommand;
use super::{CUSTOM_ICON_BYTES, NOM_ICON_BYTES, TELEX_ICON_BYTES, VNI_ICON_BYTES};
use anyhow::Result;
use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, StandardItem, SubMenu};
use ksni::MenuItem;
use std::sync::mpsc::Sender;

/// Ctrl+Shift+<key> hints shown next to rows (the hotkeys themselves are
/// registered by the tray app; these only label them).
const CUSTOM_KEYS: [&str; 7] = ["4", "5", "6", "7", "8", "9", "0"];

/// A method icon in both states, as SNI pixmaps (ARGB32, network order).
struct Pixmaps {
    color: ksni::Icon,
    grey: ksni::Icon,
}

impl Pixmaps {
    fn from_png(bytes: &[u8]) -> Self {
        let (rgba, width, height) = decode_rgba(bytes).unwrap_or((vec![0; 4], 1, 1));
        let grey = greyscale_rgba(&rgba);
        Self {
            color: argb_icon(&rgba, width, height),
            grey: argb_icon(&grey, width, height),
        }
    }
}

fn argb_icon(rgba: &[u8], width: u32, height: u32) -> ksni::Icon {
    let mut data = Vec::with_capacity(rgba.len());
    for [r, g, b, a] in rgba.as_chunks::<4>().0 {
        data.extend_from_slice(&[*a, *r, *g, *b]);
    }
    ksni::Icon {
        width: width as i32,
        height: height as i32,
        data,
    }
}

/// The tray item. Lives on ksni's thread; reach it through the handle.
pub struct SniTray {
    method: String,
    enabled: bool,
    /// Custom keyboards `(id, label)`, menu order (`MethodRegistry`'s).
    customs: Vec<(String, String)>,
    telex: Pixmaps,
    vni: Pixmaps,
    nom: Pixmaps,
    custom: Pixmaps,
    commands: Sender<TrayCommand>,
}

/// Live handle: push state changes with [`SniTray::set_state`].
pub type SniTrayHandle = ksni::blocking::Handle<SniTray>;

impl SniTray {
    /// Register the item. Succeeds even when no SNI host is up yet (tray
    /// autostarted before plasmashell): the item appears once one is.
    pub fn spawn(
        method: &str,
        enabled: bool,
        customs: Vec<(String, String)>,
        commands: Sender<TrayCommand>,
    ) -> Result<SniTrayHandle> {
        let tray = Self {
            method: method.to_string(),
            enabled,
            customs,
            telex: Pixmaps::from_png(TELEX_ICON_BYTES),
            vni: Pixmaps::from_png(VNI_ICON_BYTES),
            nom: Pixmaps::from_png(NOM_ICON_BYTES),
            custom: Pixmaps::from_png(CUSTOM_ICON_BYTES),
            commands,
        };
        Ok(tray.assume_sni_available(true).spawn()?)
    }

    /// Show `method` / `enabled`. No-op (no D-Bus traffic) when unchanged.
    pub fn set_state(handle: &SniTrayHandle, method: &str, enabled: bool) {
        handle.update(|tray| {
            if tray.method != method || tray.enabled != enabled {
                tray.method = method.to_string();
                tray.enabled = enabled;
            }
        });
    }

    fn send(&self, command: TrayCommand) {
        // The receiver lives as long as the tray app's event loop; after it
        // is gone the tray is shutting down and the click means nothing.
        let _ = self.commands.send(command);
    }

    fn custom_name(&self, method: &str) -> Option<&str> {
        self.customs
            .iter()
            .find(|(id, _)| id == method)
            .map(|(_, label)| label.as_str())
    }

    fn method_row(&self, id: &str, label: &str, key: Option<&str>) -> MenuItem<Self> {
        let id = id.to_string();
        CheckmarkItem {
            label: label.to_string(),
            // OFF checks no method: a check must never claim the IME is
            // typing while it is not (same rule as the Windows tray).
            checked: self.enabled && self.method == id,
            shortcut: shortcut(key),
            activate: Box::new(move |tray: &mut Self| tray.send(TrayCommand::Select(id.clone()))),
            ..Default::default()
        }
        .into()
    }
}

fn shortcut(key: Option<&str>) -> Vec<Vec<String>> {
    key.map(|k| vec![vec!["Control".into(), "Shift".into(), k.into()]])
        .unwrap_or_default()
}

impl ksni::Tray for SniTray {
    fn id(&self) -> String {
        "buttre".into()
    }

    fn title(&self) -> String {
        "buttre".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let set = match self.method.as_str() {
            "telex" => &self.telex,
            "vni" => &self.vni,
            "nom" => &self.nom,
            _ => &self.custom,
        };
        vec![if self.enabled {
            set.color.clone()
        } else {
            set.grey.clone()
        }]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let text = get_tooltip(&self.method, self.enabled, self.custom_name(&self.method));
        let (title, description) = text.split_once('\n').unwrap_or((text.as_str(), ""));
        ksni::ToolTip {
            title: title.to_string(),
            description: description.replace('\n', " "),
            ..Default::default()
        }
    }

    /// Left click: on/off, like the Windows tray.
    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayCommand::Toggle);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = vec![
            CheckmarkItem {
                label: "Bật bộ gõ".into(),
                checked: self.enabled,
                shortcut: vec![vec!["Control".into(), "Shift".into(), "space".into()]],
                activate: Box::new(|tray: &mut Self| tray.send(TrayCommand::Toggle)),
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Chữ Việt".into(),
                submenu: vec![
                    self.method_row("telex", "Telex", Some("1")),
                    self.method_row("vni", "VNI", Some("2")),
                ],
                ..Default::default()
            }
            .into(),
            self.method_row("nom", "Nôm", Some("3")),
        ];
        for (i, (id, label)) in self.customs.iter().enumerate() {
            items.push(self.method_row(id, label, CUSTOM_KEYS.get(i).copied()));
        }
        items.extend([
            MenuItem::Separator,
            StandardItem {
                label: "Cấu hình…".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayCommand::OpenConfig)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Thoát".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayCommand::Quit)),
                ..Default::default()
            }
            .into(),
        ]);
        items
    }
}
