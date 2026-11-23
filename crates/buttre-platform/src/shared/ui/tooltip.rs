//! Tray tooltip text, shared by the muda tray (Windows) and the SNI tray
//! (Linux).

/// Tooltip for the current state. OFF still names the chosen method — the
/// tooltip answers "what will I get when I turn it back on". `custom_name`
/// is the keyboard's display name when `method` is a custom TOML.
pub fn get_tooltip(method: &str, enabled: bool, custom_name: Option<&str>) -> String {
    let method_label = match method {
        "telex" => "Chữ Việt\nTELEX".to_string(),
        "vni" => "Chữ Việt\nVNI".to_string(),
        "nom" => "Chữ Nôm".to_string(),
        method_id => match custom_name {
            Some(name) => format!("Custom\n{name}"),
            None => format!("Chữ Việt\n{}", method_id.to_uppercase()),
        },
    };
    if enabled {
        format!("buttre\n{method_label}")
    } else {
        format!("buttre\nĐã tắt ({})", method_label.replace('\n', " "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_when_off_still_names_the_method() {
        let on = get_tooltip("telex", true, None);
        let off = get_tooltip("telex", false, None);
        assert!(on.contains("TELEX"));
        assert!(off.contains("Đã tắt"), "off must say so: {off}");
        assert!(
            off.contains("TELEX"),
            "off must still show what turning on gives: {off}"
        );
    }

    #[test]
    fn tooltip_never_mentions_english() {
        // "english" is not a method (ADR-0003) — no state renders it.
        for method in ["telex", "vni", "nom", "sometoml"] {
            for enabled in [true, false] {
                let tip = get_tooltip(method, enabled, None);
                assert!(!tip.to_lowercase().contains("english"), "{tip}");
            }
        }
    }
}
