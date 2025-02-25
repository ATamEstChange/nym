// Copyright 2022-2023 - Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: Apache-2.0

use super::{connection_state::BuilderState, Config, StoragePaths};
use crate::bandwidth::BandwidthAcquireClient;
use crate::mixnet::socks5_client::Socks5MixnetClient;
use crate::mixnet::{CredentialStorage, MixnetClient, Recipient};
use crate::GatewayTransceiver;
use crate::NymNetworkDetails;
use crate::{Error, Result};
use futures::channel::mpsc;
use futures::StreamExt;
use log::warn;
use nym_client_core::client::base_client::storage::gateway_details::{
    GatewayDetailsStore, PersistedGatewayDetails,
};
use nym_client_core::client::base_client::storage::{
    Ephemeral, MixnetClientStorage, OnDiskPersistent,
};
use nym_client_core::client::base_client::BaseClient;
use nym_client_core::client::key_manager::persistence::KeyStore;
use nym_client_core::config::DebugConfig;
use nym_client_core::init::helpers::current_gateways;
use nym_client_core::init::types::{GatewaySelectionSpecification, GatewaySetup};
use nym_client_core::{
    client::{base_client::BaseClientBuilder, replies::reply_storage::ReplyStorageBackend},
    config::GatewayEndpointConfig,
};
use nym_network_defaults::{DEFAULT_CLIENT_LISTENING_PORT, WG_TUN_DEVICE_ADDRESS};
use nym_socks5_client_core::config::Socks5;
use nym_task::manager::TaskStatus;
use nym_task::{TaskClient, TaskHandle};
use nym_topology::provider_trait::TopologyProvider;
use nym_validator_client::{nyxd, QueryHttpRpcNyxdClient};
use rand::rngs::OsRng;
use std::path::Path;
use std::path::PathBuf;
use url::Url;

// The number of surbs to include in a message by default
const DEFAULT_NUMBER_OF_SURBS: u32 = 10;

#[derive(Default)]
pub struct MixnetClientBuilder<S: MixnetClientStorage = Ephemeral> {
    config: Config,
    storage_paths: Option<StoragePaths>,
    gateway_config: Option<GatewayEndpointConfig>,
    socks5_config: Option<Socks5>,

    wireguard_mode: bool,
    wait_for_gateway: bool,
    custom_topology_provider: Option<Box<dyn TopologyProvider + Send + Sync>>,
    custom_gateway_transceiver: Option<Box<dyn GatewayTransceiver + Send + Sync>>,
    custom_shutdown: Option<TaskClient>,
    force_tls: bool,

    // TODO: incorporate it properly into `MixnetClientStorage` (I will need it in wasm anyway)
    gateway_endpoint_config_path: Option<PathBuf>,

    storage: S,
}

impl MixnetClientBuilder<Ephemeral> {
    /// Creates a client builder with ephemeral storage.
    #[must_use]
    pub fn new_ephemeral() -> Self {
        MixnetClientBuilder {
            ..Default::default()
        }
    }

    /// Create a client builder with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::new_ephemeral()
    }
}

impl MixnetClientBuilder<OnDiskPersistent> {
    pub async fn new_with_default_storage(storage_paths: StoragePaths) -> Result<Self> {
        Ok(MixnetClientBuilder {
            config: Default::default(),
            storage_paths: None,
            gateway_config: None,
            socks5_config: None,
            wireguard_mode: false,
            wait_for_gateway: false,
            custom_topology_provider: None,
            storage: storage_paths
                .initialise_default_persistent_storage()
                .await?,
            gateway_endpoint_config_path: None,
            custom_shutdown: None,
            custom_gateway_transceiver: None,
            force_tls: false,
        })
    }
}

