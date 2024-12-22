use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Html,
    routing::get,
    Router,
};
use hyper::Server;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::broadcast;
use uuid::Uuid;
use futures::{sink::SinkExt, stream::StreamExt};
use rand::Rng;

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

#[derive(Serialize, Deserialize)]
enum ClientMessage {
    Move { direction: String },
}

#[derive(Clone, Serialize, Deserialize)]
struct GameUpdate {
    landscape: Vec<Vec<String>>,
    players: HashMap<String, Position>,
}

struct GameState {
    players: Arc<RwLock<HashMap<String, Position>>>,
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
            players: Arc::new(RwLock::new(HashMap::new())),
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

    Server::bind(&addr)
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

async fn handle_socket(
    socket: WebSocket,
    game_state: Arc<GameState>,
    tx: broadcast::Sender<String>,
) {
    let (mut sender, mut receiver) = socket.split();
    let player_id = Uuid::new_v4().to_string();
    
    // Assign random empty position to new player
    let position = loop {
        let mut rng = rand::thread_rng();
        let x = rng.gen_range(0..GRID_WIDTH);
        let y = rng.gen_range(0..GRID_HEIGHT);
        if game_state.landscape[y][x].is_empty() {
            break Position { x, y };
        }
    };
    
    // Add player to game state
    game_state.players.write().insert(player_id.clone(), position);
    
    // Send initial state
    let update = GameUpdate {
        landscape: game_state.landscape.clone(),
        players: game_state.players.read().clone(),
    };
    let _ = sender.send(Message::Text(serde_json::to_string(&update).unwrap())).await;
    
    // Subscribe to broadcasts
    let mut rx = tx.subscribe();
    
    let game_state_clone = game_state.clone();
    let tx_clone = tx.clone();
    
    // Handle incoming messages
    tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                    match client_msg {
                        ClientMessage::Move { direction } => {
                            let mut players = game_state_clone.players.write();
                            if let Some(pos) = players.get_mut(&player_id) {
                                let new_pos = match direction.as_str() {
                                    "ArrowUp" if pos.y > 0 => Position { x: pos.x, y: pos.y - 1 },
                                    "ArrowDown" if pos.y < GRID_HEIGHT - 1 => Position { x: pos.x, y: pos.y + 1 },
                                    "ArrowLeft" if pos.x > 0 => Position { x: pos.x - 1, y: pos.y },
                                    "ArrowRight" if pos.x < GRID_WIDTH - 1 => Position { x: pos.x + 1, y: pos.y },
                                    _ => continue,
                                };
                                
                                // Check if new position is empty or has obstacle
                                if game_state_clone.landscape[new_pos.y][new_pos.x].is_empty() {
                                    *pos = new_pos;
                                }
                            }
                            
                            // Broadcast update
                            let update = GameUpdate {
                                landscape: game_state_clone.landscape.clone(),
                                players: players.clone(),
                            };
                            let _ = tx_clone.send(serde_json::to_string(&update).unwrap());
                        }
                    }
                }
            }
        }
        
        // Remove player when connection closes
        game_state_clone.players.write().remove(&player_id);
        let update = GameUpdate {
            landscape: game_state_clone.landscape.clone(),
            players: game_state_clone.players.read().clone(),
        };
        let _ = tx_clone.send(serde_json::to_string(&update).unwrap());
    });
    
    // Forward broadcasts to client
    while let Ok(msg) = rx.recv().await {
        if sender.send(Message::Text(msg)).await.is_err() {
            break;
        }
    }
}
