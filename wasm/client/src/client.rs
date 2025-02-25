// Copyright 2021-2023 - Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: Apache-2.0

use crate::config::{ClientConfig, ClientConfigOpts};
use crate::error::WasmClientError;
use crate::helpers::{InputSender, WasmTopologyExt};
use crate::response_pusher::ResponsePusher;
use js_sys::Promise;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tsify::Tsify;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;
use wasm_client_core::client::{
    base_client::{BaseClientBuilder, ClientInput, ClientOutput, ClientState},
    inbound_messages::InputMessage,
};
use wasm_client_core::config::r#override::DebugWasmOverride;
use wasm_client_core::helpers::{
    parse_recipient, parse_sender_tag, setup_from_topology, setup_gateway_from_api,
};
use wasm_client_core::init::types::GatewaySetup;
use wasm_client_core::nym_task::connections::TransmissionLane;
use wasm_client_core::nym_task::TaskManager;
use wasm_client_core::storage::core_client_traits::FullWasmClientStorage;
use wasm_client_core::storage::ClientStorage;
use wasm_client_core::topology::{SerializableNymTopology, SerializableTopologyExt};
use wasm_client_core::{
    HardcodedTopologyProvider, IdentityKey, NymTopology, PacketType, QueryReqwestRpcNyxdClient,
    TopologyProvider,
};
use wasm_utils::error::PromisableResult;
use wasm_utils::{check_promise_result, console_log};

#[cfg(feature = "node-tester")]
use crate::helpers::{NymClientTestRequest, WasmTopologyTestExt};

#[cfg(feature = "node-tester")]
use rand::{rngs::OsRng, RngCore};

#[cfg(feature = "node-tester")]
pub(crate) const NODE_TESTER_CLIENT_ID: &str = "_nym-node-tester-client";

#[wasm_bindgen]
pub struct NymClient {
    self_address: String,
    client_input: Arc<ClientInput>,
    client_state: Arc<ClientState>,

    // keep track of the "old" topology for the purposes of node tester
    // so that it could be restored after the check is done
    _full_topology: Option<NymTopology>,

    // even though we don't use graceful shutdowns, other components rely on existence of this struct
    // and if it's dropped, everything will start going offline
    _task_manager: TaskManager,

    packet_type: PacketType,
}

// TODO: we don't really need a builder anymore,
// but we might as well leave it for backwards compatibility
#[wasm_bindgen]
pub struct NymClientBuilder {
    config: ClientConfig,
    force_tls: bool,
    custom_topology: Option<NymTopology>,
    preferred_gateway: Option<IdentityKey>,

    storage_passphrase: Option<String>,
    on_message: js_sys::Function,
}

#[wasm_bindgen]
impl NymClientBuilder {
    fn new(
        config: ClientConfig,
        on_message: js_sys::Function,
        force_tls: bool,
        preferred_gateway: Option<IdentityKey>,
        storage_passphrase: Option<String>,
    ) -> Self {
        NymClientBuilder {
            config,
            force_tls,
            custom_topology: None,
            storage_passphrase,
            on_message,
            // on_mix_fetch_message: Some(on_mix_fetch_message),
            preferred_gateway,
        }
    }

    // no cover traffic
    // no poisson delay
    // hardcoded topology
    // NOTE: you most likely want to use `[NymNodeTester]` instead.
    #[cfg(feature = "node-tester")]
    pub fn new_tester(
        topology: SerializableNymTopology,
        on_message: js_sys::Function,
        gateway: Option<IdentityKey>,
    ) -> Result<NymClientBuilder, WasmClientError> {
        if let Some(gateway_id) = &gateway {
            if !topology.ensure_contains_gateway_id(gateway_id) {
                panic!("the specified topology does not contain the gateway used by the client")
            }
        }

        let full_config = ClientConfig::new_tester_config(NODE_TESTER_CLIENT_ID);

        Ok(NymClientBuilder {
            config: full_config,
            force_tls: false,
            custom_topology: Some(topology.try_into()?),
            on_message,
            storage_passphrase: None,
            preferred_gateway: gateway,
        })
    }

    fn start_reconstructed_pusher(client_output: ClientOutput, on_message: js_sys::Function) {
        ResponsePusher::new(client_output, on_message).start()
    }

    fn topology_provider(&mut self) -> Option<Box<dyn TopologyProvider + Send + Sync>> {
        if let Some(hardcoded_topology) = self.custom_topology.take() {
            Some(Box::new(HardcodedTopologyProvider::new(hardcoded_topology)))
        } else {
            None
        }
    }

    fn initialise_storage(
        config: &ClientConfig,
        base_storage: ClientStorage,
    ) -> FullWasmClientStorage {
        FullWasmClientStorage::new(&config.base, base_storage)
    }

