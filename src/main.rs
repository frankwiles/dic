use bollard::image::{ListImagesOptions, RemoveImageOptions};
use bollard::models::ImageSummary;
use bollard::Docker;
use clap::{App, Arg};
use humansize::{file_size_opts as options, FileSize};
use std::default::Default;
use termion::color;

/* To show file size in the same format `docker images` does */
const ROUNDED_BINARY: options::FileSizeOpts = options::FileSizeOpts {
    divider: options::Kilo::Decimal,
    units: options::Kilo::Decimal,
    decimal_places: 0,
    decimal_zeroes: 0,
    fixed_at: options::FixedAt::No,
    long_units: false,
    space: true,
    suffix: "",
    allow_negative: false,
};

fn display_image(tag: &str, size: i64) {
    println!("  - {} {}", tag, size.file_size(ROUNDED_BINARY).unwrap());
}

async fn get_images(query: &str) -> Vec<ImageSummary> {
    let docker = Docker::connect_with_local_defaults().unwrap();
    let images = &docker
        .list_images(Some(ListImagesOptions::<String> {
            all: true,
            ..Default::default()
        }))
        .await
        .unwrap();

    let mut matching_images = Vec::new();

    for image in images {
        for tag in &image.repo_tags {
            if tag.contains(&query) {
                display_image(tag, image.size);
                let img = image.clone();
                matching_images.push(img);
            }
        }
    }

    // Sort and deduplicate
    matching_images.sort_by(|a, b| b.id.cmp(&a.id));
    matching_images.dedup();

    return matching_images;
}

async fn remove_images(images: Vec<ImageSummary>) {
    let docker = Docker::connect_with_local_defaults().unwrap();
    let remove_options = Some(RemoveImageOptions { force: true, ..Default::default()});

    for image in images {
        println!("{}Removing {}", color::Fg(color::Yellow), image.id);
        docker.remove_image(&image.id, remove_options, None).await.unwrap();
    }
}

#[derive(Debug)]
pub enum PromptError {
    Bailed,
}

pub type PromptResult = Result<(), PromptError>;

fn prompt_user() -> PromptResult {
    // Prompt user for deletion
    println!(
        "\n{}Delete these Docker images? [y/N]",
        color::Fg(color::White)
    );
    let reply = rprompt::read_reply().unwrap();

    if reply.to_lowercase() != *"y" {
        return Err(PromptError::Bailed);
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    // Setup command line options
    let matches = App::new("Docker Image Cleaner")
        .version("0.1.1")
        .author("Frank Wiles <frank@revsys.com>")
        .about("Removes local docker images by simple text pattern matching")
        .arg(
            Arg::with_name("QUERY")
                .help("The string you query to match Docker containers")
                .required(true)
                .index(1),
        )
        .get_matches();

    let query = matches.value_of("QUERY").unwrap();

    // Search images, displaying matches
    println!(
        "{}Looking for local Docker images matching: \"{}{}{}\"\n",
        color::Fg(color::Green),
        color::Fg(color::LightBlue),
        query,
        color::Fg(color::Green)
    );
    let matching_images = get_images(query).await;

    if matching_images.is_empty() {
        println!(
            "{}Sorry, no matching containers found!",
            color::Fg(color::Red)
        );
        return Ok(());
    }

    // Prompt the user and remove if they agree
    match prompt_user() {
        Ok(_) => remove_images(matching_images).await,
        Err(_) => {
            println!(
                "{}Exiting, will leave your images alone!",
                color::Fg(color::Red)
            );
            std::process::exit(0);
        }
    }

    Ok(())
}
