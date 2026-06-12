use std::fmt;

use clap::{Parser, ValueEnum};
use color_eyre::eyre::{Result, eyre};
use url::Url;

use crate::onboarding::{OnboardingDefaults, run_onboarding};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Dbms {
    Postgres,
    Mysql,
}

impl fmt::Display for Dbms {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Dbms::Postgres => write!(f, "postgres"),
            Dbms::Mysql => write!(f, "mysql"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub dbms: Dbms,
    pub url: String,
    pub database: Option<String>,
    pub safe_mode: bool,
}

#[derive(Debug, Parser)]
#[command(author, version, about)]
pub struct Cli {
    #[arg(value_enum)]
    pub dbms: Option<Dbms>,

    #[arg(long, env = "RDBT_URL")]
    pub url: Option<String>,

    #[arg(long, default_value = "localhost", env = "RDBT_HOST")]
    pub host: String,

    #[arg(short = 'P', long, env = "RDBT_PORT")]
    pub port: Option<u16>,

    #[arg(short = 'u', long, env = "RDBT_USER")]
    pub user: Option<String>,

    #[arg(short = 'p', long, num_args = 0..=1, default_missing_value = "", env = "RDBT_PASSWORD")]
    pub password: Option<String>,

    #[arg(short = 'd', long, env = "RDBT_DATABASE")]
    pub database: Option<String>,

    #[arg(long, help = "Start with write-capable SQL enabled")]
    pub unsafe_mode: bool,
}

impl Cli {
    pub fn into_config(self) -> Result<Option<Config>> {
        let safe_mode = !self.unsafe_mode;

        if let Some(url) = self.url {
            let dbms = self
                .dbms
                .or_else(|| dbms_from_url(&url))
                .ok_or_else(|| eyre!("could not infer database connector from URL"))?;
            return Ok(Some(Config {
                dbms,
                url,
                database: self.database,
                safe_mode,
            }));
        }

        run_onboarding(OnboardingDefaults {
            dbms: self.dbms.unwrap_or(Dbms::Postgres),
            host: self.host,
            port: self.port,
            user: self.user.unwrap_or_default(),
            password: self.password.unwrap_or_default(),
            database: self.database,
            safe_mode,
        })
    }
}

pub(crate) fn default_port(dbms: Dbms) -> u16 {
    match dbms {
        Dbms::Postgres => 5432,
        Dbms::Mysql => 3306,
    }
}

fn dbms_from_url(url: &str) -> Option<Dbms> {
    let url = Url::parse(url).ok()?;
    match url.scheme() {
        "postgres" | "postgresql" => Some(Dbms::Postgres),
        "mysql" | "mariadb" => Some(Dbms::Mysql),
        _ => None,
    }
}

pub(crate) fn build_url(
    dbms: Dbms,
    host: &str,
    port: u16,
    user: Option<&str>,
    password: Option<String>,
    database: Option<&str>,
) -> Result<String> {
    let scheme = match dbms {
        Dbms::Postgres => "postgres",
        Dbms::Mysql => "mysql",
    };

    let mut url = Url::parse(&format!("{scheme}://{host}:{port}/"))?;

    if let Some(user) = user {
        url.set_username(user)
            .map_err(|_| eyre!("invalid database user"))?;
    }

    if let Some(password) = password {
        url.set_password(Some(&password))
            .map_err(|_| eyre!("invalid database password"))?;
    }

    if let Some(database) = database {
        url.set_path(database);
    }

    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Dbms, dbms_from_url};

    #[test]
    fn infers_dbms_from_url_scheme() {
        assert_eq!(
            dbms_from_url("postgres://user@localhost/app"),
            Some(Dbms::Postgres)
        );
        assert_eq!(
            dbms_from_url("mysql://user@localhost/app"),
            Some(Dbms::Mysql)
        );
    }

    #[test]
    fn dbms_argument_is_optional_for_onboarding() {
        let cli = Cli::try_parse_from(["rdbt"]).expect("empty invocation should start onboarding");
        assert_eq!(cli.dbms, None);
    }
}
