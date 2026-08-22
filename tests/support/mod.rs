use std::{os::unix::fs::PermissionsExt, path::PathBuf, process::Command, sync::OnceLock};

struct HermeticTools {
    _temp: tempfile::TempDir,
    tailwindcss: PathBuf,
}

fn tools() -> &'static HermeticTools {
    static TOOLS: OnceLock<HermeticTools> = OnceLock::new();

    TOOLS.get_or_init(|| {
        let temp = tempfile::tempdir().unwrap();
        let tailwindcss = temp.path().join("tailwindcss");
        std::fs::write(
            &tailwindcss,
            r#"#!/bin/sh
input=''
output=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    -i) input=$2; shift 2 ;;
    -o) output=$2; shift 2 ;;
    *) shift ;;
  esac
done
: > "$output"
while IFS= read -r line || [ -n "$line" ]; do
  printf '%s\n' "$line" >> "$output"
done < "$input"
"#,
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&tailwindcss).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&tailwindcss, permissions).unwrap();
        HermeticTools {
            _temp: temp,
            tailwindcss,
        }
    })
}

pub fn sideshow() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sideshow"));
    command
        .env_remove("SIDESHOW_CONFIG")
        .env("SIDESHOW_TAILWINDCSS", &tools().tailwindcss);
    command
}
