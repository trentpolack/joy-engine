// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Launch parameters: the page's URL query in browsers (`?capture=1&tick=48`) and `--name=value` arguments natively, so
//! a game exposes the same development switches (capture scenarios, fixtures) on every target. The shell itself reads
//! `backend` this way in browsers.

/// The value of launch parameter `name`: the URL query parameter in browsers, or the native command line's
/// `--name=value` (a bare `--name` reads as an empty string). `None` when absent.
pub fn launch_parameter(name: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        crate::web::query_parameter(name)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        argument_parameter(std::env::args().skip(1), name)
    }
}

/// Find `--name=value` or a bare `--name` among command-line arguments.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn argument_parameter(arguments: impl IntoIterator<Item = String>, name: &str) -> Option<String> {
    let flag = format!("--{name}");
    let prefix = format!("--{name}=");
    arguments.into_iter().find_map(|argument| {
        if let Some(value) = argument.strip_prefix(&prefix) {
            return Some(value.to_owned());
        }
        (argument == flag).then(String::new)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_arguments_supply_named_values_and_flags() {
        let arguments = || ["--capture", "--tick=48", "--faction=nature", "positional"].map(String::from);
        assert_eq!(argument_parameter(arguments(), "capture").as_deref(), Some(""));
        assert_eq!(argument_parameter(arguments(), "tick").as_deref(), Some("48"));
        assert_eq!(argument_parameter(arguments(), "faction").as_deref(), Some("nature"));
        assert_eq!(argument_parameter(arguments(), "burst"), None);
        assert_eq!(argument_parameter(arguments(), "tic"), None);
    }
}
