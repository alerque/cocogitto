#![cfg(not(tarpaulin_include))]
mod cog_commit;

use std::path::PathBuf;

use cocogitto::conventional::changelog::template::{RemoteContext, Template};
use cocogitto::conventional::commit;
use cocogitto::conventional::version::VersionIncrement;
use cocogitto::git::hook::HookKind;
use cocogitto::log::filter::{CommitFilter, CommitFilters};
use cocogitto::log::output::Output;
use cocogitto::{CocoGitto, SETTINGS};

use anyhow::{Context, Result};
use cocogitto::git::revspec::RevspecPattern;
use structopt::clap::{AppSettings, Shell};
use structopt::StructOpt;

const APP_SETTINGS: &[AppSettings] = &[
    AppSettings::SubcommandRequiredElseHelp,
    AppSettings::UnifiedHelpMessage,
    AppSettings::ColoredHelp,
    AppSettings::VersionlessSubcommands,
    AppSettings::DeriveDisplayOrder,
];

const SUBCOMMAND_SETTINGS: &[AppSettings] = &[
    AppSettings::UnifiedHelpMessage,
    AppSettings::ColoredHelp,
    AppSettings::DeriveDisplayOrder,
];

fn hook_profiles() -> Vec<&'static str> {
    SETTINGS
        .bump_profiles
        .keys()
        .map(|profile| profile.as_ref())
        .collect()
}

/// A command line tool for the conventional commits and semver specifications
#[derive(StructOpt)]
#[structopt(name = "Cog", author = "Paul D. <paul.delafosse@protonmail.com>", settings = APP_SETTINGS)]
enum Cli {
    /// Verify all commit messages against the conventional commit specification
    #[structopt(no_version, settings = SUBCOMMAND_SETTINGS)]
    Check {
        /// Check commit history, starting from the latest tag to HEAD
        #[structopt(short = "l", long)]
        from_latest_tag: bool,
    },

    /// Create a new conventional commit
    Commit(CommitArgs),

    /// Interactively rename invalid commit messages
    #[structopt(no_version, settings = SUBCOMMAND_SETTINGS)]
    Edit {
        /// Edit non conventional commits, starting from the latest tag to HEAD
        #[structopt(short = "l", long)]
        from_latest_tag: bool,
    },

    /// Like git log but for conventional commits
    #[structopt(no_version, settings = SUBCOMMAND_SETTINGS)]
    Log {
        /// filter BREAKING CHANGE commits
        #[structopt(short = "B", long)]
        breaking_change: bool,

        /// filter on commit type
        #[structopt(short, long = "type", value_name = "type")]
        typ: Option<Vec<String>>,

        /// filter on commit author
        #[structopt(short, long)]
        author: Option<Vec<String>>,

        /// filter on commit scope
        #[structopt(short, long)]
        scope: Option<Vec<String>>,

        /// omit error on the commit log
        #[structopt(short = "e", long)]
        no_error: bool,
    },

    /// Verify a single commit message
    #[structopt(no_version, settings = SUBCOMMAND_SETTINGS)]
    Verify {
        /// The commit message
        message: String,
    },

    /// Display a changelog for the given commit oid range
    #[structopt(no_version, settings = SUBCOMMAND_SETTINGS)]
    Changelog {
        /// Generate the changelog from in the given spec range
        #[structopt(conflicts_with = "at")]
        pattern: Option<String>,

        /// Generate the changelog for a specific git tag
        #[structopt(short, long)]
        at: Option<String>,

        /// Generate the changelog with the given template.
        /// Possible values are 'remote', 'full_hash', 'default' or the path to your template.  
        /// If not specified cog will use cog.toml template config or fallback to 'default'.
        #[structopt(name = "template", long, short)]
        template: Option<String>,

        /// Url to use during template generation
        #[structopt(name = "remote", long, short, required_if("template", "remote"))]
        remote: Option<String>,

        /// Repository owner to use during template generation
        #[structopt(name = "owner", long, short, required_if("template", "remote"))]
        owner: Option<String>,

        /// Name of the repository used during template generation
        #[structopt(name = "repository", long, required_if("template", "remote"))]
        repository: Option<String>,
    },

    /// Commit changelog from latest tag to HEAD and create new tag
    #[structopt(no_version, settings = SUBCOMMAND_SETTINGS)]
    Bump {
        /// Manually set the next version
        #[structopt(short, long, required_unless_one = &["auto", "major", "minor", "patch"])]
        version: Option<String>,

        /// Automatically suggest the next version
        #[structopt(short, long, required_unless_one = &["version", "major", "minor", "patch"])]
        auto: bool,

        /// Increment the major version
        #[structopt(short = "M", long, required_unless_one = &["version", "auto", "minor", "patch"])]
        major: bool,

        /// Increment the minor version
        #[structopt(short, long, required_unless_one = &["version", "auto", "major", "patch"])]
        minor: bool,

        /// Increment the patch version
        #[structopt(short, long, required_unless_one = &["version", "auto", "major", "minor"])]
        patch: bool,

        /// Set the pre-release version
        #[structopt(long)]
        pre: Option<String>,

        /// Specify the bump profile hooks to run
        #[structopt(short, long, possible_values = &hook_profiles())]
        hook_profile: Option<String>,
    },

    /// Install cog config files
    #[structopt(no_version, settings = SUBCOMMAND_SETTINGS)]
    Init {
        /// path to init
        #[structopt(default_value = ".")]
        path: PathBuf,
    },

