//! Provider-related instantiation and usage utilities.

pub mod curl_transport;
pub mod runtime_transport;

use crate::{
    ALCHEMY_FREE_TIER_CUPS, REQUEST_TIMEOUT,
    provider::{curl_transport::CurlTransport, runtime_transport::RuntimeTransportBuilder},
};
use alloy_chains::NamedChain;
use alloy_json_rpc::{RequestPacket, ResponsePacket, SerializedRequest};
use alloy_network::{Network, NetworkWallet};
use alloy_provider::{
    Identity, ProviderBuilder as AlloyProviderBuilder, RootProvider,
    fillers::{FillProvider, JoinFill, RecommendedFillers, WalletFiller},
    network::{AnyNetwork, EthereumWallet},
};
use alloy_rpc_client::ClientBuilder;
use alloy_transport::{
    TransportError, TransportFut, layers::RetryBackoffLayer, utils::guess_local_url,
};
use eyre::{Result, WrapErr};
use foundry_config::Config;
use reqwest::Url;
use std::{
    marker::PhantomData,
    net::SocketAddr,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tower::{Layer, Service};
use url::ParseError;

/// The assumed block time for unknown chains.
/// We assume that these are chains have a faster block time.
const DEFAULT_UNKNOWN_CHAIN_BLOCK_TIME: Duration = Duration::from_secs(3);

/// The factor to scale the block time by to get the poll interval.
const POLL_INTERVAL_BLOCK_TIME_SCALE_FACTOR: f32 = 0.6;

/// Helper type alias for a retry provider
pub type RetryProvider<N = AnyNetwork> = RootProvider<N>;

/// Helper type alias for a retry provider with a signer
pub type RetryProviderWithSigner<N = AnyNetwork, W = EthereumWallet> = FillProvider<
    JoinFill<JoinFill<Identity, <N as RecommendedFillers>::RecommendedFillers>, WalletFiller<W>>,
    RootProvider<N>,
    N,
>;

/// Constructs a provider with a 100 millisecond interval poll if it's a localhost URL (most likely
/// an anvil or other dev node) and with the default, or 7 second otherwise.
///
/// See [`try_get_http_provider`] for more details.
///
/// # Panics
///
/// Panics if the URL is invalid.
///
/// # Examples
///
/// ```
/// use foundry_common::provider::get_http_provider;
///
/// let retry_provider = get_http_provider("http://localhost:8545");
/// ```
#[inline]
#[track_caller]
pub fn get_http_provider(builder: impl AsRef<str>) -> RetryProvider {
    try_get_http_provider(builder).unwrap()
}

/// Constructs a provider with a 100 millisecond interval poll if it's a localhost URL (most likely
/// an anvil or other dev node) and with the default, or 7 second otherwise.
#[inline]
pub fn try_get_http_provider(builder: impl AsRef<str>) -> Result<RetryProvider> {
    ProviderBuilder::new(builder.as_ref()).build()
}

fn fix_serialized(ser: SerializedRequest) -> SerializedRequest {
    if ser.serialized().get().contains("\"params\"") {
        return ser;
    }
    let (meta, _) = ser.decompose();
    alloy_json_rpc::Request::new(meta.method, meta.id, serde_json::Value::Array(vec![]))
        .try_into()
        .unwrap_or_else(|_| unreachable!())
}

fn fix_request_packet(req: RequestPacket) -> RequestPacket {
    match req {
        RequestPacket::Single(ser) => RequestPacket::Single(fix_serialized(ser)),
        RequestPacket::Batch(batch) => {
            RequestPacket::Batch(batch.into_iter().map(fix_serialized).collect())
        }
    }
}

fn normalize_error_text(text: &str) -> Option<String> {
    let mut value: serde_json::Value = serde_json::from_str(text).ok()?;
    match &mut value {
        serde_json::Value::Object(obj) => {
            if let Some(serde_json::Value::String(msg)) = obj.get("error") {
                obj.insert(
                    "error".to_string(),
                    serde_json::json!({"code": -32000, "message": msg}),
                );
                Some(serde_json::to_string(obj).ok()?)
            } else {
                None
            }
        }
        serde_json::Value::Array(arr) => {
            let mut changed = false;
            for item in arr.iter_mut() {
                if let serde_json::Value::Object(obj) = item {
                    if let Some(serde_json::Value::String(msg)) = obj.get("error") {
                        obj.insert(
                            "error".to_string(),
                            serde_json::json!({"code": -32000, "message": msg}),
                        );
                        changed = true;
                    }
                }
            }
            if changed {
                Some(serde_json::to_string(arr).ok()?)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[derive(Clone, Default)]
pub struct NormalizeErrorLayer;

impl<S> Layer<S> for NormalizeErrorLayer {
    type Service = NormalizeErrorService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        NormalizeErrorService { inner }
    }
}

#[derive(Clone)]
pub struct NormalizeErrorService<S> {
    inner: S,
}

impl<S> Service<RequestPacket> for NormalizeErrorService<S>
where
    S: Service<
            RequestPacket,
            Response = ResponsePacket,
            Error = TransportError,
            Future = TransportFut<'static>,
        > + Clone
        + Send
        + Sync
        + 'static,
{
    type Response = ResponsePacket;
    type Error = TransportError;
    type Future = TransportFut<'static>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: RequestPacket) -> Self::Future {
        let mut inner = self.inner.clone();
        Box::pin(async move {
            match inner.call(req).await {
                Ok(resp) => Ok(resp),
                Err(TransportError::DeserError { err, text }) => {
                    if let Some(normalized) = normalize_error_text(&text) {
                        serde_json::from_str(&normalized)
                            .map_err(|e| TransportError::deser_err(e, normalized))
                    } else {
                        Err(TransportError::DeserError { err, text })
                    }
                }
                Err(e) => Err(e),
            }
        })
    }
}

#[derive(Clone, Default)]
pub struct ParamsLayer;

impl<S> Layer<S> for ParamsLayer {
    type Service = ParamsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ParamsService { inner }
    }
}

#[derive(Clone)]
pub struct ParamsService<S> {
    inner: S,
}

impl<S> Service<RequestPacket> for ParamsService<S>
where
    S: Service<
            RequestPacket,
            Response = ResponsePacket,
            Error = TransportError,
            Future = TransportFut<'static>,
        > + Clone
        + Send
        + Sync
        + 'static,
{
    type Response = ResponsePacket;
    type Error = TransportError;
    type Future = TransportFut<'static>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: RequestPacket) -> Self::Future {
        let req = fix_request_packet(req);
        let mut inner = self.inner.clone();
        inner.call(req)
    }
}

/// A round-robin transport that distributes requests across multiple transports.
///
/// Each request is sent to exactly one transport, rotating through the list.
/// Failover on error is handled by the retry layer above this service.
#[derive(Clone)]
pub struct RoundRobinService<S> {
    transports: Arc<Vec<S>>,
    next: Arc<AtomicUsize>,
}

impl<S> RoundRobinService<S> {
    /// Creates a new round-robin service from a non-empty list of transports.
    ///
    /// # Panics
    ///
    /// Panics if `transports` is empty.
    pub fn new(transports: Vec<S>) -> Self {
        assert!(!transports.is_empty(), "RoundRobinService requires at least one transport");
        Self { transports: Arc::new(transports), next: Arc::new(AtomicUsize::new(0)) }
    }
}

impl<S> Service<RequestPacket> for RoundRobinService<S>
where
    S: Service<
            RequestPacket,
            Response = ResponsePacket,
            Error = TransportError,
            Future = TransportFut<'static>,
        > + Clone
        + Send
        + Sync
        + 'static,
{
    type Response = ResponsePacket;
    type Error = TransportError;
    type Future = TransportFut<'static>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: RequestPacket) -> Self::Future {
        let transports = self.transports.clone();
        let idx = self.next.fetch_add(1, Ordering::Relaxed) % transports.len();
        let mut transport = transports[idx].clone();
        transport.call(req)
    }
}

