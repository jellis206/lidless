//! Everything that touches power state: reading it from `pmset`, and switching `disablesleep`
//! through a narrow sudoers rule that allows only those two exact commands.

use std::process::Command;

use objc2::AnyThread;
use objc2_foundation::{NSAppleScript, NSProcessInfo, NSProcessInfoThermalState, NSString, NSUserName};

pub const SUDOERS_PATH: &str = "/etc/sudoers.d/lidless";
pub const LOW_BATTERY: u8 = 20;
const PMSET: &str = "/usr/bin/pmset";
const SUDO: &str = "/usr/bin/sudo";

/// Runs a system tool by absolute path. Returns whether it exited 0, and its stdout.
fn run(path: &str, args: &[&str]) -> (bool, String) {
    match Command::new(path).args(args).output() {
        Ok(out) => (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned()),
        Err(_) => (false, String::new()),
    }
}

pub fn sleep_disabled() -> bool {
    parse_sleep_disabled(&run(PMSET, &["-g"]).1)
}

fn parse_sleep_disabled(pmset_g: &str) -> bool {
    pmset_g
        .lines()
        .find(|l| l.contains("SleepDisabled"))
        .is_some_and(|l| l.trim_end().ends_with('1'))
}

/// True when the sudoers rule is installed, so switching needs no password.
pub fn password_free() -> bool {
    run(SUDO, &["-n", "-l", PMSET, "-a", "disablesleep", "0"]).0
}

fn switch_without_password(on: bool) -> bool {
    run(SUDO, &["-n", PMSET, "-a", "disablesleep", if on { "1" } else { "0" }]).0
}

/// Runs a shell script as root through the standard macOS admin-password dialog.
fn as_admin(sh: &str) -> bool {
    let escaped = sh.replace('\\', "\\\\").replace('"', "\\\"");
    let source = NSString::from_str(&format!(
        "do shell script \"{escaped}\" with administrator privileges"
    ));
    let Some(script) = NSAppleScript::initWithSource(NSAppleScript::alloc(), &source) else {
        return false;
    };
    let mut error = None;
    unsafe { script.executeAndReturnError(Some(&mut error)) };
    error.is_none()
}

/// macOS short usernames are ASCII `[A-Za-z0-9._-]`. Anything else never goes into sudoers.
fn valid_user(user: &str) -> bool {
    !user.is_empty() && user.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// The root script for the first switch: flip the setting, then install the sudoers rule
/// atomically (dot-prefixed temp file is ignored by sudo, checked by visudo, then renamed, and
/// removed if any of that fails).
fn install_script(user: &str, on: bool) -> String {
    let v = if on { "1" } else { "0" };
    if !valid_user(user) {
        return format!("{PMSET} -a disablesleep {v}");
    }
    let rule = format!(
        "{user} ALL=(root) NOPASSWD: {PMSET} -a disablesleep 0, {PMSET} -a disablesleep 1"
    );
    format!(
        "{PMSET} -a disablesleep {v} && \
         T=$(/usr/bin/mktemp /etc/sudoers.d/.lidless.XXXXXX) && \
         {{ printf '%s\\n' '{rule}' > \"$T\" && \
         /usr/sbin/visudo -cf \"$T\" >/dev/null && \
         /bin/chmod 440 \"$T\" && /usr/sbin/chown root:wheel \"$T\" && \
         /bin/mv -f \"$T\" {SUDOERS_PATH} || {{ /bin/rm -f \"$T\"; false; }}; }}"
    )
}

/// Switches `disablesleep`. Asks for the admin password only if the sudoers rule is missing.
pub fn set_sleep_disabled(on: bool) -> bool {
    switch_without_password(on) || as_admin(&install_script(&NSUserName().to_string(), on))
}

/// Restores normal sleep without ever prompting. Used on quit, when a dialog would be wrong.
pub fn allow_sleep_quietly() -> bool {
    switch_without_password(false)
}

pub fn remove_rule() -> bool {
    as_admin(&format!("/bin/rm -f {SUDOERS_PATH}"))
}

/// Battery percent and whether we're running on it, or None on a Mac with no battery.
fn battery() -> Option<(u8, bool)> {
    parse_battery(&run(PMSET, &["-g", "batt"]).1)
}

fn parse_battery(pmset_batt: &str) -> Option<(u8, bool)> {
    let line = pmset_batt.lines().find(|l| l.contains("InternalBattery"))?;
    let percent = line
        .split_whitespace()
        .find_map(|w| w.trim_end_matches(';').strip_suffix('%'))?
        .parse()
        .ok()?;
    Some((percent, pmset_batt.contains("'Battery Power'")))
}

/// Why Lidless should switch itself off right now, or None if it's safe to stay awake.
pub fn safety_reason() -> Option<String> {
    let thermal = NSProcessInfo::processInfo().thermalState();
    if thermal == NSProcessInfoThermalState::Serious || thermal == NSProcessInfoThermalState::Critical {
        return Some("Your Mac got too hot".into());
    }
    match battery() {
        Some((percent, true)) if percent <= LOW_BATTERY => Some(format!("Battery is down to {percent}%")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sleep_disabled() {
        assert!(parse_sleep_disabled("System-wide power settings:\n SleepDisabled\t\t1\n"));
        assert!(!parse_sleep_disabled("System-wide power settings:\n SleepDisabled\t\t0\n"));
        assert!(!parse_sleep_disabled("Currently in use:\n standby 1\n"));
    }

    #[test]
    fn reads_battery() {
        let on_battery = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=23855203)\t18%; discharging; 1:02 remaining present: true\n";
        assert_eq!(parse_battery(on_battery), Some((18, true)));
        let plugged = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=23855203)\t100%; charged; 0:00 remaining present: true\n";
        assert_eq!(parse_battery(plugged), Some((100, false)));
        assert_eq!(parse_battery("Now drawing from 'AC Power'\n"), None);
    }

    #[test]
    fn only_plain_usernames_reach_sudoers() {
        assert!(valid_user("jay"));
        assert!(valid_user("first.last-2_x"));
        for bad in ["", "a b", "a'b", "x\ny ALL=(ALL) NOPASSWD: ALL", "jäy", "a,b", "a=b"] {
            assert_eq!(install_script(bad, true), "/usr/bin/pmset -a disablesleep 1", "{bad:?}");
        }
    }

    /// `sh -n` parses without running anything, so this is safe outside root.
    #[test]
    fn install_script_is_valid_shell() {
        for user in ["jay", "not a user"] {
            let checked = Command::new("/bin/sh").args(["-n", "-c", &install_script(user, true)]).status();
            assert!(checked.unwrap().success(), "{user:?}");
        }
    }

    #[test]
    fn rule_allows_exactly_two_commands() {
        let script = install_script("jay", false);
        assert!(script.starts_with("/usr/bin/pmset -a disablesleep 0 && "));
        assert!(script.contains(
            "'jay ALL=(root) NOPASSWD: /usr/bin/pmset -a disablesleep 0, /usr/bin/pmset -a disablesleep 1'"
        ));
        // A rule that fails any step is deleted, never left behind or half-installed.
        assert!(script.ends_with(
            "/bin/mv -f \"$T\" /etc/sudoers.d/lidless || { /bin/rm -f \"$T\"; false; }; }"
        ));
    }
}
