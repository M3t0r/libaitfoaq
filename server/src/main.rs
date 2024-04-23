use askama_axum::Template;
use axum::{
    async_trait,
    extract::{ws::WebSocketUpgrade, ConnectInfo, State, FromRequestParts, FromRef},
    http::{
        header,
        StatusCode,
        request::Parts,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use tracing_subscriber::prelude::*;
use state::StateChannelsAndToken;
use std::{net::SocketAddr, path::PathBuf};
use tokio_util::sync::CancellationToken;
use tower_http::services::ServeDir;
use machineid_rs::{IdBuilder, HWIDComponent, Encryption};

mod communication;
mod state;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tracing_subscriber::registry()
        .with(console_subscriber::spawn())
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("libaitfoaq_server=trace".parse().unwrap())
        )
        .init();

    let cancellation_token = CancellationToken::new();

    let mut admin_token = IdBuilder::new(Encryption::SHA256)
        .add_component(HWIDComponent::SystemID)
        .add_component(HWIDComponent::CPUID)
        .add_component(HWIDComponent::MachineName)
        .add_component(HWIDComponent::FileToken("./token"))
        .build("nonceorsomminidunno")
        .expect("Can't generate a admin token");
    admin_token.truncate(16);

    let journal = PathBuf::from("./journal.jsonl");
    let mut state = crate::state::State::with_journal_and_token(&journal, admin_token.clone()).expect("Could not load or create journal file");

    let app = Router::new()
        .route("/", get(index))
        .route("/websocket", get(websocket))
        .route("/favicon.ico", get(favicon))
        .route("/style.css", get(style))
        .route("/Mallanna-Regular.ttf", get(mallanna))
        .route("/htmx.min.js", get(htmx))
        .route("/htmx.ws.js", get(htmx_ws))
        .route("/confetti.min.js", get(confetti))
        .nest_service("/board-assets", ServeDir::new("board-assets"))
        .with_state(state.clonable_channels());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    println!();
    println!("Admin interface: http://{}/?{}", listener.local_addr().unwrap(), &admin_token);
    println!();

    tokio::join!(
        async {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(cancellation_token.clone().cancelled_owned())
            .await
            .expect("stopped serving");
        },
        async {
            state.process(cancellation_token.clone()).await;
        },
        async {
            if let Err(sigint_error) = tokio::signal::ctrl_c().await {
                dbg!(sigint_error);
            }
            println!("Stopping");
            cancellation_token.cancel();
        },
    );
}

#[derive(Template)]
#[template(path = "index.html")]
struct Index{token: Option<String>}

#[tracing::instrument(skip(admin))]
async fn index(ExtractAdminToken(admin): ExtractAdminToken) -> Index {
    Index{token: admin}
}

async fn favicon() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/x-icon")],
        include_bytes!("../templates/favicon.ico"),
    )
}

async fn style() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/css")],
        include_str!("../templates/style.css"),
    )
}

async fn mallanna() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "font/ttf")],
        include_bytes!("../templates/Mallanna-Regular.ttf"),
    )
}

async fn htmx() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/javascript")],
        include_str!("../templates/htmx.min.js"),
    )
}

async fn htmx_ws() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/javascript")],
        include_str!("../templates/htmx.ws.js"),
    )
}

async fn confetti() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/javascript")],
        include_str!("../templates/confetti.min.js"),
    )
}

#[tracing::instrument(skip(ws, admin, channels_and_token))]
async fn websocket(
    ConnectInfo(peer_address): ConnectInfo<SocketAddr>,
    ExtractUserAgent(user_agent): ExtractUserAgent,
    ExtractAdminToken(admin): ExtractAdminToken,
    headers: header::HeaderMap,
    ws: WebSocketUpgrade,
    State(channels_and_token): axum::extract::State<StateChannelsAndToken>,
) -> impl IntoResponse {
    tracing::info!(%peer_address, "new websocket connection");

    let json = header::HeaderValue::from_static("application/json");
    let serializer = match headers.get(header::ACCEPT) {
        Some(value) if value == json => crate::communication::Serializer::JSON,
        _ => crate::communication::Serializer::HTML,
    };
    ws.on_upgrade(move |socket| {
        crate::communication::player_handler(socket, peer_address, channels_and_token, admin.is_some(), serializer)
    })
}

struct ExtractUserAgent(header::HeaderValue);

#[async_trait]
impl<S> FromRequestParts<S> for ExtractUserAgent
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if let Some(user_agent) = parts.headers.get(header::USER_AGENT) {
            Ok(ExtractUserAgent(user_agent.clone()))
        } else {
            Err((StatusCode::BAD_REQUEST, "`User-Agent` header is missing"))
        }
    }
}

struct ExtractAdminToken(Option<String>);

#[async_trait]
impl<S> FromRequestParts<S> for ExtractAdminToken
where
    StateChannelsAndToken: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let admin_token = StateChannelsAndToken::from_ref(state).admin_token;
        if Some(admin_token.as_str()) == parts.uri.query() {
            Ok(Self(Some(admin_token)))
        } else {
            Ok(Self(None))
        }
    }
}