/// Helper type to construct a `RetryProvider`
///
/// This builder is generic over the network type `N`, defaulting to `AnyNetwork`.
#[derive(Debug)]
pub struct ProviderBuilder<N: Network = AnyNetwork> {
    // Note: this is a result, so we can easily chain builder calls
    url: Result<Url>,
    chain: NamedChain,
    max_retry: u32,
    initial_backoff: u64,
    timeout: Duration,
    /// available CUPS
    compute_units_per_second: u64,
    /// JWT Secret
    jwt: Option<String>,
    headers: Vec<String>,
    is_local: bool,
    /// Whether to accept invalid certificates.
    accept_invalid_certs: bool,
    /// Whether to disable automatic proxy detection.
    no_proxy: bool,
    /// Whether to output curl commands instead of making requests.
    curl_mode: bool,
    /// Whether to always include `"params"` in JSON-RPC requests.
    require_params: bool,
    /// Phantom data for the network type.
    _network: PhantomData<N>,
}

impl<N: Network> ProviderBuilder<N> {
    /// Creates a new ProviderBuilder helper instance.
    pub fn new(url_str: &str) -> Self {
        // a copy is needed for the next lines to work
        let mut url_str = url_str;

        // invalid url: non-prefixed URL scheme is not allowed, so we prepend the default http
        // prefix
        let storage;
        if url_str.starts_with("localhost:") {
            storage = format!("http://{url_str}");
            url_str = storage.as_str();
        }

        let url = Url::parse(url_str)
            .or_else(|err| match err {
                ParseError::RelativeUrlWithoutBase => {
                    if SocketAddr::from_str(url_str).is_ok() {
                        Url::parse(&format!("http://{url_str}"))
                    } else {
                        let path = Path::new(url_str);

                        if let Ok(path) = resolve_path(path) {
                            Url::parse(&format!("file://{}", path.display()))
                        } else {
                            Err(err)
                        }
                    }
                }
                _ => Err(err),
            })
            .wrap_err_with(|| format!("invalid provider URL: {url_str:?}"));

        // Use the final URL string to guess if it's a local URL.
        let is_local = url.as_ref().is_ok_and(|url| guess_local_url(url.as_str()));

        Self {
            url,
            chain: NamedChain::Mainnet,
            max_retry: 8,
            initial_backoff: 800,
            timeout: REQUEST_TIMEOUT,
            // alchemy max cpus <https://docs.alchemy.com/reference/compute-units#what-are-cups-compute-units-per-second>
            compute_units_per_second: ALCHEMY_FREE_TIER_CUPS,
            jwt: None,
            headers: vec![],
            is_local,
            accept_invalid_certs: false,
            no_proxy: false,
            curl_mode: false,
            require_params: false,
            _network: PhantomData,
        }
    }

