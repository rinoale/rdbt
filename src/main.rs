mod args;
mod database;
mod onboarding;
mod safety;
mod tui;

use args::{Cli, Startup};
use clap::Parser;
use color_eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();
    let (config, client) = match cli.into_startup()? {
        Startup::Direct(config) => {
            let client = database::DatabaseClient::connect(&config).await?;
            (config, client)
        }
        Startup::Onboarding(defaults) => {
            let Some(connection) = onboarding::run_onboarding(defaults).await? else {
                return Ok(());
            };
            connection
        }
    };
    let app = tui::App::new(config, client);

    tui::run(app).await
}
