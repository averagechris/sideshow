use std::{fs, path::Path, process::Command};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sideshow"))
}

fn write_pack(deck: &Path) {
    fs::create_dir_all(deck.join("slides")).unwrap();
    fs::create_dir_all(deck.join("packs/local/components")).unwrap();
    fs::create_dir_all(deck.join("packs/local/themes")).unwrap();
    fs::write(
        deck.join("deck.toml"),
        "[deck]\ntitle='Pack Test'\n[packs]\nroots=['packs/local']\n",
    )
    .unwrap();
    fs::write(deck.join("theme.css"), "body{color:black}").unwrap();
    fs::write(
        deck.join("packs/local/pack.toml"),
        "schema_version=1\npack='local'\n[[components]]\nname='safe-card'\ntemplate='components/card.html'\ncss='components/card.css'\nprops=['title']\ncapabilities=['js-free']\n[[themes]]\nname='local-theme'\ncss='themes/local.css'\n",
    )
    .unwrap();
    fs::write(
        deck.join("packs/local/components/card.html"),
        "<article><h1>{{title}}</h1></article>",
    )
    .unwrap();
    fs::write(
        deck.join("packs/local/components/card.css"),
        ".safe-card{color:purple}",
    )
    .unwrap();
    fs::write(
        deck.join("packs/local/themes/local.css"),
        "body{color:green}",
    )
    .unwrap();
}