    /// Constructs a [ProviderBuilder] instantiated using [Config] values.
    ///
    /// Defaults to `http://localhost:8545` and `Mainnet`.
    pub fn from_config(config: &Config) -> Result<Self> {
        let url = config.get_rpc_url_or_localhost_http()?;
        let mut builder = Self::new(url.as_ref());

        builder = builder.accept_invalid_certs(config.eth_rpc_accept_invalid_certs);
        builder = builder.curl_mode(config.eth_rpc_curl);
        builder = builder.require_params(
            config.eth_rpc_require_params
                || std::env::var("ETH_RPC_REQUIRE_PARAMS").is_ok_and(|v| v == "true"),
        );

        if let Ok(chain) = config.chain.unwrap_or_default().try_into() {
            builder = builder.chain(chain);
        }

        if let Some(jwt) = config.get_rpc_jwt_secret()? {
            builder = builder.jwt(jwt.as_ref());
        }

        if let Some(rpc_timeout) = config.eth_rpc_timeout {
            builder = builder.timeout(Duration::from_secs(rpc_timeout));
        }

        if let Some(rpc_headers) = config.eth_rpc_headers.clone() {
            builder = builder.headers(rpc_headers);
        }

        Ok(builder)
    }

    /// Enables a request timeout.
    ///
    /// The timeout is applied from when the request starts connecting until the
    /// response body has finished.
    ///
    /// Default is no timeout.
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sets the chain of the node the provider will connect to
    pub const fn chain(mut self, chain: NamedChain) -> Self {
        self.chain = chain;
        self
    }

