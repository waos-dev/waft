//! WaOS programs as a platform's apps, as Flutter's tools find an Android device's through `adb`:
//! `waos` lists the programs that serve extensions, each with a relay command that reaches it.

use std::process::{Command, Stdio};

use waft_service_protocol::{Discovery, ListedApp};

/// The WaOS programs serving extensions, as `waos ext list --json` lists them; none when `waos`
/// is not on the `PATH` or WaOS is not running.
pub(crate) struct WaosPrograms;

impl Discovery for WaosPrograms {
    fn apps(&self) -> Vec<ListedApp> {
        let listed = Command::new("waos")
            .args(["ext", "list", "--json"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        match listed {
            Ok(output) if output.status.success() => {
                listings(&String::from_utf8_lossy(&output.stdout))
            }
            _ => Vec::new(),
        }
    }
}

/// The listings `text` holds, one a line, as a listing file holds one.
fn listings(text: &str) -> Vec<ListedApp> {
    text.lines().filter_map(ListedApp::from_json).collect()
}

#[cfg(test)]
mod tests {
    use waft_service_protocol::Address;

    use super::*;

    #[test]
    fn each_line_waos_prints_is_a_program() {
        let printed = concat!(
            r#"{"app":"notes","identifier":"dev.example.notes.development","device":"WaOS","pid":7,"#,
            r#""address":"command:[\"waos\",\"ext\",\"--id\",\"dev.example.notes.development\",\"--stdio\"]","token":""}"#,
            "\n",
        );
        let [notes] = listings(printed).try_into().expect("one program");
        assert_eq!(notes.app, "notes");
        assert_eq!(notes.device.as_deref(), Some("WaOS"));
        assert_eq!(
            notes.address,
            Address::Command(
                [
                    "waos",
                    "ext",
                    "--id",
                    "dev.example.notes.development",
                    "--stdio"
                ]
                .map(String::from)
                .to_vec()
            )
        );
    }
}