#[test]
fn project_component_build_discovery_compose_and_theme_apply_work() {
    let t = tempfile::tempdir().unwrap();
    write_pack(t.path());
    let source = t.path().join("slides/01.slide.toml");
    let out = bin()
        .args([
            "compose",
            "add",
            "--deck",
            t.path().to_str().unwrap(),
            source.to_str().unwrap(),
            "--component",
            "safe-card",
            "--prop",
            "title=<b>x</b>",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("project:0"));

    let list = bin()
        .args(["registry", "list", "--deck", t.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(list.status.success());
    assert!(String::from_utf8_lossy(&list.stdout).contains("safe-card"));
    assert!(String::from_utf8_lossy(&list.stdout).contains("sha256:"));

    let build = bin()
        .args(["build", t.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let html = fs::read_to_string(t.path().join("dist/pack-test.html")).unwrap();
    assert!(html.contains("&lt;b&gt;x&lt;/b&gt;"));
    assert!(html.contains("color:purple"));

    let no_force = bin()
        .args([
            "registry",
            "apply-theme",
            "--deck",
            t.path().to_str().unwrap(),
            "local-theme",
        ])
        .output()
        .unwrap();
    assert!(!no_force.status.success());
    let applied = bin()
        .args([
            "registry",
            "apply-theme",
            "--deck",
            t.path().to_str().unwrap(),
            "local-theme",
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
        fs::read_to_string(t.path().join("theme.css")).unwrap(),
        "body{color:green}"
    );
    fs::write(
        t.path().join("packs/local/themes/local.css"),
        "body{color:red}",
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(t.path().join("theme.css")).unwrap(),
        "body{color:green}"
    );
}

#[cfg(unix)]
#[test]
fn project_pack_rejects_root_intermediate_and_file_symlinks() {
    use std::os::unix::fs::symlink;
    for case in ["root", "intermediate", "file"] {
        let t = tempfile::tempdir().unwrap();
        write_pack(t.path());
        match case {
            "root" => {
                fs::remove_dir_all(t.path().join("packs/local")).unwrap();
                symlink("/tmp", t.path().join("packs/local")).unwrap();
            }
            "intermediate" => {
                fs::remove_dir_all(t.path().join("packs/local/components")).unwrap();
                symlink("/tmp", t.path().join("packs/local/components")).unwrap();
            }
            "file" => {
                fs::remove_file(t.path().join("packs/local/components/card.html")).unwrap();
                symlink(
                    "/etc/passwd",
                    t.path().join("packs/local/components/card.html"),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let out = bin()
            .args(["registry", "list", "--deck", t.path().to_str().unwrap()])
            .output()
            .unwrap();
        assert!(!out.status.success(), "{case} unexpectedly succeeded");
        assert!(String::from_utf8_lossy(&out.stderr).contains("symlink"));
    }
}

#[test]
fn project_pack_rejects_collisions_runtime_names_and_unsafe_markup() {
    let t = tempfile::tempdir().unwrap();
    write_pack(t.path());
    fs::write(
        t.path().join("packs/local/components/card.html"),
        "<img src=\"https://x\" onload=\"x\">{{title}}",
    )
    .unwrap();
    let out = bin()
        .args(["registry", "list", "--deck", t.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("forbidden"));

    write_pack(t.path());
    fs::write(t.path().join("packs/local/pack.toml"), "schema_version=1\npack='local'\n[[components]]\nname='review-feedback'\ntemplate='components/card.html'\ncss='components/card.css'\nprops=['title']\ncapabilities=['js-free']\n").unwrap();
    let out = bin()
        .args(["registry", "list", "--deck", t.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("fixed runtime"));

    fs::write(t.path().join("packs/local/pack.toml"), "schema_version=1\npack='local'\n[[components]]\nname='literal-card'\ntemplate='components/card.html'\ncss='components/card.css'\nprops=['title']\ncapabilities=['js-free']\n").unwrap();
    let out = bin()
        .args(["registry", "list", "--deck", t.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("collision"));
}

#[test]
fn compose_rejects_invalid_project_props_without_writing() {
    let t = tempfile::tempdir().unwrap();
    write_pack(t.path());
    let source = t.path().join("slides/bad.slide.toml");
    let missing = bin()
        .args([
            "compose",
            "add",
            "--deck",
            t.path().to_str().unwrap(),
            source.to_str().unwrap(),
            "--component",
            "safe-card",
        ])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(!source.exists());
    fs::write(&source, "component='safe-card'\n[props]\ntitle='old'\n").unwrap();
    let before = fs::read_to_string(&source).unwrap();
    let unknown = bin()
        .args([
            "compose",
            "update",
            "--deck",
            t.path().to_str().unwrap(),
            source.to_str().unwrap(),
            "--component",
            "safe-card",
            "--prop",
            "title=ok",
            "--prop",
            "nope=x",
        ])
        .output()
        .unwrap();
    assert!(!unknown.status.success());
    assert_eq!(fs::read_to_string(&source).unwrap(), before);
    let bound = bin()
        .args([
            "compose",
            "update",
            "--deck",
            t.path().to_str().unwrap(),
            source.to_str().unwrap(),
            "--component",
            "safe-card",
            "--prop",
            "title=ok",
            "--bind-kind",
            "outcome",
            "--bind-id",
            "x",
        ])
        .output()
        .unwrap();
    assert!(!bound.status.success());
    assert_eq!(fs::read_to_string(&source).unwrap(), before);
}

#[test]
fn compose_remove_handles_project_sources_and_preserves_invalid() {
    let t = tempfile::tempdir().unwrap();
    write_pack(t.path());
    let source = t.path().join("slides/remove.slide.toml");
    fs::write(&source, "component='safe-card'\n[props]\ntitle='ok'\n").unwrap();
    let removed = bin()
        .args(["compose", "remove", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(!source.exists());

    fs::write(&source, "component='safe-card'\n[props]\nnope='bad'\n").unwrap();
    let invalid = bin()
        .args(["compose", "remove", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(
        source.exists(),
        "invalid project source must not be deleted"
    );
}

#[test]
fn project_pack_assets_are_declared_embedded_and_digest_inputs() {
    let t = tempfile::tempdir().unwrap();
    write_pack(t.path());
    fs::write(t.path().join("packs/local/pack.toml"), "schema_version=1\npack='local'\n[[components]]\nname='safe-card'\ntemplate='components/card.html'\ncss='components/card.css'\nprops=['title']\ncapabilities=['js-free']\n[[components.assets]]\npath='assets/icon.png'\n[[themes]]\nname='local-theme'\ncss='themes/local.css'\n[[themes.assets]]\npath='assets/bg.png'\n").unwrap();
    fs::create_dir_all(t.path().join("packs/local/assets")).unwrap();
    fs::write(t.path().join("packs/local/assets/icon.png"), b"icon").unwrap();
    fs::write(t.path().join("packs/local/assets/bg.png"), b"bg").unwrap();
    fs::write(
        t.path().join("packs/local/components/card.html"),
        "<article><img src='assets/icon.png'><h1>{{title}}</h1></article>",
    )
    .unwrap();
    fs::write(
        t.path().join("packs/local/components/card.css"),
        ".safe-card{background:url(assets/icon.png)}",
    )
    .unwrap();
    fs::write(
        t.path().join("packs/local/themes/local.css"),
        "body{background:url(assets/bg.png)}",
    )
    .unwrap();
    let source = t.path().join("slides/01.slide.toml");
    assert!(
        bin()
            .args([
                "compose",
                "add",
                "--deck",
                t.path().to_str().unwrap(),
                source.to_str().unwrap(),
                "--component",
                "safe-card",
                "--prop",
                "title=x"
            ])
            .status()
            .unwrap()
            .success()
    );
    let reg = bin()
        .args(["registry", "list", "--deck", t.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        reg.status.success(),
        "{}",
        String::from_utf8_lossy(&reg.stderr)
    );
    let stdout = String::from_utf8_lossy(&reg.stdout);
    assert!(stdout.contains("assets/icon.png"));
    assert!(stdout.contains("assets/bg.png"));
    let build = bin()
        .args(["build", t.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let html = fs::read_to_string(t.path().join("dist/pack-test.html")).unwrap();
    assert!(html.contains("data:image/png;base64,aWNvbg=="), "{html}");
    assert!(!html.contains("assets/icon.png"));
    let applied = bin()
        .args([
            "registry",
            "apply-theme",
            "--deck",
            t.path().to_str().unwrap(),
            "local-theme",
            "--force",
        ])
        .output()
        .unwrap();
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    assert!(
        fs::read_to_string(t.path().join("theme.css"))
            .unwrap()
            .contains("data:image/png;base64,Ymc=")
    );
    let applied_json = String::from_utf8_lossy(&applied.stdout);
    assert!(applied_json.contains("written_digest"));
    assert!(applied_json.contains("source_digest"));
}

#[test]
fn project_template_rejects_attribute_placeholders_and_css_urls() {
    let t = tempfile::tempdir().unwrap();
    write_pack(t.path());
    for template in [
        "<div title=\"{{title}}\">x</div>",
        "<div title=\"not > closed {{title}}\">x</div>",
        "<div title={{title}}>x</div>",
        "<div\n data-x = \"{{title}}\">x</div>",
    ] {
        fs::write(t.path().join("packs/local/components/card.html"), template).unwrap();
        let out = bin()
            .args(["registry", "list", "--deck", t.path().to_str().unwrap()])
            .output()
            .unwrap();
        assert!(!out.status.success(), "template passed: {template}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("placeholder inside HTML tag"));
    }
    fs::write(
        t.path().join("packs/local/components/card.html"),
        "<p>{{title}}</p>",
    )
    .unwrap();
    for css in [
        ".x{background: U R L (https://x)}",
        r".x{background:u\72l(https://x)}",
        ".x{background:u/**/rl(https://x)}",
        r"@im\70ort 'https://x';",
        "@im/**/port 'https://x';",
    ] {
        fs::write(t.path().join("packs/local/components/card.css"), css).unwrap();
        let out = bin()
            .args(["registry", "list", "--deck", t.path().to_str().unwrap()])
            .output()
            .unwrap();
        assert!(!out.status.success(), "CSS passed: {css}");
    }
}

#[test]
fn pack_asset_rewrite_is_contextual_declared_and_deterministic() {
    let t = tempfile::tempdir().unwrap();
    write_pack(t.path());
    fs::write(t.path().join("packs/local/pack.toml"), "schema_version=1\npack='local'\n[[components]]\nname='safe-card'\ntemplate='components/card.html'\ncss='components/card.css'\nprops=['title']\ncapabilities=['js-free']\n[[components.assets]]\npath='assets/icon.png'\n[[components.assets]]\npath='assets/unused.png'\n").unwrap();
    fs::create_dir_all(t.path().join("packs/local/assets")).unwrap();
    fs::write(t.path().join("packs/local/assets/icon.png"), b"icon").unwrap();
    fs::write(t.path().join("packs/local/assets/unused.png"), b"unused").unwrap();
    fs::write(t.path().join("packs/local/components/card.html"), "<p>literal assets/icon.png text</p><img src='assets/icon.png?cache=1#x'><h1>{{title}}</h1>").unwrap();
    fs::write(
        t.path().join("packs/local/components/card.css"),
        ".safe-card{color:purple}",
    )
    .unwrap();
    let out = bin()
        .args(["registry", "list", "--deck", t.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("unused project pack asset(s): assets/unused.png")
    );

    fs::write(t.path().join("packs/local/pack.toml"), "schema_version=1\npack='local'\n[[components]]\nname='safe-card'\ntemplate='components/card.html'\ncss='components/card.css'\nprops=['title']\ncapabilities=['js-free']\n[[components.assets]]\npath='assets/icon.png'\n").unwrap();
    let out = bin()
        .args(["registry", "list", "--deck", t.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn pack_assets_participate_in_budgets_and_build_agrees() {
    let t = tempfile::tempdir().unwrap();
    write_pack(t.path());
    fs::write(t.path().join("packs/local/pack.toml"), "schema_version=1\npack='local'\n[[components]]\nname='safe-card'\ntemplate='components/card.html'\ncss='components/card.css'\nprops=['title']\ncapabilities=['js-free']\n[[components.assets]]\npath='assets/html.bin'\n[[components.assets]]\npath='assets/css.bin'\n[[themes]]\nname='local-theme'\ncss='themes/local.css'\n[[themes.assets]]\npath='assets/theme.bin'\n").unwrap();
    fs::create_dir_all(t.path().join("packs/local/assets")).unwrap();
    fs::write(
        t.path().join("packs/local/assets/html.bin"),
        vec![0u8; 390_000],
    )
    .unwrap();
    fs::write(
        t.path().join("packs/local/assets/css.bin"),
        vec![0u8; 390_000],
    )
    .unwrap();
    fs::write(
        t.path().join("packs/local/assets/theme.bin"),
        vec![0u8; 390_000],
    )
    .unwrap();
    fs::write(
        t.path().join("packs/local/components/card.html"),
        "<article><img src='assets/html.bin'><h1>{{title}}</h1></article>",
    )
    .unwrap();
    fs::write(
        t.path().join("packs/local/components/card.css"),
        ".safe-card{background:url(assets/css.bin)}",
    )
    .unwrap();
    fs::write(
        t.path().join("packs/local/themes/local.css"),
        "body{background:url(assets/theme.bin)}",
    )
    .unwrap();
    fs::write(
        t.path().join("slides/01.slide.toml"),
        "component='safe-card'\n[props]\ntitle='ok'\n",
    )
    .unwrap();
    let applied = bin()
        .args([
            "registry",
            "apply-theme",
            "--deck",
            t.path().to_str().unwrap(),
            "local-theme",
            "--force",
        ])
        .output()
        .unwrap();
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let check = bin()
        .args(["check", t.path().to_str().unwrap()])
        .output()
        .unwrap();
    let check_text = format!(
        "{}{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(check_text.contains("asset_size_budget"), "{check_text}");
    assert!(
        check_text.contains("pack component/safe-card/assets/html.bin"),
        "{check_text}"
    );
    assert!(
        check_text.contains("pack component/safe-card/assets/css.bin"),
        "{check_text}"
    );
    assert!(
        check_text.contains("pack theme/local-theme/assets/theme.bin"),
        "{check_text}"
    );
    let build = bin()
        .args(["build", t.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
}

#[test]
fn pack_rejects_svg_policy_navigation_entities_and_role_collisions() {
    let t = tempfile::tempdir().unwrap();
    write_pack(t.path());
    fs::write(t.path().join("packs/local/pack.toml"), "schema_version=1\npack='local'\n[[components]]\nname='safe-card'\ntemplate='components/card.html'\ncss='components/card.css'\nprops=['title']\ncapabilities=['js-free']\n[[components.assets]]\npath='components/card.css'\n").unwrap();
    let out = bin()
        .args(["registry", "list", "--deck", t.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("collides"));

    fs::write(t.path().join("packs/local/pack.toml"), "schema_version=1\npack='local'\n[[components]]\nname='safe-card'\ntemplate='components/card.html'\ncss='components/card.css'\nprops=['title']\ncapabilities=['js-free']\n[[components.assets]]\npath='assets/bad.SVG'\n").unwrap();
    fs::create_dir_all(t.path().join("packs/local/assets")).unwrap();
    fs::write(
        t.path().join("packs/local/assets/bad.SVG"),
        "<svg xmlns='http://www.w3.org/2000/svg'><script/></svg>",
    )
    .unwrap();
    fs::write(
        t.path().join("packs/local/components/card.html"),
        "<img src='assets/bad.SVG'><h1>{{title}}</h1>",
    )
    .unwrap();
    let out = bin()
        .args(["registry", "list", "--deck", t.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("static policy"));

    fs::write(t.path().join("packs/local/pack.toml"), "schema_version=1\npack='local'\n[[components]]\nname='safe-card'\ntemplate='components/card.html'\ncss='components/card.css'\nprops=['title']\ncapabilities=['js-free']\n").unwrap();
    for template in [
        "<a href='assets/file.bin'>x</a>",
        "<img src='java&#x73;cript:alert(1)'>",
        "<img on&#x6c;oad='alert(1)'>",
    ] {
        fs::write(t.path().join("packs/local/components/card.html"), template).unwrap();
        let out = bin()
            .args(["registry", "list", "--deck", t.path().to_str().unwrap()])
            .output()
            .unwrap();
        assert!(!out.status.success(), "template passed: {template}");
    }
}
