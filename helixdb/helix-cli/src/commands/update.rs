use color_eyre::owo_colors::OwoColorize;
use eyre::Result;
use self_update::cargo_crate_version;

use crate::output::{Operation, Step, Verbosity};
use crate::utils::print_error_with_hint;

pub async fn run(force: bool) -> Result<()> {
    // We're using the self_update crate which is very handy but doesn't support async.
    // Still, this is good enough, but because it panics in an async context we must
    // do a spawn_blocking
    tokio::task::spawn_blocking(move || run_sync(force)).await?
}

fn run_sync(force: bool) -> Result<()> {
    let op = Operation::new("Updating", "CLI");

    let mut check_step = Step::with_messages("Checking for updates", "Checked for updates");
    check_step.start();

    let status = self_update::backends::github::Update::configure()
        .repo_owner("HelixDB")
        .repo_name("helix-db")
        .bin_name("helix")
        .show_download_progress(true)
        .show_output(false)
        .no_confirm(true)
        .current_version(cargo_crate_version!())
        .build()?;

    let current_version = cargo_crate_version!();
    let latest_release = status.get_latest_release()?;

    if !force {
        if latest_release.version == current_version {
            check_step.done_with_info("already up to date");
            op.success();
            println!("  Use --force to reinstall");
            return Ok(());
        }

        check_step.done_with_info(&format!(
            "v{current_version} -> v{}",
            latest_release.version
        ));
    } else {
        check_step.done_with_info("force update");
    }

    if is_v3_update(current_version, &latest_release.version) {
        print_v3_update_warning();
    }

    let mut install_step =
        Step::with_messages("Downloading and installing", "Downloaded and installed");
    install_step.start();

    match status.update() {
        Ok(_) => {
            install_step.done();
            op.success();
            if Verbosity::current().show_normal() {
                Operation::print_details(&[(
                    "Note",
                    "Please restart your terminal to use the new version",
                )]);
            }
        }
        Err(e) => {
            install_step.fail();
            op.failure();
            print_error_with_hint(
                &format!("Update failed: {e}"),
                "check your internet connection and try again",
            );
            return Err(e.into());
        }
    }

    Ok(())
}

fn is_v3_update(current_version: &str, latest_version: &str) -> bool {
    let current_version = current_version.trim_start_matches('v');
    let latest_version = latest_version.trim_start_matches('v');

    !current_version.starts_with("3.") && latest_version.starts_with("3.")
}

fn print_v3_update_warning() {
    if !Verbosity::current().show_normal() {
        return;
    }

    println!();
    println!(
        "{}",
        "WARNING: Updating from this version will update to v3, which is a breaking change."
            .yellow()
            .bold()
    );
    println!(
        "{}",
        "All existing databases will cease to work and only the Helix v2 DB will be available."
            .yellow()
            .bold()
    );
    println!(
        "{}",
        "For more information, see https://docs.helix-db.com"
            .yellow()
            .bold()
    );
}