    async fn start_client_async(mut self) -> Result<NymClient, WasmClientError> {
        console_log!("Starting the wasm client");

        let nym_api_endpoints = self.config.base.client.nym_api_urls.clone();

        // TODO: this will have to be re-used for surbs. but this is a problem for another PR.
        let client_store =
            ClientStorage::new_async(&self.config.base.client.id, self.storage_passphrase.take())
                .await?;

        let user_chosen = self.preferred_gateway.clone();

        // if we provided hardcoded topology, get gateway from it, otherwise get it the 'standard' way
        let init_res = if let Some(topology) = &self.custom_topology {
            setup_from_topology(user_chosen, self.force_tls, topology, &client_store).await?
        } else {
            setup_gateway_from_api(
                &client_store,
                self.force_tls,
                user_chosen,
                &nym_api_endpoints,
            )
            .await?
        };

        let packet_type = self.config.base.debug.traffic.packet_type;
        let storage = Self::initialise_storage(&self.config, client_store);
        let maybe_topology_provider = self.topology_provider();

        let mut base_builder = BaseClientBuilder::<QueryReqwestRpcNyxdClient, _>::new(
            &self.config.base,
            storage,
            None,
        );
        if let Some(topology_provider) = maybe_topology_provider {
            base_builder = base_builder.with_topology_provider(topology_provider);
        }

        if let Ok(reuse_setup) = GatewaySetup::try_reuse_connection(init_res) {
            base_builder = base_builder.with_gateway_setup(reuse_setup);
        }

        let mut started_client = base_builder.start_base().await?;
        let self_address = started_client.address.to_string();

        let client_input = started_client.client_input.register_producer();
        let client_output = started_client.client_output.register_consumer();

        Self::start_reconstructed_pusher(client_output, self.on_message);

        Ok(NymClient {
            self_address,
            client_input: Arc::new(client_input),
            client_state: Arc::new(started_client.client_state),
            _full_topology: None,
            // this cannot failed as we haven't passed an external task manager
            _task_manager: started_client.task_handle.try_into_task_manager().unwrap(),
            packet_type,
        })
    }

    pub fn start_client(self) -> Promise {
        future_to_promise(async move { self.start_client_async().await.into_promise_result() })
    }
}

#[derive(Tsify, Debug, Clone, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ClientOptsSimple {
    // ideally we'd have used the `IdentityKey` type alias, but that'd be extremely annoying to get working in TS
    #[tsify(optional)]
    pub(crate) preferred_gateway: Option<String>,

    #[tsify(optional)]
    pub(crate) storage_passphrase: Option<String>,

    #[tsify(optional)]
    pub(crate) force_tls: Option<bool>,
}

#[derive(Tsify, Debug, Default, Clone, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ClientOpts {
    #[serde(flatten)]
    pub(crate) base: Option<ClientOptsSimple>,

    #[tsify(optional)]
    pub(crate) client_id: Option<String>,

    #[tsify(optional)]
    pub(crate) nym_api_url: Option<String>,

    // currently not used, but will be required once we have coconut
    #[tsify(optional)]
    pub(crate) nyxd_url: Option<String>,

    #[tsify(optional)]
    pub(crate) client_override: Option<DebugWasmOverride>,
}

impl<'a> From<&'a ClientOpts> for ClientConfigOpts {
    fn from(value: &'a ClientOpts) -> Self {
        ClientConfigOpts {
            id: value.client_id.as_ref().map(|v| v.to_owned()),
            nym_api: value.nym_api_url.as_ref().map(|v| v.to_owned()),
            nyxd: value.nyxd_url.as_ref().map(|v| v.to_owned()),
            debug: value.client_override.as_ref().map(|&v| v.into()),
        }
    }
}

