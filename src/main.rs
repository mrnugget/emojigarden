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
const FLOWER_LIFETIME: u64 = 10; // seconds
const PICKAXE: &str = "⛏️";
const PICKAXE_SPAWN_INTERVAL: u64 = 45; // seconds
const PICKAXE_LIFETIME: u64 = 30; // seconds

// Player emojis
const PLAYER_EMOJIS: &[&str] = &[
    "🎅",   // Santa
    "👨",   // Man
    "👩",   // Woman
    "🤡",   // Clown
    "🧙",   // Wizard
    "👻",   // Ghost
    "🦸",   // Superhero
    "🧛",   // Vampire
    "🤠",   // Cowboy
    "👽",   // Alien
    "👷",   // Construction Worker
    "👮",   // Police Officer
    "👨‍🌾", // Farmer
    "👨‍🍳", // Chef
    "👨‍🎤", // Singer
    "👨‍🎨", // Artist
    "👨‍🏫", // Teacher
    "👨‍⚕️",  // Doctor
    "👨‍🔧", // Mechanic
    "👨‍🚀", // Astronaut
    "👸",   // Princess
    "🤴",   // Prince
    "🧔",   // Person with Beard
    "👱",   // Person with Blond Hair
];

#[derive(Clone, Serialize, Deserialize, Debug)]
struct Position {
    x: usize,
    y: usize,
    player_num: usize,
    emoji: String,
    has_pickaxe: bool,
    pickaxe_uses: u8,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum ClientMessage {
    #[serde(rename = "move")]
    Move { direction: String },
    #[serde(rename = "plant")]
    PlantFlower,
}

#[derive(Clone, Serialize, Deserialize)]
struct GameUpdate {
    landscape: Vec<Vec<String>>,
    players: HashMap<String, Position>,
    width: usize,
    height: usize,
    flowers: Vec<(usize, usize)>,
    pickaxe_position: Option<(usize, usize)>,
    current_player_id: String,
}

#[derive(Clone)]
struct Flower {
    planted_at: std::time::Instant,
    planted_by: String,
}

struct GameState {
    players: Arc<RwLock<HashMap<String, Position>>>,
    landscape: Arc<RwLock<Vec<Vec<String>>>>,
    player_counter: Arc<RwLock<usize>>,
    flowers: Arc<RwLock<HashMap<(usize, usize), Flower>>>,
    pickaxe_position: Arc<RwLock<Option<(usize, usize)>>>,
}

impl GameState {
    fn new() -> Self {
        let mut landscape = vec![vec![String::new(); GRID_WIDTH]; GRID_HEIGHT];
        // Initialize landscape with random trees and mountains
        let mut tree_count = 0;
        let mut mountain_count = 0;
        
        for row in landscape.iter_mut() {
            for cell in row.iter_mut() {
                *cell = match rand::random::<f32>() {
                    n if n < 0.2 => {
                        tree_count += 1;
                        TREE.to_string()
                    },
                    n if n < 0.3 => {
                        mountain_count += 1;
                        MOUNTAIN.to_string()
                    },
                    _ => String::new(),
                };
            }
        }
        
        println!("Initialized landscape with {} trees and {} mountains", tree_count, mountain_count);

        Self {
            players: Arc::new(RwLock::new(HashMap::new())),
            landscape: Arc::new(RwLock::new(landscape)),
            player_counter: Arc::new(RwLock::new(0)),
            flowers: Arc::new(RwLock::new(HashMap::new())),
            pickaxe_position: Arc::new(RwLock::new(None)),
        }
    }
}

#[tokio::main]
async fn main() {
    let game_state = Arc::new(GameState::new());
    let (tx, _rx) = broadcast::channel(100);

    // Spawn pickaxe spawning task
    let game_state_clone = Arc::clone(&game_state);
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(PICKAXE_SPAWN_INTERVAL));
        loop {
            interval.tick().await;

            let mut rng = rand::thread_rng();
            let mut pickaxe_pos = game_state_clone.pickaxe_position.write();

            // Remove old pickaxe if it exists
            *pickaxe_pos = None;

            // Spawn new pickaxe at random empty position
            loop {
                let x = rng.gen_range(0..GRID_WIDTH);
                let y = rng.gen_range(0..GRID_HEIGHT);
                if game_state_clone.landscape.read()[y][x].is_empty() {
                    *pickaxe_pos = Some((x, y));
                    break;
                }
            }

            // Schedule pickaxe removal
            let game_state_clone2 = Arc::clone(&game_state_clone);
            let tx_clone2 = tx_clone.clone();
            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(PICKAXE_LIFETIME)).await;
                *game_state_clone2.pickaxe_position.write() = None;
                let update = GameUpdate {
                    landscape: game_state_clone2.landscape.read().clone(),
                    players: game_state_clone2.players.read().clone(),
                    width: GRID_WIDTH,
                    height: GRID_HEIGHT,
                    flowers: game_state_clone2.flowers.read().keys().cloned().collect(),
                    pickaxe_position: None,
                    current_player_id: String::new(),
                };
                let _ = tx_clone2.send(serde_json::to_string(&update).unwrap());
            });

            // Broadcast update
            let update = GameUpdate {
                landscape: game_state_clone.landscape.read().clone(),
                players: game_state_clone.players.read().clone(),
                width: GRID_WIDTH,
                height: GRID_HEIGHT,
                flowers: game_state_clone.flowers.read().keys().cloned().collect(),
                pickaxe_position: *pickaxe_pos,
                current_player_id: String::new(),
            };
            let _ = tx_clone.send(serde_json::to_string(&update).unwrap());
        }
    });

    // Spawn flower cleanup task
    let game_state_clone = Arc::clone(&game_state);
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;

            // Clean up expired flowers
            {
                let mut flowers = game_state_clone.flowers.write();
                flowers.retain(|_, flower| flower.planted_at.elapsed().as_secs() < FLOWER_LIFETIME);
            }

            // Broadcast update
            let update = GameUpdate {
                landscape: game_state_clone.landscape.read().clone(),
                players: game_state_clone.players.read().clone(),
                width: GRID_WIDTH,
                height: GRID_HEIGHT,
                flowers: game_state_clone.flowers.read().keys().cloned().collect(),
                pickaxe_position: *game_state_clone.pickaxe_position.read(),
                current_player_id: String::new(),
            };
            let _ = tx_clone.send(serde_json::to_string(&update).unwrap());
        }
    });

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
        if game_state.landscape.read()[y][x].is_empty() {
            let emoji =
                PLAYER_EMOJIS[rand::thread_rng().gen_range(0..PLAYER_EMOJIS.len())].to_string();
            break Position {
                x,
                y,
                player_num,
                emoji,
                has_pickaxe: false,
                pickaxe_uses: 0,
            };
        }
    };

    // Add player to game state and log connection
    {
        game_state
            .players
            .write()
            .insert(player_id.clone(), position.clone());
        println!(
            "Player {} connected with emoji {} at position ({}, {})",
            player_id, position.emoji, position.x, position.y
        );
    }

    // Send initial state
    let update = {
        let landscape = game_state.landscape.read().clone();
        let players = game_state.players.read().clone();
        let flowers = game_state.flowers.read().keys().cloned().collect();
        let pickaxe = *game_state.pickaxe_position.read();
        
        GameUpdate {
            landscape,
            players,
            width: GRID_WIDTH,
            height: GRID_HEIGHT,
            flowers,
            pickaxe_position: pickaxe,
            current_player_id: player_id.clone(),
        }
    };
    
    if let Ok(msg) = serde_json::to_string(&update) {
        if let Err(e) = sender.send(Message::Text(msg)).await {
            println!("Error sending initial state: {:?}", e);
            return;
        }
    }

    // Subscribe to broadcasts
    let mut rx = tx.subscribe();

    let game_state = game_state.clone();
    let tx = tx.clone();

    // Handle incoming messages
    tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                    match client_msg {
                        ClientMessage::PlantFlower => {
                            let players = game_state.players.read();
                            if let Some(pos) = players.get(&player_id) {
                                // Plant flower in adjacent cell
                                let possible_spots = vec![
                                    (pos.x.saturating_sub(1), pos.y),
                                    (pos.x + 1, pos.y),
                                    (pos.x, pos.y.saturating_sub(1)),
                                    (pos.x, pos.y + 1),
                                ];

                                for (x, y) in possible_spots {
                                    if x < GRID_WIDTH
                                        && y < GRID_HEIGHT
                                        && game_state.landscape.read()[y][x].is_empty()
                                    {
                                        game_state.flowers.write().insert(
                                            (x, y),
                                            Flower {
                                                planted_at: std::time::Instant::now(),
                                                planted_by: player_id.clone(),
                                            },
                                        );
                                        break;
                                    }
                                }
                            }
                        }
                        ClientMessage::Move { direction } => {
                            if let Some(pos) = game_state.players.write().get_mut(&player_id) {
                                let mut new_pos = match direction.as_str() {
                                    "ArrowUp" if pos.y > 0 => Position {
                                        x: pos.x,
                                        y: pos.y - 1,
                                        player_num: pos.player_num,
                                        emoji: pos.emoji.clone(),
                                        has_pickaxe: pos.has_pickaxe,
                                        pickaxe_uses: pos.pickaxe_uses,
                                    },
                                    "ArrowDown" if pos.y < GRID_HEIGHT - 1 => Position {
                                        x: pos.x,
                                        y: pos.y + 1,
                                        player_num: pos.player_num,
                                        emoji: pos.emoji.clone(),
                                        has_pickaxe: pos.has_pickaxe,
                                        pickaxe_uses: pos.pickaxe_uses,
                                    },
                                    "ArrowLeft" if pos.x > 0 => Position {
                                        x: pos.x - 1,
                                        y: pos.y,
                                        player_num: pos.player_num,
                                        emoji: pos.emoji.clone(),
                                        has_pickaxe: pos.has_pickaxe,
                                        pickaxe_uses: pos.pickaxe_uses,
                                    },
                                    "ArrowRight" if pos.x < GRID_WIDTH - 1 => Position {
                                        x: pos.x + 1,
                                        y: pos.y,
                                        player_num: pos.player_num,
                                        emoji: pos.emoji.clone(),
                                        has_pickaxe: pos.has_pickaxe,
                                        pickaxe_uses: pos.pickaxe_uses,
                                    },
                                    _ => continue,
                                };

                                // Check if new position has mountain and player has pickaxe
                                let has_mountain = {
                                    let landscape = game_state.landscape.read();
                                    landscape[new_pos.y][new_pos.x] == MOUNTAIN
                                };

                                if has_mountain && pos.has_pickaxe {
                                    // Remove mountain
                                    {
                                        let mut landscape = game_state.landscape.write();
                                        landscape[new_pos.y][new_pos.x] = String::new();
                                    }

                                    pos.pickaxe_uses += 1;
                                    if pos.pickaxe_uses >= 5 {
                                        pos.has_pickaxe = false;
                                        pos.pickaxe_uses = 0;
                                    }

                                    // Spawn new mountain at random location
                                    let (mx, my) = loop {
                                        let mut rng = rand::thread_rng();
                                        let mx = rng.gen_range(0..GRID_WIDTH);
                                        let my = rng.gen_range(0..GRID_HEIGHT);
                                        let landscape = game_state.landscape.read();
                                        if landscape[my][mx].is_empty() {
                                            break (mx, my);
                                        }
                                    };

                                    // Place new mountain
                                    {
                                        let mut landscape = game_state.landscape.write();
                                        landscape[my][mx] = MOUNTAIN.to_string();
                                    }

                                    *pos = new_pos;
                                } else if game_state.landscape.read()[new_pos.y][new_pos.x]
                                    .is_empty()
                                {
                                    // Check for flower
                                    let mut flowers = game_state.flowers.write();
                                    if let Some(flower) = flowers.remove(&(new_pos.x, new_pos.y)) {
                                        if flower.planted_by != player_id {
                                            // Change to random player emoji when picking up someone else's flower
                                            new_pos.emoji = PLAYER_EMOJIS[rand::thread_rng()
                                                .gen_range(0..PLAYER_EMOJIS.len())]
                                            .to_string();
                                        }
                                    }
                                    // Check for pickaxe
                                    let mut pickaxe_pos = game_state.pickaxe_position.write();
                                    if let Some(pickup_pos) = *pickaxe_pos {
                                        if pickup_pos == (new_pos.x, new_pos.y) {
                                            *pickaxe_pos = None;
                                            new_pos.has_pickaxe = true;
                                            new_pos.pickaxe_uses = 0;
                                        }
                                    }
                                    *pos = new_pos;
                                }
                            }
                        }
                    }

                    // Broadcast update
                    let update = GameUpdate {
                        landscape: game_state.landscape.read().clone(),
                        players: game_state.players.read().clone(),
                        width: GRID_WIDTH,
                        height: GRID_HEIGHT,
                        flowers: game_state.flowers.read().keys().cloned().collect(),
                        pickaxe_position: *game_state.pickaxe_position.read(),
                        current_player_id: player_id.clone(),
                    };
                    let _ = tx.send(serde_json::to_string(&update).unwrap());
                }
            }
        }

        // Remove player when connection closes and log disconnection
        {
            let mut players = game_state.players.write();
            if let Some(pos) = players.remove(&player_id) {
                println!(
                    "Player {} disconnected (was {} at position ({}, {}))",
                    player_id, pos.emoji, pos.x, pos.y
                );
            }
        }
        let update = GameUpdate {
            landscape: game_state.landscape.read().clone(),
            players: game_state.players.read().clone(),
            width: GRID_WIDTH,
            height: GRID_HEIGHT,
            flowers: game_state.flowers.read().keys().cloned().collect(),
            pickaxe_position: *game_state.pickaxe_position.read(),
            current_player_id: String::new(),
        };
        let _ = tx.send(serde_json::to_string(&update).unwrap());
    });

    // Forward broadcasts to client
    while let Ok(msg) = rx.recv().await {
        if sender.send(Message::Text(msg)).await.is_err() {
            break;
        }
    }
}
