use std::fmt;

use clap::{Parser, ValueEnum};
use color_eyre::eyre::{Result, eyre};
use url::Url;

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
    pub dbms: Dbms,

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
    pub fn into_config(self) -> Result<Config> {
        let url = match self.url {
            Some(url) => url,
            None => build_url(
                self.dbms,
                &self.host,
                self.port.unwrap_or_else(|| default_port(self.dbms)),
                self.user.as_deref(),
                self.password,
                self.database.as_deref(),
            )?,
        };

        Ok(Config {
            dbms: self.dbms,
            url,
            database: self.database,
            safe_mode: !self.unsafe_mode,
        })
    }
}

fn default_port(dbms: Dbms) -> u16 {
    match dbms {
        Dbms::Postgres => 5432,
        Dbms::Mysql => 3306,
    }
}

fn build_url(
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

    let password = match password {
        Some(password) if password.is_empty() => Some(rpassword::prompt_password("Password: ")?),
        Some(password) => Some(password),
        None => None,
    };

    if let Some(password) = password {
        url.set_password(Some(&password))
            .map_err(|_| eyre!("invalid database password"))?;
    }

    if let Some(database) = database {
        url.set_path(database);
    }

    Ok(url.to_string())
}
