use anyhow::Result;
use predicates::str::contains;
use std::path::Path;
use tempfile::TempDir;

fn motyga_command(motyga_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(motyga_utils_cargo_bin::cargo_bin("motyga")?);
    cmd.env("MOTYGA_HOME", motyga_home);
    Ok(cmd)
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn update_does_not_start_interactive_prompt() -> Result<()> {
    let motyga_home = TempDir::new()?;

    motyga_command(motyga_home.path())?
        .arg("update")
        .assert()
        .failure()
        .stderr(contains("`motyga update` is not available in debug builds"));

    Ok(())
}