impl<S> MixnetClientBuilder<S>
where
    S: MixnetClientStorage + 'static,
    S::ReplyStore: Send + Sync,
    <S::ReplyStore as ReplyStorageBackend>::StorageError: Sync + Send,
    <S::CredentialStore as CredentialStorage>::StorageError: Send + Sync,
    <S::KeyStore as KeyStore>::StorageError: Send + Sync,
    <S::GatewayDetailsStore as GatewayDetailsStore>::StorageError: Send + Sync,
{
    /// Creates a client builder with the provided client storage implementation.
    #[must_use]
    pub fn new_with_storage(storage: S) -> MixnetClientBuilder<S> {
        MixnetClientBuilder {
            config: Default::default(),
            storage_paths: None,
            gateway_config: None,
            socks5_config: None,
            wireguard_mode: false,
            wait_for_gateway: false,
            custom_topology_provider: None,
            custom_gateway_transceiver: None,
            custom_shutdown: None,
            force_tls: false,
            gateway_endpoint_config_path: None,
            storage,
        }
    }

    /// Change the underlying storage implementation.
    #[must_use]
    pub fn set_storage<T: MixnetClientStorage>(self, storage: T) -> MixnetClientBuilder<T> {
        MixnetClientBuilder {
            config: self.config,
            storage_paths: self.storage_paths,
            gateway_config: self.gateway_config,
            socks5_config: self.socks5_config,
            wireguard_mode: self.wireguard_mode,
            wait_for_gateway: self.wait_for_gateway,
            custom_topology_provider: self.custom_topology_provider,
            custom_gateway_transceiver: self.custom_gateway_transceiver,
            custom_shutdown: self.custom_shutdown,
            force_tls: self.force_tls,
            gateway_endpoint_config_path: self.gateway_endpoint_config_path,
            storage,
        }
    }

    /// Change the underlying storage of this builder to use default implementation of on-disk disk_persistence.
    #[must_use]
    pub fn set_default_storage(
        self,
        storage: OnDiskPersistent,
    ) -> MixnetClientBuilder<OnDiskPersistent> {
        self.set_storage(storage)
    }

    /// Request a specific gateway instead of a random one.
    #[must_use]
    pub fn request_gateway(mut self, user_chosen_gateway: String) -> Self {
        self.config.user_chosen_gateway = Some(user_chosen_gateway);
        self
    }

    /// Use a specific network instead of the default (mainnet) one.
    #[must_use]
    pub fn network_details(mut self, network_details: NymNetworkDetails) -> Self {
        self.config.network_details = network_details;
        self
    }

    /// Attempt to only choose a gateway that supports wss protocol.
    #[must_use]
    pub fn force_tls(mut self, must_use_tls: bool) -> Self {
        self.force_tls = must_use_tls;
        self
    }

    /// Enable paid coconut bandwidth credentials mode.
    #[must_use]
    pub fn enable_credentials_mode(mut self) -> Self {
        self.config.enabled_credentials_mode = true;
        self
    }

    /// Use a custom debugging configuration.
    #[must_use]
    pub fn debug_config(mut self, debug_config: DebugConfig) -> Self {
        self.config.debug_config = debug_config;
        self
    }

    /// Configure the SOCKS5 mode.
    #[must_use]
    pub fn socks5_config(mut self, socks5_config: Socks5) -> Self {
        self.socks5_config = Some(socks5_config);
        self
    }

    /// Use a custom topology provider.
    #[must_use]
    pub fn custom_topology_provider(
        mut self,
        topology_provider: Box<dyn TopologyProvider + Send + Sync>,
    ) -> Self {
        self.custom_topology_provider = Some(topology_provider);
        self
    }

    /// Use an externally managed shutdown mechanism.
    #[must_use]
    pub fn custom_shutdown(mut self, shutdown: TaskClient) -> Self {
        self.custom_shutdown = Some(shutdown);
        self
    }

    /// Attempt to wait for the selected gateway (if applicable) to come online if its currently not bonded.
    #[must_use]
    pub fn with_wireguard_mode(mut self, wireguard_mode: bool) -> Self {
        self.wireguard_mode = wireguard_mode;
        self
    }

    /// Attempt to wait for the selected gateway (if applicable) to come online if its currently not bonded.
    #[must_use]
    pub fn with_wait_for_gateway(mut self, wait_for_gateway: bool) -> Self {
        self.wait_for_gateway = wait_for_gateway;
        self
    }

    /// Use custom mixnet sender that might not be the default websocket gateway connection.
    /// only for advanced use
    #[must_use]
    pub fn custom_gateway_transceiver(
        mut self,
        gateway_transceiver: Box<dyn GatewayTransceiver + Send + Sync>,
    ) -> Self {
        self.custom_gateway_transceiver = Some(gateway_transceiver);
        self
    }

    /// Use specified file for storing gateway configuration.
    pub fn gateway_endpoint_config_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.gateway_endpoint_config_path = Some(path.as_ref().to_owned());
        self
    }

    /// Construct a [`DisconnectedMixnetClient`] from the setup specified.
    pub fn build(self) -> Result<DisconnectedMixnetClient<S>> {
        let client = DisconnectedMixnetClient::new(self.config, self.socks5_config, self.storage)?
            .custom_gateway_transceiver(self.custom_gateway_transceiver)
            .custom_topology_provider(self.custom_topology_provider)
            .custom_shutdown(self.custom_shutdown)
            .wireguard_mode(self.wireguard_mode)
            .wait_for_gateway(self.wait_for_gateway)
            .force_tls(self.force_tls);

        Ok(client)
    }
}

