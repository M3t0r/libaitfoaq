use askama_axum::Template;
use axum::{
    extract::{ws::WebSocketUpgrade, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use state::StateChannels;
use tokio_util::sync::CancellationToken;

mod communication;
mod state;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // initialize tracing
    tracing_subscriber::fmt::init();

    let cancellation_token = CancellationToken::new();

    let mut state = crate::state::State::new();

    let app = Router::new()
        .route("/", get(index))
        .route("/websocket", get(websocket))
        .route("/style.css", get(style))
        .route("/Mallanna-Regular.ttf", get(mallanna))
        .route("/htmx.min.js", get(htmx))
        .route("/htmx.ws.js", get(htmx_ws))
        .with_state(state.clonable_channels());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    tokio::join!(
        async {
            axum::serve(listener, app)
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

async fn index() -> Index {
    Index
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
async fn websocket(
    ws: WebSocketUpgrade,
    State(channels): axum::extract::State<StateChannels>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| crate::communication::player_handler(socket, channels))
}
