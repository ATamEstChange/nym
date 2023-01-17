// Copyright 2022 - Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use log::{debug, info};

use coconut_dkg_common::msg::InstantiateMsg;

#[derive(Debug, Parser)]
pub struct Args {
    #[clap(long)]
    pub group_addr: String,

    #[clap(long)]
    pub multisig_addr: Option<String>,

    #[clap(long)]
    pub mix_denom: Option<String>,
}

pub async fn generate(args: Args) {
    info!("Starting to generate vesting contract instantiate msg");

    debug!("Received arguments: {:?}", args);

    let multisig_addr = args.multisig_addr.unwrap_or_else(|| {
        std::env::var(network_defaults::var_names::REWARDING_VALIDATOR_ADDRESS)
            .expect("Multisig address has to be set")
    });

    let mix_denom = args.mix_denom.unwrap_or_else(|| {
        std::env::var(network_defaults::var_names::MIX_DENOM).expect("Mix denom has to be set")
    });

    let instantiate_msg = InstantiateMsg {
        group_addr: args.group_addr,
        multisig_addr,
        mix_denom,
    };

    debug!("instantiate_msg: {:?}", instantiate_msg);

    let res =
        serde_json::to_string(&instantiate_msg).expect("failed to convert instantiate msg to json");

    println!("{}", res)
}
