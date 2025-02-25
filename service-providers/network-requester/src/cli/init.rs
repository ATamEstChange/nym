// Copyright 2023 - Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only

use crate::cli::try_upgrade_config;
use crate::config::{default_config_directory, default_config_filepath, default_data_directory};
use crate::{
    cli::{override_config, OverrideConfig},
    config::Config,
    error::NetworkRequesterError,
};
use clap::Args;
use nym_bin_common::output_format::OutputFormat;
use nym_client_core::cli_helpers::client_init::{
    initialise_client, CommonClientInitArgs, InitResultsWithConfig, InitialisableClient,
};
use serde::Serialize;
use std::fmt::Display;
use std::fs;
use std::path::PathBuf;

struct NetworkRequesterInit;

impl InitialisableClient for NetworkRequesterInit {
    const NAME: &'static str = "network requester";
    type Error = NetworkRequesterError;
    type InitArgs = Init;
    type Config = Config;

    fn try_upgrade_outdated_config(id: &str) -> Result<(), Self::Error> {
        try_upgrade_config(id)
    }

    fn initialise_storage_paths(id: &str) -> Result<(), Self::Error> {
        fs::create_dir_all(default_data_directory(id))?;
        fs::create_dir_all(default_config_directory(id))?;
        Ok(())
    }

    fn default_config_path(id: &str) -> PathBuf {
        default_config_filepath(id)
    }

    fn construct_config(init_args: &Self::InitArgs) -> Self::Config {
        override_config(
            Config::new(&init_args.common_args.id),
            OverrideConfig::from(init_args.clone()),
        )
    }
}

#[derive(Args, Clone, Debug)]
pub(crate) struct Init {
    #[command(flatten)]
    common_args: CommonClientInitArgs,

    /// Specifies whether this network requester should run in 'open-proxy' mode
    #[clap(long)]
    open_proxy: Option<bool>,

    /// Enable service anonymized statistics that get sent to a statistics aggregator server
    #[clap(long)]
    enable_statistics: Option<bool>,

    /// Mixnet client address where a statistics aggregator is running. The default value is a Nym
    /// aggregator client
    #[clap(long)]
    statistics_recipient: Option<String>,

    /// Specifies whether this network requester will run using the default ExitPolicy
    /// as opposed to the allow list.
    /// Note: this setting will become the default in the future releases.
    #[clap(long)]
    with_exit_policy: Option<bool>,

    #[clap(short, long, default_value_t = OutputFormat::default())]
    output: OutputFormat,
}

impl From<Init> for OverrideConfig {
    fn from(init_config: Init) -> Self {
        OverrideConfig {
            nym_apis: init_config.common_args.nym_apis,
            fastmode: init_config.common_args.fastmode,
            no_cover: init_config.common_args.no_cover,
            medium_toggle: false,
            nyxd_urls: init_config.common_args.nyxd_urls,
            enabled_credentials_mode: init_config.common_args.enabled_credentials_mode,
            enable_exit_policy: init_config.with_exit_policy,
            open_proxy: init_config.open_proxy,
            enable_statistics: init_config.enable_statistics,
            statistics_recipient: init_config.statistics_recipient,
        }
    }
}

impl AsRef<CommonClientInitArgs> for Init {
    fn as_ref(&self) -> &CommonClientInitArgs {
        &self.common_args
    }
}

#[derive(Debug, Serialize)]
pub struct InitResults {
    #[serde(flatten)]
    client_core: nym_client_core::init::types::InitResults,
    client_address: String,
}

impl InitResults {
    fn new(res: InitResultsWithConfig<Config>) -> Self {
        Self {
            client_address: res.init_results.address.to_string(),
            client_core: res.init_results,
        }
    }
}

impl Display for InitResults {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.client_core)?;
        write!(
            f,
            "Address of this network-requester: {}",
            self.client_address
        )
    }
}

pub(crate) async fn execute(args: Init) -> Result<(), NetworkRequesterError> {
    eprintln!("Initialising client...");

    let output = args.output;
    let res = initialise_client::<NetworkRequesterInit>(args).await?;

    let init_results = InitResults::new(res);
    println!("{}", output.format(&init_results));

    Ok(())
}
