use std::fmt::Display;
use std::io::{self, IsTerminal};

use anyhow::{Context, Result, bail};
use bollard::Docker;
use bollard::models::ImageSummary;
use bollard::query_parameters::{ListImagesOptionsBuilder, RemoveImageOptionsBuilder};
use clap::Parser;
use humansize::{DECIMAL, format_size_i};
use termion::color;

/// Remove local Docker images using simple text matching.
#[derive(Debug, Parser)]
#[command(author, version, about)]
pub struct Cli {
    /// String to match against image repository tags
    #[arg(
        value_name = "QUERY",
        required_unless_present = "all",
        conflicts_with = "all"
    )]
    pub query: Option<String>,

    /// Match every local image, including untagged images
    #[arg(long)]
    pub all: bool,

    /// Match image tags without regard to ASCII case
    #[arg(short = 'i', long)]
    pub ignore_case: bool,

    /// Show matches without prompting or deleting anything
    #[arg(long)]
    pub dry_run: bool,

    /// Delete without asking for confirmation
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Disable forced image removal (force is enabled by default)
    #[arg(long)]
    pub no_force: bool,
}

impl Cli {
    fn force(&self) -> bool {
        !self.no_force
    }
}

#[derive(Debug)]
struct SelectedImage {
    image: ImageSummary,
    matched_tags: Vec<String>,
}

fn matching_tags(image: &ImageSummary, query: Option<&str>, ignore_case: bool) -> Vec<String> {
    let Some(query) = query else {
        return image.repo_tags.clone();
    };

    if ignore_case {
        let query = query.to_ascii_lowercase();
        image
            .repo_tags
            .iter()
            .filter(|tag| tag.to_ascii_lowercase().contains(&query))
            .cloned()
            .collect()
    } else {
        image
            .repo_tags
            .iter()
            .filter(|tag| tag.contains(query))
            .cloned()
            .collect()
    }
}

fn select_images(
    images: Vec<ImageSummary>,
    query: Option<&str>,
    ignore_case: bool,
) -> Vec<SelectedImage> {
    let select_all = query.is_none();
    let mut selected: Vec<_> = images
        .into_iter()
        .filter_map(|image| {
            let matched_tags = matching_tags(&image, query, ignore_case);
            (select_all || !matched_tags.is_empty()).then_some(SelectedImage {
                image,
                matched_tags,
            })
        })
        .collect();

    selected.sort_by(|a, b| b.image.id.cmp(&a.image.id));
    selected.dedup_by(|a, b| a.image.id == b.image.id);
    selected
}

fn paint<C: color::Color>(enabled: bool, shade: C, text: impl Display) -> String {
    if enabled {
        format!("{}{}{}", color::Fg(shade), text, color::Fg(color::Reset))
    } else {
        text.to_string()
    }
}

fn display_images(images: &[SelectedImage], color_enabled: bool) {
    for selected in images {
        let size = format_size_i(selected.image.size, DECIMAL);
        if selected.matched_tags.is_empty() {
            println!("  - {} {}", selected.image.id, size);
        } else {
            for tag in &selected.matched_tags {
                println!("  - {tag} {size}");
            }
        }
    }

    println!(
        "\n{} image{} selected.",
        paint(color_enabled, color::LightBlue, images.len()),
        if images.len() == 1 { "" } else { "s" }
    );
}

fn is_confirmed(reply: &str) -> bool {
    reply.eq_ignore_ascii_case("y")
}

fn prompt_user() -> Result<bool> {
    let reply = rprompt::prompt_reply("\nDelete these Docker images? [y/N] ")
        .context("failed to read confirmation")?;
    Ok(is_confirmed(&reply))
}

