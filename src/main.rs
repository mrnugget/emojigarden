use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Html,
    routing::get,
    Router,
};
use futures::{sink::SinkExt, stream::StreamExt};
use parking_lot::RwLock;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::broadcast;
use uuid::Uuid;

// Grid dimensions
const GRID_WIDTH: usize = 40;
const GRID_HEIGHT: usize = 30;

// Environment emojis
const TREE: &str = "🌳";
const MOUNTAIN: &str = "⛰️";

// Player emojis
const PLAYER_EMOJIS: &[&str] = &[
    "🎅", // Santa
    "👨", // Man
    "👩", // Woman
    "🤡", // Clown
    "🧙", // Wizard
    "👻", // Ghost
    "🦸", // Superhero
    "🧛", // Vampire
    "🤠", // Cowboy
    "👽", // Alien
];

#[derive(Clone, Serialize, Deserialize, Debug)]
struct Position {
    x: usize,
    y: usize,
    player_num: usize,
    emoji: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum ClientMessage {
    #[serde(rename = "move")]
    Move { direction: String },
}

#[derive(Clone, Serialize, Deserialize)]
struct GameUpdate {
    landscape: Vec<Vec<String>>,
    players: HashMap<String, Position>,
    width: usize,
    height: usize,
}

struct GameState {
    players: Arc<RwLock<HashMap<String, Position>>>,
    landscape: Vec<Vec<String>>,
    player_counter: Arc<RwLock<usize>>,
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
            player_counter: Arc::new(RwLock::new(0)),
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

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    println!("Listening on {}", addr);

    axum_server::bind(addr)
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

    // Increment player counter and assign random empty position to new player
    let player_num = {
        let mut counter = game_state.player_counter.write();
        *counter += 1;
        *counter
    };

    let position = loop {
        let mut rng = rand::thread_rng();
        let x = rng.gen_range(0..GRID_WIDTH);
        let y = rng.gen_range(0..GRID_HEIGHT);
        if game_state.landscape[y][x].is_empty() {
            let emoji =
                PLAYER_EMOJIS[rand::thread_rng().gen_range(0..PLAYER_EMOJIS.len())].to_string();
            break Position {
                x,
                y,
                player_num,
                emoji,
            };
        }
    };

    // Add player to game state
    game_state
        .players
        .write()
        .insert(player_id.clone(), position);

    // Send initial state
    let update = GameUpdate {
        landscape: game_state.landscape.clone(),
        players: game_state.players.read().clone(),
        width: GRID_WIDTH,
        height: GRID_HEIGHT,
    };
    let _ = sender
        .send(Message::Text(serde_json::to_string(&update).unwrap()))
        .await;

    // Subscribe to broadcasts
    let mut rx = tx.subscribe();

    let game_state_clone = game_state.clone();
    let tx_clone = tx.clone();

    // Handle incoming messages
    tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                println!("Received text message: {}", text);
                if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                    println!("Parsed client message: {:?}", client_msg);
                    match client_msg {
                        ClientMessage::Move { direction } => {
                            let mut players = game_state_clone.players.write();
                            if let Some(pos) = players.get_mut(&player_id) {
                                let new_pos = match direction.as_str() {
                                    "ArrowUp" if pos.y > 0 => Position {
                                        x: pos.x,
                                        y: pos.y - 1,
                                        player_num: pos.player_num,
                                        emoji: pos.emoji.clone(),
                                    },
                                    "ArrowDown" if pos.y < GRID_HEIGHT - 1 => Position {
                                        x: pos.x,
                                        y: pos.y + 1,
                                        player_num: pos.player_num,
                                        emoji: pos.emoji.clone(),
                                    },
                                    "ArrowLeft" if pos.x > 0 => Position {
                                        x: pos.x - 1,
                                        y: pos.y,
                                        player_num: pos.player_num,
                                        emoji: pos.emoji.clone(),
                                    },
                                    "ArrowRight" if pos.x < GRID_WIDTH - 1 => Position {
                                        x: pos.x + 1,
                                        y: pos.y,
                                        player_num: pos.player_num,
                                        emoji: pos.emoji.clone(),
                                    },
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
                                width: GRID_WIDTH,
                                height: GRID_HEIGHT,
                            };
                            println!("sending players: {:?}", &update.players);
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
            width: GRID_WIDTH,
            height: GRID_HEIGHT,
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