/// Represents a client that is not yet connected to the mixnet. You typically create one when you
/// want to have a separate configuration and connection phase. Once the mixnet client builder is
/// configured, call [`MixnetClientBuilder::connect_to_mixnet()`] or
/// [`MixnetClientBuilder::connect_to_mixnet_via_socks5()`] to transition to a connected
/// client.
pub struct DisconnectedMixnetClient<S>
where
    S: MixnetClientStorage,
{
    /// Client configuration
    config: Config,

    /// Socks5 configuration
    socks5_config: Option<Socks5>,

    /// The client can be in one of multiple states, depending on how it is created and if it's
    /// connected to the mixnet.
    state: BuilderState,

    /// Underlying storage of this client.
    storage: S,

    /// In the case of enabled credentials, a client instance responsible for querying the state of the
    /// dkg and coconut contracts
    dkg_query_client: Option<QueryHttpRpcNyxdClient>,

    /// Alternative provider of network topology used for constructing sphinx packets.
    custom_topology_provider: Option<Box<dyn TopologyProvider + Send + Sync>>,

    /// advanced usage of custom gateways
    custom_gateway_transceiver: Option<Box<dyn GatewayTransceiver + Send + Sync>>,

    /// If the client connects via Wireguard tunnel to the gateway.
    wireguard_mode: bool,

    /// Attempt to wait for the selected gateway (if applicable) to come online if its currently not bonded.
    wait_for_gateway: bool,

    /// Force the client to connect using wss protocol with the gateway.
    force_tls: bool,

    /// Allows passing an externally controlled shutdown handle.
    custom_shutdown: Option<TaskClient>,
}