async fn remove_images(
    docker: &Docker,
    images: &[SelectedImage],
    force: bool,
    color_enabled: bool,
) -> Result<()> {
    let options = RemoveImageOptionsBuilder::default().force(force).build();
    let mut failures = Vec::new();

    for selected in images {
        println!(
            "{} {}",
            paint(color_enabled, color::Yellow, "Removing"),
            selected.image.id
        );

        if let Err(error) = docker
            .remove_image(&selected.image.id, Some(options.clone()), None)
            .await
        {
            eprintln!(
                "{} {}: {error}",
                paint(color_enabled, color::Red, "Failed to remove"),
                selected.image.id
            );
            failures.push(selected.image.id.clone());
        }
    }

    if !failures.is_empty() {
        bail!(
            "failed to remove {} of {} selected images",
            failures.len(),
            images.len()
        );
    }

    Ok(())
}

pub async fn run(cli: Cli) -> Result<()> {
    let color_enabled = io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let description = cli.query.as_deref().unwrap_or("all local images");

    println!(
        "{} {}\n",
        paint(color_enabled, color::Green, "Looking for images matching:"),
        paint(color_enabled, color::LightBlue, description)
    );

    let docker = Docker::connect_with_local_defaults().context("failed to connect to Docker")?;
    let options = ListImagesOptionsBuilder::default().all(true).build();
    let images = docker
        .list_images(Some(options))
        .await
        .context("failed to list Docker images")?;
    let selected = select_images(images, cli.query.as_deref(), cli.ignore_case);

    if selected.is_empty() {
        println!(
            "{}",
            paint(color_enabled, color::Yellow, "No matching images found.")
        );
        return Ok(());
    }

    display_images(&selected, color_enabled);

    if cli.dry_run {
        println!("\nDry run: no images were removed.");
        return Ok(());
    }

    if !cli.yes && !prompt_user()? {
        println!("No images were removed.");
        return Ok(());
    }

    remove_images(&docker, &selected, cli.force(), color_enabled).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn image(id: &str, tags: &[&str]) -> ImageSummary {
        ImageSummary {
            id: id.to_owned(),
            repo_tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            size: 1_000_000,
            ..Default::default()
        }
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn query_is_required_unless_all_is_used() {
        assert!(Cli::try_parse_from(["dic"]).is_err());
        assert!(Cli::try_parse_from(["dic", "--all"]).is_ok());
        assert!(Cli::try_parse_from(["dic", "query", "--all"]).is_err());
    }

    #[test]
    fn parses_automation_and_safety_options() {
        let cli = Cli::try_parse_from([
            "dic",
            "ubuntu",
            "--ignore-case",
            "--dry-run",
            "--yes",
            "--no-force",
        ])
        .unwrap();

        assert_eq!(cli.query.as_deref(), Some("ubuntu"));
        assert!(cli.ignore_case);
        assert!(cli.dry_run);
        assert!(cli.yes);
        assert!(cli.no_force);
        assert!(!cli.force());
    }

    #[test]
    fn image_removal_is_forced_by_default() {
        let cli = Cli::try_parse_from(["dic", "ubuntu"]).unwrap();

        assert!(cli.force());
    }

    #[test]
    fn selects_images_by_matching_tag_and_deduplicates_by_image() {
        let images = vec![
            image("sha256:2", &["acme/api:latest", "acme/api:v1"]),
            image("sha256:1", &["postgres:latest"]),
            image("sha256:2", &["acme/api:duplicate"]),
        ];

        let selected = select_images(images, Some("acme/api"), false);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].image.id, "sha256:2");
        assert_eq!(selected[0].matched_tags, ["acme/api:latest", "acme/api:v1"]);
    }

    #[test]
    fn matching_can_ignore_ascii_case() {
        let images = vec![image("sha256:1", &["Acme/API:Latest"])];

        assert!(select_images(images.clone(), Some("acme/api"), false).is_empty());
        assert_eq!(select_images(images, Some("acme/api"), true).len(), 1);
    }

    #[test]
    fn all_includes_untagged_images() {
        let selected = select_images(vec![image("sha256:1", &[])], None, false);

        assert_eq!(selected.len(), 1);
        assert!(selected[0].matched_tags.is_empty());
    }

    #[test]
    fn confirmation_only_accepts_y() {
        assert!(is_confirmed("y"));
        assert!(is_confirmed("Y"));
        assert!(!is_confirmed("yes"));
        assert!(!is_confirmed(""));
        assert!(!is_confirmed("n"));
    }
}
