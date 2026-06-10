use std::{
    fmt,
    io::{self, Write},
};

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
    pub fn into_config(self) -> Result<Config> {
        let safe_mode = !self.unsafe_mode;

        if let Some(url) = self.url {
            let dbms = self
                .dbms
                .or_else(|| dbms_from_url(&url))
                .ok_or_else(|| eyre!("could not infer database connector from URL"))?;
            return Ok(Config {
                dbms,
                url,
                database: self.database,
                safe_mode,
            });
        }

        let dbms = match self.dbms {
            Some(dbms) => dbms,
            None => prompt_dbms()?,
        };
        let host = prompt_with_default("Host", &self.host)?;
        let port = match self.port {
            Some(port) => port,
            None => prompt_port(dbms)?,
        };
        let user = match self.user {
            Some(user) => user,
            None => prompt_required("User")?,
        };
        let password = match self.password {
            Some(password) if password.is_empty() => Some(prompt_password_optional()?),
            Some(password) => Some(password),
            None => prompt_password_optional().map(Some)?,
        }
        .filter(|password| !password.is_empty());
        let database = match self.database {
            Some(database) => Some(database),
            None => prompt_optional("Schema/database (optional)")?,
        };
        let url = build_url(
            dbms,
            &host,
            port,
            Some(user.as_str()),
            password,
            database.as_deref(),
        )?;

        Ok(Config {
            dbms,
            url,
            database,
            safe_mode,
        })
    }
}

fn default_port(dbms: Dbms) -> u16 {
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

fn prompt_dbms() -> Result<Dbms> {
    loop {
        let input = prompt_with_default("Connector [postgres/mysql]", "postgres")?;
        match input.trim().to_ascii_lowercase().as_str() {
            "1" | "pg" | "postgres" | "postgresql" => return Ok(Dbms::Postgres),
            "2" | "my" | "mysql" | "mariadb" => return Ok(Dbms::Mysql),
            _ => eprintln!("Please enter postgres or mysql."),
        }
    }
}

fn prompt_port(dbms: Dbms) -> Result<u16> {
    loop {
        let default = default_port(dbms).to_string();
        let input = prompt_with_default("Port", &default)?;
        match input.parse::<u16>() {
            Ok(port) => return Ok(port),
            Err(_) => eprintln!("Please enter a valid TCP port."),
        }
    }
}

fn prompt_required(label: &str) -> Result<String> {
    loop {
        let input = prompt(label)?;
        if !input.trim().is_empty() {
            return Ok(input);
        }
        eprintln!("{label} is required.");
    }
}

fn prompt_optional(label: &str) -> Result<Option<String>> {
    let input = prompt(label)?;
    if input.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(input))
    }
}

fn prompt_with_default(label: &str, default: &str) -> Result<String> {
    let input = prompt(&format!("{label} [{default}]"))?;
    if input.trim().is_empty() {
        Ok(default.to_string())
    } else {
        Ok(input)
    }
}

fn prompt(label: &str) -> Result<String> {
    print!("{label}: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn prompt_password_optional() -> Result<String> {
    rpassword::prompt_password("Password (optional): ").map_err(Into::into)
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
