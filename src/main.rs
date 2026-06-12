mod args;
mod database;
mod onboarding;
mod safety;
mod tui;

use args::Cli;
use clap::Parser;
use color_eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();
    let Some(config) = cli.into_config()? else {
        return Ok(());
    };
    let client = database::DatabaseClient::connect(&config).await?;
    let app = tui::App::new(config, client);

    tui::run(app).await
}