    /// How often to retry a failed request
    pub const fn max_retry(mut self, max_retry: u32) -> Self {
        self.max_retry = max_retry;
        self
    }

    /// How often to retry a failed request. If `None`, defaults to the already-set value.
    pub fn maybe_max_retry(mut self, max_retry: Option<u32>) -> Self {
        self.max_retry = max_retry.unwrap_or(self.max_retry);
        self
    }

    /// The starting backoff delay to use after the first failed request. If `None`, defaults to
    /// the already-set value.
    pub fn maybe_initial_backoff(mut self, initial_backoff: Option<u64>) -> Self {
        self.initial_backoff = initial_backoff.unwrap_or(self.initial_backoff);
        self
    }

    /// The starting backoff delay to use after the first failed request
    pub const fn initial_backoff(mut self, initial_backoff: u64) -> Self {
        self.initial_backoff = initial_backoff;
        self
    }

    /// Sets the number of assumed available compute units per second
    ///
    /// See also, <https://docs.alchemy.com/reference/compute-units#what-are-cups-compute-units-per-second>
    pub const fn compute_units_per_second(mut self, compute_units_per_second: u64) -> Self {
        self.compute_units_per_second = compute_units_per_second;
        self
    }

    /// Sets the number of assumed available compute units per second
    ///
    /// See also, <https://docs.alchemy.com/reference/compute-units#what-are-cups-compute-units-per-second>
    pub const fn compute_units_per_second_opt(
        mut self,
        compute_units_per_second: Option<u64>,
    ) -> Self {
        if let Some(cups) = compute_units_per_second {
            self.compute_units_per_second = cups;
        }
        self
    }

    /// Sets the provider to be local.
    ///
    /// This is useful for local dev nodes.
    pub const fn local(mut self, is_local: bool) -> Self {
        self.is_local = is_local;
        self
    }

    /// Sets aggressive `max_retry` and `initial_backoff` values
    ///
    /// This is only recommend for local dev nodes
    pub const fn aggressive(self) -> Self {
        self.max_retry(100).initial_backoff(100).local(true)
    }

    /// Sets the JWT secret
    pub fn jwt(mut self, jwt: impl Into<String>) -> Self {
        self.jwt = Some(jwt.into());
        self
    }

    /// Sets http headers
    pub fn headers(mut self, headers: Vec<String>) -> Self {
        self.headers = headers;

        self
    }

    /// Sets http headers. If `None`, defaults to the already-set value.
    pub fn maybe_headers(mut self, headers: Option<Vec<String>>) -> Self {
        self.headers = headers.unwrap_or(self.headers);
        self
    }

    /// Sets whether to accept invalid certificates.
    pub const fn accept_invalid_certs(mut self, accept_invalid_certs: bool) -> Self {
        self.accept_invalid_certs = accept_invalid_certs;
        self
    }

    /// Sets whether to disable automatic proxy detection.
    ///
    /// This can help in sandboxed environments (e.g., Cursor IDE sandbox, macOS App Sandbox)
    /// where system proxy detection via SCDynamicStore causes crashes.
    pub const fn no_proxy(mut self, no_proxy: bool) -> Self {
        self.no_proxy = no_proxy;
        self
    }

    /// Sets whether to output curl commands instead of making requests.
    ///
    /// When enabled, the provider will print equivalent curl commands to stdout
    /// instead of actually executing the RPC requests.
    pub const fn curl_mode(mut self, curl_mode: bool) -> Self {
        self.curl_mode = curl_mode;
        self
    }

    /// Sets whether to always include `"params"` in JSON-RPC requests.
    pub const fn require_params(mut self, require_params: bool) -> Self {
        self.require_params = require_params;
        self
    }

