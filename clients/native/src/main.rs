// Copyright 2021 - Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use clap::{crate_version, Parser};
use logging::setup_logging;
use network_defaults::setup_env;

pub mod client;
pub mod commands;
pub mod error;
pub mod websocket;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    setup_logging();
    println!("{}", banner());

    let args = commands::Cli::parse();
    setup_env(args.config_env_file.as_ref());
    commands::execute(&args).await
}

fn banner() -> String {
    format!(
        r#"

      _ __  _   _ _ __ ___
     | '_ \| | | | '_ \ _ \
     | | | | |_| | | | | | |
     |_| |_|\__, |_| |_| |_|
            |___/

             (client - version {:})

    "#,
        crate_version!()
    )
}
