//! Which window may call which app command. The app's commands aren't in a Tauri app manifest,
//! so a capability can't scope them; each command checks its caller's window label instead:
//! the bubble page can't reach the `signin_*` commands (keys, sign-in codes, sign-out) and the
//! Sign in… page can't answer approval cards.

/// `caller` (the calling window's label) is `want`; else the call is refused and logged.
pub fn is(caller: &str, want: &str, cmd: &str) -> bool {
    let ok = caller == want;
    if !ok {
        eprintln!("[buddy] refused {cmd} from window {caller:?}");
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_named_window_may_call() {
        assert!(is("signin", "signin", "signin_save_key"));
        assert!(!is("bubble", "signin", "signin_save_key"));
        assert!(!is("signin", "bubble", "card_answer"));
        assert!(!is("", "signin", "signin_state"));
    }
}