    /// Constructs the `RetryProvider` taking all configs into account.
    pub fn build(self) -> Result<RetryProvider<N>> {
        let Self {
            url,
            chain,
            max_retry,
            initial_backoff,
            timeout,
            compute_units_per_second,
            jwt,
            headers,
            is_local,
            accept_invalid_certs,
            no_proxy,
            curl_mode,
            require_params,
            ..
        } = self;
        let url = url?;

        let retry_layer =
            RetryBackoffLayer::new(max_retry, initial_backoff, compute_units_per_second);

        let normalize_layer = NormalizeErrorLayer;

        // If curl_mode is enabled, use CurlTransport instead of RuntimeTransport
        if curl_mode {
            let transport = CurlTransport::new(url).with_headers(headers).with_jwt(jwt);
            let client = if require_params {
                ClientBuilder::default().layer(ParamsLayer).layer(retry_layer).layer(normalize_layer).transport(transport, is_local)
            } else {
                ClientBuilder::default().layer(retry_layer).layer(normalize_layer).transport(transport, is_local)
            };

            let provider = AlloyProviderBuilder::<_, _, N>::default()
                .connect_provider(RootProvider::new(client));

            return Ok(provider);
        }

        let transport = RuntimeTransportBuilder::new(url)
            .with_timeout(timeout)
            .with_headers(headers)
            .with_jwt(jwt)
            .accept_invalid_certs(accept_invalid_certs)
            .no_proxy(no_proxy)
            .build();
        let client = if require_params {
            ClientBuilder::default().layer(ParamsLayer).layer(retry_layer).layer(normalize_layer).transport(transport, is_local)
        } else {
            ClientBuilder::default().layer(retry_layer).layer(normalize_layer).transport(transport, is_local)
        };

        if !is_local {
            client.set_poll_interval(
                chain
                    .average_blocktime_hint()
                    // we cap the poll interval because if not provided, chain would default to
                    // mainnet
                    .map(|hint| hint.min(DEFAULT_UNKNOWN_CHAIN_BLOCK_TIME))
                    .unwrap_or(DEFAULT_UNKNOWN_CHAIN_BLOCK_TIME)
                    .mul_f32(POLL_INTERVAL_BLOCK_TIME_SCALE_FACTOR),
            );
        }

        let provider =
            AlloyProviderBuilder::<_, _, N>::default().connect_provider(RootProvider::new(client));

        Ok(provider)
    }
}

impl<N: Network> ProviderBuilder<N> {
    /// Constructs a `RetryProvider` backed by multiple URLs using round-robin load balancing.
    ///
    /// Each request is sent to exactly one transport, rotating through the list via
    /// [`RoundRobinService`]. There is no health scoring or endpoint deprioritization.
    /// On failure, the `RetryBackoffLayer` retries the request, which naturally hits
    /// the next transport in the rotation.
    pub fn build_fallback(self, urls: Vec<String>) -> Result<RetryProvider<N>> {
        let Self {
            chain,
            max_retry,
            initial_backoff,
            timeout,
            compute_units_per_second,
            jwt,
            headers,
            accept_invalid_certs,
            no_proxy,
            curl_mode,
            require_params,
            ..
        } = self;

        eyre::ensure!(!urls.is_empty(), "at least one fork URL is required");
        eyre::ensure!(!curl_mode, "curl mode is not supported with multiple fork URLs");

        // Build a RuntimeTransport for each URL, using the same URL normalization
        // as ProviderBuilder::new() (handles localhost:port, raw socket addrs, IPC paths)
        let mut parsed_urls = Vec::with_capacity(urls.len());
        let transports: Vec<_> = urls
            .iter()
            .map(|url_str| {
                let builder = Self::new(url_str);
                let url = builder.url?;
                parsed_urls.push(url.clone());
                Ok(RuntimeTransportBuilder::new(url)
                    .with_timeout(timeout)
                    .with_headers(headers.clone())
                    .with_jwt(jwt.clone())
                    .accept_invalid_certs(accept_invalid_certs)
                    .no_proxy(no_proxy)
                    .build())
            })
            .collect::<Result<Vec<_>>>()?;

        let round_robin = RoundRobinService::new(transports);

        let retry_layer =
            RetryBackoffLayer::new(max_retry, initial_backoff, compute_units_per_second);
        let normalize_layer = NormalizeErrorLayer;
        // Use normalized/parsed URLs for local detection, consistent with build()
        let is_local = parsed_urls.iter().all(|url| guess_local_url(url.as_str()));
        let client = if require_params {
            ClientBuilder::default().layer(ParamsLayer).layer(retry_layer).layer(normalize_layer).transport(round_robin, is_local)
        } else {
            ClientBuilder::default().layer(retry_layer).layer(normalize_layer).transport(round_robin, is_local)
        };

        if !is_local {
            client.set_poll_interval(
                chain
                    .average_blocktime_hint()
                    .map(|hint| hint.min(DEFAULT_UNKNOWN_CHAIN_BLOCK_TIME))
                    .unwrap_or(DEFAULT_UNKNOWN_CHAIN_BLOCK_TIME)
                    .mul_f32(POLL_INTERVAL_BLOCK_TIME_SCALE_FACTOR),
            );
        }

        let provider =
            AlloyProviderBuilder::<_, _, N>::default().connect_provider(RootProvider::new(client));

        Ok(provider)
    }

