//! Golden snapshots of the section: `BLESS=1 cargo test -p mascot-core persona::golden` rewrites them.
//! A change to the wording shows up as a diff of these files, to be read before it is blessed.

use std::path::PathBuf;

use super::*;
use crate::pack;
use crate::test_pack::repo;

fn golden(name: &str, actual: &str) {
    let path: PathBuf = repo().join("crates/mascot-core/tests/fixtures").join(name);
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{name} is missing: run with BLESS=1"));
    assert_eq!(actual, want, "{name} differs (BLESS=1 rewrites it)");
}

fn section(user: UserPersona) -> String {
    let p = pack::load(&repo().join("mascots/bear")).unwrap();
    prompt_text(&p, &user).unwrap()
}

#[test]
fn biscuit() {
    golden("persona_biscuit.txt", &section(UserPersona::default()));
}

#[test]
fn mochi() {
    golden("persona_mochi.txt", &section(UserPersona { preset: Some("mochi".into()), ..Default::default() }));
}

#[test]
fn biscuit_customized_with_notes() {
    let user = UserPersona {
        name: Some("Bun".into()),
        call_me: Some("Sam".into()),
        catchphrase: Some("called it".into()),
        chattiness: Some(0.2),
        playfulness: Some(0.9),
        emotes: Some(0.1),
        notes: Some("No jokes about my code reviews.\nCall bugs critters.".into()),
        ..Default::default()
    };
    golden("persona_biscuit_custom.txt", &section(user));
}
