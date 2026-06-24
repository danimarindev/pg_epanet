use std::fs;
use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::changelog::{Version, detect_gh_repo, detect_repo_url, release_notes};

#[derive(Serialize)]
struct ReleasePayload<'a> {
    tag_name: &'a str,
    name: &'a str,
    body: &'a str,
    draft: bool,
    prerelease: bool,
}

pub fn create_github_release(
    version: &Version,
    changelog: &str,
    changelog_path: &Path,
    dry_run: bool,
) -> Result<(), String> {
    let _ = changelog_path;
    let repo_url = detect_repo_url(changelog);
    let gh_repo = detect_gh_repo(&repo_url);
    let notes = release_notes(changelog, version)?;

    let mut gh_args = vec![
        "release".to_string(),
        "create".to_string(),
        version.tag(),
        "--title".to_string(),
        version.tag(),
    ];
    if version.is_prerelease() {
        gh_args.push("--prerelease".to_string());
    }

    if dry_run {
        println!("--- GitHub release notes ---");
        print!("{notes}");
        println!("--- End GitHub release notes ---");
        println!(
            "$ gh {} --notes-file /tmp/release-notes.md",
            gh_args.join(" ")
        );
        return Ok(());
    }

    if Command::new("gh")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        let notes_file = std::env::temp_dir().join(format!("pg_epanet-release-{}.md", version.text));
        fs::write(&notes_file, &notes).map_err(|e| e.to_string())?;
        gh_args.push("--notes-file".to_string());
        gh_args.push(notes_file.to_string_lossy().into_owned());

        let output = Command::new("gh")
            .args(&gh_args)
            .output()
            .map_err(|e| format!("failed to run gh: {e}"))?;
        let _ = fs::remove_file(&notes_file);

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("gh release create failed\n{stdout}{stderr}"));
        }
        println!("Created GitHub Release: {}", version.tag());
        return Ok(());
    }

    create_github_release_with_api(version, &notes, &gh_repo)
}

fn create_github_release_with_api(
    version: &Version,
    notes: &str,
    gh_repo: &str,
) -> Result<(), String> {
    let token = std::env::var("GITHUB_TOKEN").map_err(|_| {
        "gh is not installed. Install gh or export GITHUB_TOKEN to create the GitHub Release via API."
            .to_string()
    })?;

    let payload = ReleasePayload {
        tag_name: &version.tag(),
        name: &version.tag(),
        body: notes,
        draft: false,
        prerelease: version.is_prerelease(),
    };

    let response = ureq::post(&format!("https://api.github.com/repos/{gh_repo}/releases"))
        .set("Accept", "application/vnd.github+json")
        .set("Authorization", &format!("Bearer {token}"))
        .set("X-GitHub-Api-Version", "2022-11-28")
        .send_json(payload)
        .map_err(|e| format!("GitHub release API failed: {e}"))?;

    let release: serde_json::Value = response.into_json().map_err(|e| e.to_string())?;
    let tag = version.tag();
    let url = release
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or(&tag);
    println!("Created GitHub Release: {url}");
    Ok(())
}
