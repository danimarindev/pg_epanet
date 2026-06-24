use regex::Regex;

const DEFAULT_REPO_URL: &str = "https://github.com/danimarindev/pg_epanet";
const DEFAULT_GH_REPO: &str = "danimarindev/pg_epanet";
const VERSION_RE: &str = r"[0-9]+\.[0-9]+\.[0-9]+(?:-[a-zA-Z0-9.]+)?";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub text: String,
}

impl Version {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let cleaned = raw.trim().trim_start_matches('v');
        let re = Regex::new(&format!("^{VERSION_RE}$")).unwrap();
        if re.is_match(cleaned) {
            Ok(Self {
                text: cleaned.to_string(),
            })
        } else {
            Err(format!(
                "invalid version '{raw}' — expected semver like 0.1.0 or 0.1.0-rc.1"
            ))
        }
    }

    pub fn tag(&self) -> String {
        format!("v{}", self.text)
    }

    pub fn is_prerelease(&self) -> bool {
        self.text.contains('-')
    }
}

fn heading_re() -> Regex {
    Regex::new(&format!(
        r"(?m)^## \[({VERSION_RE}|Unreleased)\](?: [—-] .*)?$"
    ))
    .unwrap()
}

pub fn detect_repo_url(changelog: &str) -> String {
    let re = Regex::new(r"(?mi)^\[unreleased\]:\s*(https://github\.com/[^/]+/[^/]+)/compare/")
        .unwrap();
    re.captures(changelog)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| DEFAULT_REPO_URL.to_string())
}

pub fn detect_gh_repo(repo_url: &str) -> String {
    Regex::new(r"https://github\.com/([^/]+/[^/]+)")
        .unwrap()
        .captures(repo_url)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| DEFAULT_GH_REPO.to_string())
}

fn find_headings(changelog: &str) -> Vec<(String, usize)> {
    heading_re()
        .captures_iter(changelog)
        .map(|cap| (cap[1].to_string(), cap.get(0).unwrap().start()))
        .collect()
}

fn release_versions_from_headings(changelog: &str) -> Result<Vec<Version>, String> {
    find_headings(changelog)
        .into_iter()
        .filter(|(name, _)| name != "Unreleased")
        .map(|(name, _)| Version::parse(&name))
        .collect()
}

fn split_changelog_link_block(changelog: &str) -> Result<(String, String), String> {
    let re = Regex::new(r"(?si)\n\[unreleased\]: .*\z").unwrap();
    let Some(cap) = re.find(changelog) else {
        return Err("CHANGELOG.md has no [unreleased] link block at the bottom".into());
    };
    Ok((
        format!("{}\n", changelog[..cap.start()].trim_end()),
        changelog[cap.start() + 1..].to_string(),
    ))
}

fn rebuild_changelog_links(
    changelog_body: &str,
    repo_url: &str,
    unreleased_base: &str,
) -> Result<String, String> {
    let versions = release_versions_from_headings(changelog_body)?;
    if versions.is_empty() {
        return Err("cannot rebuild changelog links without release headings".into());
    }

    let mut lines = vec![format!(
        "[unreleased]: {repo_url}/compare/{}...{unreleased_base}",
        versions[0].tag()
    )];

    for (idx, current) in versions.iter().enumerate() {
        if idx + 1 < versions.len() {
            let previous = versions[idx + 1].tag();
            lines.push(format!(
                "[{}]: {repo_url}/compare/{previous}...{}",
                current.text,
                current.tag()
            ));
        } else {
            lines.push(format!(
                "[{}]: {repo_url}/releases/tag/{}",
                current.text,
                current.tag()
            ));
        }
    }

    Ok(format!(
        "{}\n\n{}\n",
        changelog_body.trim_end(),
        lines.join("\n")
    ))
}

