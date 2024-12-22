fn main() {
    println!("Hello, world!");
}
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Html,
    routing::get,
    Router,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::broadcast;

// Grid dimensions
const GRID_WIDTH: usize = 20;
const GRID_HEIGHT: usize = 15;

// Emojis
const TREE: &str = "🌳";
const MOUNTAIN: &str = "⛰️";
const HUMAN: &str = "👤";

#[derive(Clone, Serialize, Deserialize)]
struct Position {
    x: usize,
    y: usize,
}

#[derive(Clone)]
struct GameState {
    players: RwLock<HashMap<String, Position>>,
    landscape: Vec<Vec<String>>,
}

impl GameState {
    fn new() -> Self {
        let mut landscape = vec![vec![String::new(); GRID_WIDTH]; GRID_HEIGHT];
        // Initialize landscape with random trees and mountains
        for row in landscape.iter_mut() {
            for cell in row.iter_mut() {
                *cell = match rand::random::<f32>() {
                    n if n < 0.2 => TREE.to_string(),
                    n if n < 0.3 => MOUNTAIN.to_string(),
                    _ => String::new(),
                };
            }
        }
        
        Self {
            players: RwLock::new(HashMap::new()),
            landscape,
        }
    }
}

#[tokio::main]
async fn main() {
    let game_state = Arc::new(GameState::new());
    let (tx, _rx) = broadcast::channel(100);

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/ws", get(ws_handler))
        .with_state((Arc::clone(&game_state), tx));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Listening on {}", addr);
    
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn index_handler() -> Html<String> {
    Html(include_str!("../static/index.html").to_string())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State((game_state, tx)): State<(Arc<GameState>, broadcast::Sender<String>)>,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, game_state, tx))
}

async fn handle_socket(socket: WebSocket, game_state: Arc<GameState>, _tx: broadcast::Sender<String>) {
    // WebSocket handler implementation will go here
    // This will handle player movements and state updates
}
