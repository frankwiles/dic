use bollard::image::ListImagesOptions;
use bollard::models::ImageSummary;
use bollard::Docker;
use clap::{App, Arg};
use humansize::{file_size_opts as options, FileSize};
use rprompt;
use std::default::Default;
use termion::color;

fn print_type_of<T>(_: &T) {
    println!("{}", std::any::type_name::<T>())
}

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

    return matching_images;
}

async fn remove_images(images: Vec<ImageSummary>) {
    let docker = Docker::connect_with_local_defaults().unwrap();
    // TODO actually delete the image
    for image in images {
        println!("{}", image.id)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    // Setup command line options
    let matches = App::new("Docker Image Cleaner")
        .version("0.1.0")
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

    if matching_images.len() == 0 {
        println!(
            "{}Sorry, no matching containers found!",
            color::Fg(color::Red)
        );
        return Ok(());
    }

    // Prompt user for deletion
    println!(
        "{}Delete these Docker images? [y/N]",
        color::Fg(color::White)
    );
    let reply = rprompt::read_reply().unwrap();

    if reply.to_lowercase() != String::from("y") {
        println!(
            "{}Exiting, will leave your images alone!",
            color::Fg(color::Red)
        );
    }

    // Delete the images
    remove_images(matching_images).await;

    Ok(())
}
