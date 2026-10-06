//! Boekhouding: the bookkeeping server for Not Bad Company.
//!
//!   boekhouding serve                 run the API (BOEK_BIND, default 127.0.0.1:8090)
//!   boekhouding create-user <email>   make (or replace) the account; asks the password on stdin
//!   boekhouding digest                print today's reminder e-mail without sending it

mod api;
mod auth;
mod bank;
mod config;
mod db;
mod entities;
mod error;
mod export;
mod files;
mod invoices;
mod mail;
mod parse;
mod pdf;
mod reminders;
mod tax;
mod totp;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Bookkeeping server for Not Bad Company")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Serve,
    CreateUser { email: String },
    Digest,
}

fn open_db() -> Result<db::Pool> {
    std::fs::create_dir_all(config::data_dir())?;
    db::open(&config::data_dir().join("boekhouding.db"))
}

#[tokio::main]
async fn main() -> Result<()> {
    config::load();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "boekhouding=info,tower_http=warn".into()))
        .init();
    match Cli::parse().cmd {
        Cmd::Serve => {
            let _ = config::secret();
            let pool = open_db()?;
            tokio::spawn(reminders::run(pool.clone()));
            let app = api::router(api::AppState { pool }).layer(tower_http::trace::TraceLayer::new_for_http());
            let listener = tokio::net::TcpListener::bind(config::bind()).await?;
            tracing::info!("listening on {}", config::bind());
            axum::serve(listener, app).with_graceful_shutdown(async { let _ = tokio::signal::ctrl_c().await; }).await?;
        }
        Cmd::CreateUser { email } => {
            let password = match std::env::var("BOEK_PASSWORD") {
                Ok(p) => p,
                Err(_) => {
                    eprint!("Wachtwoord (12+ tekens): ");
                    let mut p = String::new();
                    std::io::stdin().read_line(&mut p)?;
                    p.trim_end_matches(['\r', '\n']).to_string()
                }
            };
            let pool = open_db()?;
            let e = auth::create_user(&*pool.get()?, &email, &password)?;
            println!("Account {email} aangemaakt. Alle apparaten zijn uitgelogd.\n");
            println!("Scan deze code met je authenticator-app:\n");
            for row in &e.qr_rows {
                // Two characters per module, with a quiet zone, dark on light.
                let line: String = row.chars().map(|c| if c == '1' { "  " } else { "██" }).collect();
                println!("████{line}████");
            }
            println!("\nOf voer de link in: {}\n", e.otpauth_uri);
            println!("Herstelcodes (elk één keer te gebruiken, bewaar ze offline):");
            for c in e.recovery_codes {
                println!("  {c}");
            }
        }
        Cmd::Digest => {
            let pool = open_db()?;
            match reminders::digest(&*pool.get()?, db::today()).map_err(|e| anyhow::anyhow!(e.1))? {
                Some((subject, html)) => println!("{subject}\n\n{html}"),
                None => println!("Niets te melden vandaag."),
            }
        }
    }
    Ok(())
}