impl<S> DisconnectedMixnetClient<S>
where
    S: MixnetClientStorage + 'static,
    S::ReplyStore: Send + Sync,
    <S::ReplyStore as ReplyStorageBackend>::StorageError: Sync + Send,
    <S::CredentialStore as CredentialStorage>::StorageError: Send + Sync,
    <S::KeyStore as KeyStore>::StorageError: Send + Sync,
    <S::GatewayDetailsStore as GatewayDetailsStore>::StorageError: Send + Sync,
{
    /// Create a new mixnet client in a disconnected state. The default configuration,
    /// creates a new mainnet client with ephemeral keys stored in RAM, which will be discarded at
    /// application close.
    ///
    /// Callers have the option of supplying further parameters to:
    /// - store persistent identities at a location on-disk, if desired;
    /// - use SOCKS5 mode
    fn new(
        config: Config,
        socks5_config: Option<Socks5>,
        storage: S,
    ) -> Result<DisconnectedMixnetClient<S>> {
        // don't create dkg client for the bandwidth controller if credentials are disabled
        let dkg_query_client = if config.enabled_credentials_mode {
            let client_config =
                nyxd::Config::try_from_nym_network_details(&config.network_details)?;
            let client = QueryHttpRpcNyxdClient::connect(
                client_config,
                config.network_details.endpoints[0].nyxd_url.as_str(),
            )?;
            Some(client)
        } else {
            None
        };

        Ok(DisconnectedMixnetClient {
            config,
            socks5_config,
            state: BuilderState::New,
            dkg_query_client,
            storage,
            custom_topology_provider: None,
            custom_gateway_transceiver: None,
            wireguard_mode: false,
            wait_for_gateway: false,
            force_tls: false,
            custom_shutdown: None,
        })
    }

    #[must_use]
    pub fn custom_shutdown(mut self, shutdown: Option<TaskClient>) -> Self {
        self.custom_shutdown = shutdown;
        self
    }

    #[must_use]
    pub fn custom_topology_provider(
        mut self,
        provider: Option<Box<dyn TopologyProvider + Send + Sync>>,
    ) -> Self {
        self.custom_topology_provider = provider;
        self
    }

    #[must_use]
    pub fn custom_gateway_transceiver(
        mut self,
        gateway_transceiver: Option<Box<dyn GatewayTransceiver + Send + Sync>>,
    ) -> Self {
        self.custom_gateway_transceiver = gateway_transceiver;
        self
    }

    #[must_use]
    pub fn wireguard_mode(mut self, wireguard_mode: bool) -> Self {
        self.wireguard_mode = wireguard_mode;
        self
    }

    #[must_use]
    pub fn wait_for_gateway(mut self, wait_for_gateway: bool) -> Self {
        self.wait_for_gateway = wait_for_gateway;
        self
    }

    #[must_use]
    pub fn force_tls(mut self, must_use_tls: bool) -> Self {
        self.force_tls = must_use_tls;
        self
    }

    fn get_api_endpoints(&self) -> Vec<Url> {
        self.config
            .network_details
            .endpoints
            .iter()
            .filter_map(|details| details.api_url.as_ref())
            .filter_map(|s| Url::parse(s).ok())
            .collect()
    }

    fn get_nyxd_endpoints(&self) -> Vec<Url> {
        self.config
            .network_details
            .endpoints
            .iter()
            .map(|details| details.nyxd_url.as_ref())
            .filter_map(|s| Url::parse(s).ok())
            .collect()
    }

    /// Client keys are generated at client creation if none were found. The gateway shared
    /// key, however, is created during the gateway registration handshake so it might not
    /// necessarily be available.
    /// Furthermore, it has to be coupled with particular gateway's config.
    async fn has_valid_gateway_info(&self) -> bool {
        let keys = match self.storage.key_store().load_keys().await {
            Ok(keys) => keys,
            Err(err) => {
                warn!("failed to load stored keys: {err}");
                return false;
            }
        };

        let gateway_details = match self
            .storage
            .gateway_details_store()
            .load_gateway_details()
            .await
        {
            Ok(details) => details,
            Err(err) => {
                warn!("failed to load stored gateway details: {err}");
                return false;
            }
        };

        if let Err(err) = gateway_details.validate(keys.gateway_shared_key().as_deref()) {
            warn!("stored key verification failure: {err}");
            return false;
        }

        true
    }

    /// Register with a gateway. If a gateway is provided in the config then that will try to be
    /// used. If none is specified, a gateway at random will be picked.
    ///
    /// # Errors
    ///
    /// This function will return an error if you try to re-register when in an already registered
    /// state.
    pub async fn register_and_authenticate_gateway(&mut self) -> Result<()> {
        if !matches!(self.state, BuilderState::New) {
            return Err(Error::ReregisteringGatewayNotSupported);
        }

        log::debug!("Registering with gateway");

        let api_endpoints = self.get_api_endpoints();

        let gateway_setup = if self.has_valid_gateway_info().await {
            GatewaySetup::MustLoad
        } else {
            let selection_spec = GatewaySelectionSpecification::new(
                self.config.user_chosen_gateway.clone(),
                None,
                self.force_tls,
            );

            let mut rng = OsRng;

            GatewaySetup::New {
                specification: selection_spec,
                available_gateways: current_gateways(&mut rng, &api_endpoints).await?,
                overwrite_data: !self.config.key_mode.is_keep(),
            }
        };

        // this will perform necessary key and details load and optional store
        let _init_result = nym_client_core::init::setup_gateway(
            gateway_setup,
            self.storage.key_store(),
            self.storage.gateway_details_store(),
        )
        .await?;

        self.state = BuilderState::Registered {};
        Ok(())
    }

    /// Creates an associated [`BandwidthAcquireClient`] that can be used to acquire bandwidth
    /// credentials for this client to consume.
    pub fn create_bandwidth_client(
        &self,
        mnemonic: String,
    ) -> Result<BandwidthAcquireClient<S::CredentialStore>> {
        if !self.config.enabled_credentials_mode {
            return Err(Error::DisabledCredentialsMode);
        }
        BandwidthAcquireClient::new(
            self.config.network_details.clone(),
            mnemonic,
            self.storage.credential_store(),
        )
    }

    async fn connect_to_mixnet_common(mut self) -> Result<(BaseClient, Recipient)> {
        // if we don't care about our keys, explicitly register
        if !self.config.key_mode.is_keep() {
            self.register_and_authenticate_gateway().await?;
        }

        // otherwise, the whole key setup and gateway selection dance will be done for us
        // when we start the base client

        let nyxd_endpoints = self.get_nyxd_endpoints();
        let nym_api_endpoints = self.get_api_endpoints();

        // a temporary workaround
        let base_config = self
            .config
            .as_base_client_config(nyxd_endpoints, nym_api_endpoints.clone());

        let known_gateway = self.has_valid_gateway_info().await;

        let mut base_builder: BaseClientBuilder<_, _> = if !known_gateway {
            let selection_spec = GatewaySelectionSpecification::new(
                self.config.user_chosen_gateway,
                None,
                self.force_tls,
            );

            let mut rng = OsRng;
            let mut available_gateways = current_gateways(&mut rng, &nym_api_endpoints).await?;
            if self.wireguard_mode {
                available_gateways
                    .iter_mut()
                    .for_each(|node| node.host = WG_TUN_DEVICE_ADDRESS.parse().unwrap());
            }
            let setup = GatewaySetup::New {
                specification: selection_spec,
                available_gateways,
                overwrite_data: !self.config.key_mode.is_keep(),
            };

            BaseClientBuilder::new(&base_config, self.storage, self.dkg_query_client)
                .with_wait_for_gateway(self.wait_for_gateway)
                .with_gateway_setup(setup)
        } else if self.wireguard_mode {
            if let Ok(PersistedGatewayDetails::Default(mut config)) = self
                .storage
                .gateway_details_store()
                .load_gateway_details()
                .await
            {
                config.details.gateway_listener = format!(
                    "ws://{}:{}",
                    WG_TUN_DEVICE_ADDRESS, DEFAULT_CLIENT_LISTENING_PORT
                );
                if let Err(e) = self
                    .storage
                    .gateway_details_store()
                    .store_gateway_details(&PersistedGatewayDetails::Default(config))
                    .await
                {
                    warn!("Could not switch to using wireguard mode - {:?}", e);
                }
            } else {
                warn!("Storage type not supported with wireguard mode");
            }
            BaseClientBuilder::new(&base_config, self.storage, self.dkg_query_client)
                .with_wait_for_gateway(self.wait_for_gateway)
        } else {
            BaseClientBuilder::new(&base_config, self.storage, self.dkg_query_client)
                .with_wait_for_gateway(self.wait_for_gateway)
        };

        if let Some(topology_provider) = self.custom_topology_provider {
            base_builder = base_builder.with_topology_provider(topology_provider);
        }

        if let Some(custom_shutdown) = self.custom_shutdown {
            base_builder = base_builder.with_shutdown(custom_shutdown)
        }

        if let Some(gateway_transceiver) = self.custom_gateway_transceiver {
            base_builder = base_builder.with_gateway_transceiver(gateway_transceiver);
        }

        let started_client = base_builder.start_base().await?;
        self.state = BuilderState::Registered {};
        let nym_address = started_client.address;

        Ok((started_client, nym_address))
    }

    /// Connect the client to the mixnet via SOCKS5. A SOCKS5 configuration must be specified
    /// before attempting to connect.
    ///
    /// - If the client is already registered with a gateway, use that gateway.
    /// - If no gateway is registered, but there is an existing configuration and key, use that.
    /// - If no gateway is registered, and there is no pre-existing configuration or key, try to
    /// register a new gateway.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use nym_sdk::mixnet;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let receiving_client = mixnet::MixnetClient::connect_new().await.unwrap();
    ///     let socks5_config = mixnet::Socks5::new(receiving_client.nym_address().to_string());
    ///     let client = mixnet::MixnetClientBuilder::new_ephemeral()
    ///         .socks5_config(socks5_config)
    ///         .build()
    ///         .unwrap();
    ///     let client = client.connect_to_mixnet_via_socks5().await.unwrap();
    /// }
    /// ```
    pub async fn connect_to_mixnet_via_socks5(self) -> Result<Socks5MixnetClient> {
        let socks5_config = self
            .socks5_config
            .clone()
            .ok_or(Error::Socks5Config { set: false })?;
        let debug_config = self.config.debug_config;
        let packet_type = self.config.debug_config.traffic.packet_type;
        let (mut started_client, nym_address) = self.connect_to_mixnet_common().await?;
        let (socks5_status_tx, mut socks5_status_rx) = mpsc::channel(128);

        let client_input = started_client.client_input.register_producer();
        let client_output = started_client.client_output.register_consumer();
        let client_state = started_client.client_state;

        nym_socks5_client_core::NymClient::<S>::start_socks5_listener(
            &socks5_config,
            debug_config,
            client_input,
            client_output,
            client_state.clone(),
            nym_address,
            started_client.task_handle.get_handle(),
            packet_type,
        );

        // TODO: more graceful handling here, surely both variants should work... I think?
        if let TaskHandle::Internal(task_manager) = &mut started_client.task_handle {
            task_manager
                .start_status_listener(socks5_status_tx, TaskStatus::Ready)
                .await;
            match socks5_status_rx
                .next()
                .await
                .ok_or(Error::Socks5NotStarted)?
                .downcast_ref::<TaskStatus>()
                .ok_or(Error::Socks5NotStarted)?
            {
                TaskStatus::Ready => {
                    log::debug!("Socks5 connected");
                }
                TaskStatus::ReadyWithGateway(gateway) => {
                    log::debug!("Socks5 connected to {gateway}");
                }
            }
        } else {
            return Err(Error::new_unsupported(
                "connecting with socks5 is currently unsupported with custom shutdown",
            ));
        }

        Ok(Socks5MixnetClient {
            nym_address,
            client_state,
            task_handle: started_client.task_handle,
            socks5_config,
        })
    }

    /// Connect the client to the mixnet.
    ///
    /// - If the client is already registered with a gateway, use that gateway.
    /// - If no gateway is registered, but there is an existing configuration and key, use that.
    /// - If no gateway is registered, and there is no pre-existing configuration or key, try to
    /// register a new gateway.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use nym_sdk::mixnet;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let client = mixnet::MixnetClientBuilder::new_ephemeral()
    ///         .build()
    ///         .unwrap();
    ///     let client = client.connect_to_mixnet().await.unwrap();
    /// }
    /// ```
    pub async fn connect_to_mixnet(self) -> Result<MixnetClient> {
        if self.socks5_config.is_some() {
            return Err(Error::Socks5Config { set: true });
        }
        let (mut started_client, nym_address) = self.connect_to_mixnet_common().await?;
        let client_input = started_client.client_input.register_producer();
        let mut client_output = started_client.client_output.register_consumer();
        let client_state = started_client.client_state;

        let reconstructed_receiver = client_output.register_receiver()?;

        Ok(MixnetClient::new(
            nym_address,
            client_input,
            client_output,
            client_state,
            reconstructed_receiver,
            started_client.task_handle,
            None,
        ))
    }
}

pub enum IncludedSurbs {
    Amount(u32),
    ExposeSelfAddress,
}
impl Default for IncludedSurbs {
    fn default() -> Self {
        Self::Amount(DEFAULT_NUMBER_OF_SURBS)
    }
}

impl IncludedSurbs {
    pub fn new(reply_surbs: u32) -> Self {
        Self::Amount(reply_surbs)
    }

    pub fn none() -> Self {
        Self::Amount(0)
    }

    pub fn expose_self_address() -> Self {
        Self::ExposeSelfAddress
    }
}
