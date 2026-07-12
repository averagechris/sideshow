use std::{fs, path::Path, process::Command};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sideshow"))
}

fn write_user_pack(root: &Path, theme_name: &str, css: &str) {
    fs::create_dir_all(root.join("themes")).unwrap();
    fs::write(
        root.join("pack.toml"),
        format!(
            "schema_version=1\npack='user-pack'\n[[themes]]\nname='{theme_name}'\ncss='themes/{theme_name}.css'\n"
        ),
    )
    .unwrap();
    fs::write(root.join(format!("themes/{theme_name}.css")), css).unwrap();
}

#[test]
fn user_roots_are_relative_discovery_only_and_default_theme_is_copied() {
    let t = tempfile::tempdir().unwrap();
    let config_dir = t.path().join("cfg");
    let pack = config_dir.join("packs/one");
    fs::create_dir_all(&config_dir).unwrap();
    write_user_pack(&pack, "user-theme", "body{color:orange}");
    let config = config_dir.join("config.toml");
    fs::write(
        &config,
        "[registry]\nroots=['packs/one']\n[authoring]\ndefault-theme='user-theme'\n",
    )
    .unwrap();

    let sources = bin()
        .env("SIDESHOW_CONFIG", &config)
        .args(["registry", "sources"])
        .output()
        .unwrap();
    assert!(
        sources.status.success(),
        "{}",
        String::from_utf8_lossy(&sources.stderr)
    );
    let stdout = String::from_utf8_lossy(&sources.stdout);
    assert!(stdout.contains("user:0"));
    assert!(stdout.contains("\"activated\": false"));

    let deck = t.path().join("deck");
    let new = bin()
        .env("SIDESHOW_CONFIG", &config)
        .args(["new", deck.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        new.status.success(),
        "{}",
        String::from_utf8_lossy(&new.stderr)
    );
    assert_eq!(
        fs::read_to_string(deck.join("theme.css")).unwrap(),
        "body{color:orange}"
    );
    fs::write(pack.join("themes/user-theme.css"), "body{color:black}").unwrap();
    assert_eq!(
        fs::read_to_string(deck.join("theme.css")).unwrap(),
        "body{color:orange}"
    );
}

#[test]
fn explicit_cli_theme_overrides_config_and_apply_theme_is_independent() {
    let t = tempfile::tempdir().unwrap();
    let config_dir = t.path().join("cfg");
    let pack = config_dir.join("packs/one");
    fs::create_dir_all(&config_dir).unwrap();
    write_user_pack(&pack, "user-theme", "body{color:orange}");
    let config = config_dir.join("config.toml");
    fs::write(
        &config,
        "[registry]\nroots=['packs/one']\n[authoring]\ndefault-theme='user-theme'\n",
    )
    .unwrap();

    let deck = t.path().join("deck");
    let new = bin()
        .env("SIDESHOW_CONFIG", &config)
        .args(["new", deck.to_str().unwrap(), "--theme", "ledger"])
        .output()
        .unwrap();
    assert!(
        new.status.success(),
        "{}",
        String::from_utf8_lossy(&new.stderr)
    );
    assert_ne!(
        fs::read_to_string(deck.join("theme.css")).unwrap(),
        "body{color:orange}"
    );

    let applied = bin()
        .env("SIDESHOW_CONFIG", &config)
        .args([
            "registry",
            "apply-theme",
            "--deck",
            deck.to_str().unwrap(),
            "user-theme",
            "--force",
        ])
        .output()
        .unwrap();
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    assert_eq!(
        fs::read_to_string(deck.join("theme.css")).unwrap(),
        "body{color:orange}"
    );
    fs::write(pack.join("themes/user-theme.css"), "body{color:black}").unwrap();
    assert_eq!(
        fs::read_to_string(deck.join("theme.css")).unwrap(),
        "body{color:orange}"
    );
}

#[test]
fn invalid_user_root_fails_without_mutating_deck() {
    let t = tempfile::tempdir().unwrap();
    let config = t.path().join("config.toml");
    fs::write(
        &config,
        "[registry]\nroots=['missing']\n[authoring]\ndefault-theme='bad'\n",
    )
    .unwrap();
    let deck = t.path().join("deck");
    let out = bin()
        .env("SIDESHOW_CONFIG", &config)
        .args(["new", deck.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!deck.exists());
}

#[test]
fn absent_config_parent_preserves_bundled_registry_and_explicit_themes_skip_config() {
    let t = tempfile::tempdir().unwrap();
    let missing_config = t.path().join("missing/parent/config.toml");

    for args in [vec!["registry", "list"], vec!["registry", "sources"]] {
        let out = bin()
            .env("SIDESHOW_CONFIG", &missing_config)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let malformed = t.path().join("malformed.toml");
    fs::write(&malformed, "not = [valid").unwrap();
    for (subcommand, deck_name) in [("new", "deck"), ("plan", "plan")] {
        let deck = t.path().join(deck_name);
        let mut command = bin();
        command.env("SIDESHOW_CONFIG", &malformed);
        if subcommand == "plan" {
            command.args(["plan", "new", deck.to_str().unwrap(), "--theme", "signal"]);
        } else {
            command.args(["new", deck.to_str().unwrap(), "--theme", "signal"]);
        }
        let out = command.output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn deck_registry_excludes_discovery_only_user_entries() {
    let t = tempfile::tempdir().unwrap();
    let config_dir = t.path().join("cfg");
    let pack = config_dir.join("packs/one");
    fs::create_dir_all(&config_dir).unwrap();
    write_user_pack(&pack, "user-theme", "body{color:orange}");
    let config = config_dir.join("config.toml");
    fs::write(&config, "[registry]\nroots=['packs/one']\n").unwrap();
    let deck = t.path().join("deck");
    assert!(
        bin()
            .args(["new", deck.to_str().unwrap(), "--theme", "signal"])
            .status()
            .unwrap()
            .success()
    );

    let global = bin()
        .env("SIDESHOW_CONFIG", &config)
        .args(["registry", "list"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&global.stdout).contains("user-theme"));
    let effective = bin()
        .env("SIDESHOW_CONFIG", &config)
        .args(["registry", "list", "--deck", deck.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(effective.status.success());
    assert!(!String::from_utf8_lossy(&effective.stdout).contains("user-theme"));
}
