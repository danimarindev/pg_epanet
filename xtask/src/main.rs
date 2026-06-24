mod changelog;
mod github;
mod release;

use clap::{Parser, Subcommand};

use changelog::Version;

#[derive(Parser)]
#[command(name = "xtask", about = "pg_epanet release tooling")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Full release workflow (version bump, SQL, CHANGELOG, tests, commit, tag, push, GitHub)
    Release {
        version: String,
        #[arg(long)]
        create_github_release: bool,
        #[arg(long)]
        github_release_only: bool,
        /// Non-interactive: auto-push and honour --create-github-release
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Move [Unreleased] content to a versioned CHANGELOG section
    CloseUnreleased {
        version: String,
        #[arg(long)]
        date: Option<String>,
    },
    /// Print GitHub release notes from CHANGELOG.md
    ReleaseNotes {
        version: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// Create a GitHub Release from an existing CHANGELOG section
    GithubRelease {
        version: String,
        #[arg(long)]
        dry_run: bool,
    },
}

fn parse_version(raw: &str) -> Version {
    Version::parse(raw).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    })
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Release {
            version,
            create_github_release,
            github_release_only,
            yes,
        } => release::run(release::ReleaseOptions {
            version: parse_version(&version),
            create_github_release,
            github_release_only,
            yes,
        }),
        Command::CloseUnreleased { version, date } => {
            release::close_unreleased(&parse_version(&version), date.as_deref())
        }
        Command::ReleaseNotes { version, dry_run } => {
            release::print_release_notes(&parse_version(&version), dry_run)
        }
        Command::GithubRelease { version, dry_run } => {
            release::github_release_only(&parse_version(&version), dry_run)
        }
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