    /// Constructs the `RetryProvider` with a wallet.
    pub fn build_with_wallet<W: NetworkWallet<N> + Clone>(
        self,
        wallet: W,
    ) -> Result<RetryProviderWithSigner<N, W>>
    where
        N: RecommendedFillers,
    {
        let Self {
            url,
            chain,
            max_retry,
            initial_backoff,
            timeout,
            compute_units_per_second,
            jwt,
            headers,
            is_local,
            accept_invalid_certs,
            no_proxy,
            curl_mode,
            require_params,
            ..
        } = self;
        let url = url?;

        let retry_layer =
            RetryBackoffLayer::new(max_retry, initial_backoff, compute_units_per_second);
        let normalize_layer = NormalizeErrorLayer;

        // If curl_mode is enabled, use CurlTransport instead of RuntimeTransport
        if curl_mode {
            let transport = CurlTransport::new(url).with_headers(headers).with_jwt(jwt);
            let client = if require_params {
                ClientBuilder::default().layer(ParamsLayer).layer(retry_layer).layer(normalize_layer).transport(transport, is_local)
            } else {
                ClientBuilder::default().layer(retry_layer).layer(normalize_layer).transport(transport, is_local)
            };

            let provider = AlloyProviderBuilder::<_, _, N>::default()
                .with_recommended_fillers()
                .wallet(wallet)
                .connect_provider(RootProvider::new(client));

            return Ok(provider);
        }

        let transport = RuntimeTransportBuilder::new(url)
            .with_timeout(timeout)
            .with_headers(headers)
            .with_jwt(jwt)
            .accept_invalid_certs(accept_invalid_certs)
            .no_proxy(no_proxy)
            .build();

        let client = if require_params {
            ClientBuilder::default().layer(ParamsLayer).layer(retry_layer).layer(normalize_layer).transport(transport, is_local)
        } else {
            ClientBuilder::default().layer(retry_layer).layer(normalize_layer).transport(transport, is_local)
        };

        if !is_local {
            client.set_poll_interval(
                chain
                    .average_blocktime_hint()
                    // we cap the poll interval because if not provided, chain would default to
                    // mainnet
                    .map(|hint| hint.min(DEFAULT_UNKNOWN_CHAIN_BLOCK_TIME))
                    .unwrap_or(DEFAULT_UNKNOWN_CHAIN_BLOCK_TIME)
                    .mul_f32(POLL_INTERVAL_BLOCK_TIME_SCALE_FACTOR),
            );
        }

        let provider = AlloyProviderBuilder::<_, _, N>::default()
            .with_recommended_fillers()
            .wallet(wallet)
            .connect_provider(RootProvider::new(client));

        Ok(provider)
    }
}

#[cfg(not(windows))]
fn resolve_path(path: &Path) -> Result<PathBuf, ()> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir().map(|d| d.join(path)).map_err(drop)
    }
}

