use anyhow::Result;
use clap::Parser;
use dic::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    dic::run(Cli::parse()).await
}
