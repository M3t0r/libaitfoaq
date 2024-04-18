use askama_axum::Template;
use axum::{
    extract::{ws::WebSocketUpgrade, ConnectInfo, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use tracing_subscriber::prelude::*;
use state::StateChannels;
use std::{net::SocketAddr, path::PathBuf};
use tokio_util::sync::CancellationToken;

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

    let journal = PathBuf::from("./journal.jsonl");
    let mut state = crate::state::State::with_journal(&journal).expect("Could not load or create journal file");

    let app = Router::new()
        .route("/", get(index))
        .route("/websocket", get(websocket))
        .route("/favicon.ico", get(favicon))
        .route("/style.css", get(style))
        .route("/Mallanna-Regular.ttf", get(mallanna))
        .route("/htmx.min.js", get(htmx))
        .route("/htmx.ws.js", get(htmx_ws))
        .with_state(state.clonable_channels());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

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
struct Index;

#[tracing::instrument]
async fn index() -> Index {
    Index
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

#[tracing::instrument]
async fn websocket(
    ConnectInfo(peer_address): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
    State(channels): axum::extract::State<StateChannels>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        crate::communication::player_handler(socket, peer_address, channels)
    })
}