pub fn close_unreleased_section(
    changelog: &str,
    version: &Version,
    release_date: &str,
) -> Result<(String, String), String> {
    let repo_url = detect_repo_url(changelog);
    let (body, _links) = split_changelog_link_block(changelog)?;

    if body.contains(&format!("## [{}]", version.text)) {
        return Err(format!(
            "CHANGELOG.md already contains section {}",
            version.text
        ));
    }

    let headings = find_headings(&body);
    if headings.first().map(|(n, _)| n.as_str()) != Some("Unreleased") {
        return Err("CHANGELOG.md must start its entries with ## [Unreleased]".into());
    }

    let unreleased_start = headings[0].1;
    let header_re = Regex::new(r"(?m)^## \[Unreleased\]\n").unwrap();
    let header_match = header_re
        .find(&body[unreleased_start..])
        .ok_or("could not parse ## [Unreleased] header")?;
    let header_len = header_match.as_str().len();
    let prefix = &body[..unreleased_start];

    let (unreleased_content, new_body) = if headings.len() == 1 {
        let content = body[unreleased_start + header_len..].trim().to_string();
        let new_body = format!(
            "{prefix}## [Unreleased]\n\n## [{ver}] — {release_date}\n\n{content}\n\n",
            ver = version.text
        );
        (content, new_body)
    } else {
        let next_start = headings[1].1;
        let content = body[unreleased_start + header_len..next_start]
            .trim()
            .to_string();
        let suffix = body[next_start..].trim_start_matches('\n');
        let new_body = format!(
            "{prefix}## [Unreleased]\n\n## [{ver}] — {release_date}\n\n{content}\n\n{suffix}",
            ver = version.text
        );
        (content, new_body)
    };

    if unreleased_content.is_empty() {
        return Err("Unreleased section is empty; nothing to release".into());
    }

    let new_changelog = rebuild_changelog_links(&new_body, &repo_url, "main")?;
    Ok((new_changelog, unreleased_content))
}

fn extract_release_section(changelog: &str, version: &Version) -> Result<String, String> {
    let (body, _links) = split_changelog_link_block(changelog)?;
    let pattern = Regex::new(&format!(
        r"(?m)^## \[{}\] [—-] .*$",
        regex::escape(&version.text)
    ))
    .unwrap();
    let Some(header) = pattern.find(&body) else {
        return Err(format!(
            "could not find CHANGELOG.md section for {}",
            version.text
        ));
    };

    let rest = &body[header.end()..];
    let next_re = Regex::new(r"(?m)^## \[").unwrap();
    let end = next_re.find(rest).map(|m| m.start()).unwrap_or(rest.len());
    let section = rest[..end].trim();
    if section.is_empty() {
        return Err(format!(
            "CHANGELOG.md section for {} is empty",
            version.text
        ));
    }
    Ok(section.to_string())
}

fn previous_release_for(changelog: &str, version: &Version) -> Result<Option<Version>, String> {
    let versions = release_versions_from_headings(changelog)?;
    for (idx, current) in versions.iter().enumerate() {
        if current == version {
            return Ok(versions.get(idx + 1).cloned());
        }
    }
    Err(format!("CHANGELOG.md has no section for {}", version.text))
}

pub fn release_notes(changelog: &str, version: &Version) -> Result<String, String> {
    let repo_url = detect_repo_url(changelog);
    let section = extract_release_section(changelog, version)?;
    let previous = previous_release_for(changelog, version)?;

    let mut notes = format!(
        "## 🔽 Read the summarized changelog here 🔽\n\n{section}\n\n"
    );
    if let Some(prev) = previous {
        notes.push_str(&format!(
            "**Full Changelog**: {repo_url}/compare/{}...{}\n",
            prev.tag(),
            version.tag()
        ));
    } else {
        notes.push_str(&format!(
            "**Full Changelog**: {repo_url}/releases/tag/{}\n",
            version.tag()
        ));
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# Changelog

## [Unreleased]

### Added
- New thing

## [0.1.0] — 2026-06-24

### Notes
- Stable

[unreleased]: https://github.com/danimarindev/pg_epanet/compare/v0.1.0...main
[0.1.0]: https://github.com/danimarindev/pg_epanet/releases/tag/v0.1.0
"#;

    #[test]
    fn close_unreleased_moves_content() {
        let ver = Version::parse("0.2.0").unwrap();
        let (new, body) = close_unreleased_section(SAMPLE, &ver, "2026-06-25").unwrap();
        assert_eq!(body, "### Added\n- New thing");
        assert!(new.contains("## [0.2.0] — 2026-06-25"));
        assert!(new.contains("[unreleased]: https://github.com/danimarindev/pg_epanet/compare/v0.2.0...main"));
    }

    #[test]
    fn release_notes_include_section_body() {
        let ver = Version::parse("0.1.0").unwrap();
        let notes = release_notes(SAMPLE, &ver).unwrap();
        assert!(notes.contains("Stable"));
        assert!(notes.contains("releases/tag/v0.1.0"));
    }
}