#[wasm_bindgen]
impl NymClient {
    async fn _new(
        config: ClientConfig,
        on_message: js_sys::Function,
        opts: Option<ClientOptsSimple>,
    ) -> Result<NymClient, WasmClientError> {
        if let Some(opts) = opts {
            let preferred_gateway = opts.preferred_gateway;
            let storage_passphrase = opts.storage_passphrase;
            let force_tls = opts.force_tls.unwrap_or_default();
            NymClientBuilder::new(
                config,
                on_message,
                force_tls,
                preferred_gateway,
                storage_passphrase,
            )
        } else {
            NymClientBuilder::new(config, on_message, false, None, None)
        }
        .start_client_async()
        .await
    }

    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_ret_no_self)]
    pub fn new(on_message: js_sys::Function, opts: Option<ClientOpts>) -> Promise {
        let opts = opts.unwrap_or_default();
        let mut config = check_promise_result!(ClientConfig::new((&opts).into()));

        if let Some(dbg) = opts.client_override {
            config.override_debug(dbg);
        }

        future_to_promise(async move {
            Self::_new(config, on_message, opts.base)
                .await
                .into_promise_result()
        })
    }

    #[wasm_bindgen(js_name = "newWithConfig")]
    pub fn new_with_config(
        config: ClientConfig,
        on_message: js_sys::Function,
        opts: ClientOptsSimple,
    ) -> Promise {
        future_to_promise(async move {
            Self::_new(config, on_message, Some(opts))
                .await
                .into_promise_result()
        })
    }

    // no cover traffic
    // no poisson delay
    // hardcoded topology
    // NOTE: you most likely want to use `[NymNodeTester]` instead.
    #[cfg(feature = "node-tester")]
    #[wasm_bindgen(js_name = "newTester")]
    pub fn new_tester() -> Promise {
        todo!()
    }

    pub fn self_address(&self) -> String {
        self.self_address.clone()
    }

    #[cfg(feature = "node-tester")]
    pub fn try_construct_test_packet_request(
        &self,
        mixnode_identity: String,
        num_test_packets: Option<u32>,
    ) -> Promise {
        // TODO: improve the source of rng (i.e. don't make it ephemeral...)
        let mut ephemeral_rng = OsRng;
        let test_id = ephemeral_rng.next_u32();
        self.client_state
            .mix_test_request(test_id, mixnode_identity, num_test_packets)
    }

    pub fn change_hardcoded_topology(&self, topology: SerializableNymTopology) -> Promise {
        self.client_state.change_hardcoded_topology(topology)
    }

    pub fn current_network_topology(&self) -> Promise {
        self.client_state.current_topology()
    }

    /// Sends a test packet through the current network topology.
    /// It's the responsibility of the caller to ensure the correct topology has been injected and
    /// correct onmessage handlers have been setup.
    #[cfg(feature = "node-tester")]
    pub fn try_send_test_packets(&mut self, request: NymClientTestRequest) -> Promise {
        // TOOD: use the premade packets instead
        console_log!(
            "Attempting to send {} test packets",
            request.test_msgs.len()
        );

        // our address MUST BE valid
        let recipient = parse_recipient(&self.self_address()).unwrap();

        let lane = TransmissionLane::General;
        let input_msgs = request
            .test_msgs
            .into_iter()
            .map(|p| InputMessage::new_regular(recipient, p, lane, None))
            .collect();

        self.client_input.send_messages(input_msgs)
    }

    /// The simplest message variant where no additional information is attached.
    /// You're simply sending your `data` to specified `recipient` without any tagging.
    ///
    /// Ends up with `NymMessage::Plain` variant
    pub fn send_regular_message(&self, message: Vec<u8>, recipient: String) -> Promise {
        console_log!(
            "Attempting to send {:.2} kiB message to {recipient}",
            message.len() as f64 / 1024.0
        );

        let recipient = check_promise_result!(parse_recipient(&recipient));

        let lane = TransmissionLane::General;

        let input_msg = InputMessage::new_regular(recipient, message, lane, Some(self.packet_type));
        self.client_input.send_message(input_msg)
    }

    /// Creates a message used for a duplex anonymous communication where the recipient
    /// will never learn of our true identity. This is achieved by carefully sending `reply_surbs`.
    ///
    /// Note that if reply_surbs is set to zero then
    /// this variant requires the client having sent some reply_surbs in the past
    /// (and thus the recipient also knowing our sender tag).
    ///
    /// Ends up with `NymMessage::Repliable` variant
    pub fn send_anonymous_message(
        &self,
        message: Vec<u8>,
        recipient: String,
        reply_surbs: u32,
    ) -> Promise {
        console_log!(
            "Attempting to anonymously send {:.2} kiB message to {recipient} while attaching {reply_surbs} replySURBs.",
            message.len() as f64 / 1024.0
        );

        let recipient = check_promise_result!(parse_recipient(&recipient));

        let lane = TransmissionLane::General;

        let input_msg = InputMessage::new_anonymous(
            recipient,
            message,
            reply_surbs,
            lane,
            Some(self.packet_type),
        );
        self.client_input.send_message(input_msg)
    }

    /// Attempt to use our internally received and stored `ReplySurb` to send the message back
    /// to specified recipient whilst not knowing its full identity (or even gateway).
    ///
    /// Ends up with `NymMessage::Reply` variant
    pub fn send_reply(&self, message: Vec<u8>, recipient_tag: String) -> Promise {
        console_log!(
            "Attempting to send {:.2} kiB reply message to {recipient_tag}",
            message.len() as f64 / 1024.0
        );

        let sender_tag = check_promise_result!(parse_sender_tag(&recipient_tag));

        let lane = TransmissionLane::General;

        let input_msg = InputMessage::new_reply(sender_tag, message, lane, Some(self.packet_type));
        self.client_input.send_message(input_msg)
    }
}