#[cfg(windows)]
fn resolve_path(path: &Path) -> Result<PathBuf, ()> {
    if let Some(s) = path.to_str()
        && s.starts_with(r"\\.\pipe\")
    {
        return Ok(path.to_path_buf());
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir().map(|d| d.join(path)).map_err(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_json_rpc::Request;

    #[test]
    fn can_auto_correct_missing_prefix() {
        let builder = ProviderBuilder::<AnyNetwork>::new("localhost:8545");
        assert!(builder.url.is_ok());

        let url = builder.url.unwrap();
        assert_eq!(url, Url::parse("http://localhost:8545").unwrap());
    }

    #[test]
    fn fix_serialized_adds_params_for_zero_param_method() {
        let req: SerializedRequest =
            Request::<()>::new("eth_blockNumber", 1.into(), ()).try_into().unwrap();
        assert!(!req.serialized().get().contains("\"params\""));
        let fixed = fix_serialized(req);
        assert!(fixed.serialized().get().contains("\"params\":[]"));
    }

    #[test]
    fn fix_serialized_preserves_existing_params() {
        let req: SerializedRequest =
            Request::new("eth_getBalance", 1.into(), serde_json::json!(["0x1234", "latest"]))
                .try_into()
                .unwrap();
        let original = req.serialized().get().to_string();
        let fixed = fix_serialized(req);
        assert_eq!(fixed.serialized().get(), original.as_str());
    }

    #[test]
    fn fix_request_packet_single() {
        let req: SerializedRequest =
            Request::<()>::new("eth_chainId", 1.into(), ()).try_into().unwrap();
        let packet = fix_request_packet(RequestPacket::Single(req));
        match packet {
            RequestPacket::Single(ser) => {
                assert!(ser.serialized().get().contains("\"params\":[]"));
            }
            _ => panic!("expected single"),
        }
    }

    #[test]
    fn fix_request_packet_batch() {
        let req1: SerializedRequest =
            Request::<()>::new("eth_blockNumber", 1.into(), ()).try_into().unwrap();
        let req2: SerializedRequest =
            Request::<()>::new("eth_chainId", 2.into(), ()).try_into().unwrap();
        let packet = fix_request_packet(RequestPacket::Batch(vec![req1, req2]));
        match packet {
            RequestPacket::Batch(batch) => {
                for ser in &batch {
                    assert!(ser.serialized().get().contains("\"params\":[]"));
                }
            }
            _ => panic!("expected batch"),
        }
    }

    #[test]
    fn params_layer_integration() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let req: SerializedRequest =
            Request::<()>::new("eth_blockNumber", 1.into(), ()).try_into().unwrap();
        let req_packet = RequestPacket::Single(req);
        let fixed = fix_request_packet(req_packet);
        match fixed {
            RequestPacket::Single(ser) => {
                assert!(ser.serialized().get().contains("\"params\":[]"));
            }
            _ => panic!("expected single"),
        }
        drop(rt);
    }

    #[test]
    fn normalize_error_text_plain_string() {
        let raw = r#"{"jsonrpc":"2.0","error":"Method, params, and jsonrpc, are all required parameters.","id":0}"#;
        let normalized = normalize_error_text(raw).unwrap();
        let val: serde_json::Value = serde_json::from_str(&normalized).unwrap();
        assert_eq!(val["error"]["code"], -32000);
        assert_eq!(val["error"]["message"], "Method, params, and jsonrpc, are all required parameters.");
        assert_eq!(val["id"], 0);
    }

    #[test]
    fn normalize_error_text_preserves_standard_error() {
        let raw = r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Invalid Request"},"id":0}"#;
        assert!(normalize_error_text(raw).is_none());
    }

    #[test]
    fn normalize_error_text_preserves_success() {
        let raw = r#"{"jsonrpc":"2.0","result":"0x6e","id":0}"#;
        assert!(normalize_error_text(raw).is_none());
    }

    #[test]
    fn normalize_error_text_batch() {
        let raw = r#"[{"jsonrpc":"2.0","error":"first error","id":1},{"jsonrpc":"2.0","error":"second error","id":2}]"#;
        let normalized = normalize_error_text(raw).unwrap();
        let val: serde_json::Value = serde_json::from_str(&normalized).unwrap();
        assert_eq!(val[0]["error"]["code"], -32000);
        assert_eq!(val[0]["error"]["message"], "first error");
        assert_eq!(val[1]["error"]["code"], -32000);
        assert_eq!(val[1]["error"]["message"], "second error");
    }
}