    /// Add git hooks to the repository
    #[structopt(no_version, settings = SUBCOMMAND_SETTINGS)]
    InstallHook {
        /// Type of hook to install
        #[structopt(possible_values = &["commit-msg", "pre-push", "all"])]
        hook_type: String,
    },

    /// Generate shell completions
    #[structopt(no_version, settings = SUBCOMMAND_SETTINGS)]
    GenerateCompletions {
        /// Type of completions to generate
        #[structopt(name = "type", possible_values = &["bash", "elvish", "fish", "zsh"])]
        shell: Shell,
    },
}

#[derive(StructOpt)]
#[structopt(flatten)]
struct CommitArgs {
    /// Conventional commit type
    #[structopt(name = "type", possible_values = &cog_commit::commit_types())]
    typ: String,

    /// Commit description
    message: String,

    /// Conventional commit scope
    scope: Option<String>,

    /// Create a BREAKING CHANGE commit
    #[structopt(short = "B", long)]
    breaking_change: bool,

    /// Open commit message in an editor
    #[structopt(short, long)]
    edit: bool,
}

fn main() -> Result<()> {
    let cli = Cli::from_args();

    match cli {
        Cli::Bump {
            version,
            auto,
            major,
            minor,
            patch,
            pre,
            hook_profile,
        } => {
            let mut cocogitto = CocoGitto::get()?;

            let increment = match version {
                Some(version) => VersionIncrement::Manual(version),
                None if auto => VersionIncrement::Auto,
                None if major => VersionIncrement::Major,
                None if minor => VersionIncrement::Minor,
                None if patch => VersionIncrement::Patch,
                _ => unreachable!(),
            };

            cocogitto.create_version(increment, pre.as_deref(), hook_profile.as_deref())?
        }
        Cli::Verify { message } => {
            let author = CocoGitto::get()
                .map(|cogito| cogito.get_committer().unwrap())
                .ok();

            commit::verify(author, &message)?;
        }
        Cli::Check { from_latest_tag } => {
            let cocogitto = CocoGitto::get()?;
            cocogitto.check(from_latest_tag)?;
        }
        Cli::Edit { from_latest_tag } => {
            let cocogitto = CocoGitto::get()?;
            cocogitto.check_and_edit(from_latest_tag)?;
        }
        Cli::Log {
            breaking_change,
            typ,
            author,
            scope,
            no_error,
        } => {
            let cocogitto = CocoGitto::get()?;

            let repo_tag_name = cocogitto.get_repo_tag_name();
            let repo_tag_name = repo_tag_name.as_deref().unwrap_or("cog log");

            let mut output = Output::builder()
                .with_pager_from_env("PAGER")
                .with_file_name(repo_tag_name)
                .build()?;

            let mut filters = vec![];
            if let Some(commit_types) = typ {
                filters.extend(
                    commit_types
                        .iter()
                        .map(|commit_type| CommitFilter::Type(commit_type.as_str().into())),
                );
            }

            if let Some(scopes) = scope {
                filters.extend(scopes.into_iter().map(CommitFilter::Scope));
            }

            if let Some(authors) = author {
                filters.extend(authors.into_iter().map(CommitFilter::Author));
            }

            if breaking_change {
                filters.push(CommitFilter::BreakingChange);
            }

            if no_error {
                filters.push(CommitFilter::NoError);
            }

            let filters = CommitFilters(filters);

            let content = cocogitto.get_log(filters)?;
            output
                .handle()?
                .write_all(content.as_bytes())
                .context("failed to write log into the pager")?;
        }
        Cli::Changelog {
            pattern,
            at,
            template,
            remote,
            owner,
            repository,
        } => {
            let cocogitto = CocoGitto::get()?;

            // Get a template either from arg or from config
            let template = match template {
                None => SETTINGS.to_changelog_template(),
                Some(template) => {
                    let context = if template == "remote" {
                        let remote = remote.expect("'remote' should be set for remote template");
                        let repository =
                            repository.expect("'repository' should be set for remote template");
                        let owner = owner.expect("'owner' should be set for remote template");
                        Some(RemoteContext::new(remote, repository, owner))
                    } else {
                        None
                    };

                    Some(Template::from_arg(&template, context)?)
                }
            };

            let template = template.unwrap_or_default();

            let pattern = pattern.as_deref().map(RevspecPattern::from);

            let result = match at {
                Some(at) => cocogitto.get_changelog_at_tag(&at, template)?,
                None => {
                    let changelog = cocogitto.get_changelog(pattern.unwrap_or_default(), true)?;
                    changelog.into_markdown(template)?
                }
            };
            println!("{}", result);
        }
        Cli::Init { path } => {
            cocogitto::init(&path)?;
        }
        Cli::InstallHook { hook_type } => {
            let cocogitto = CocoGitto::get()?;
            match hook_type.as_str() {
                "commit-msg" => cocogitto.install_hook(HookKind::PrepareCommit)?,
                "pre-push" => cocogitto.install_hook(HookKind::PrePush)?,
                "all" => cocogitto.install_hook(HookKind::All)?,
                _ => unreachable!(),
            }
        }
        Cli::GenerateCompletions { shell } => {
            Cli::clap().gen_completions_to("cog", shell, &mut std::io::stdout());
        }
        Cli::Commit(CommitArgs {
            typ,
            message,
            scope,
            breaking_change,
            edit,
        }) => {
            let cocogitto = CocoGitto::get()?;
            let (body, footer, breaking) = if edit {
                cog_commit::edit_message(&typ, &message, scope.as_deref(), breaking_change)?
            } else {
                (None, None, breaking_change)
            };

            cocogitto.conventional_commit(&typ, scope, message, body, footer, breaking)?;
        }
    }

    Ok(())
}
