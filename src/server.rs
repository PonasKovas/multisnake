use rand::distributions::Uniform;
use rand::prelude::*;
use std::collections::{HashMap, VecDeque};
use std::io;
use std::io::prelude::*;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::thread::sleep;
use std::time::Duration;

pub const DEFAULT_WORLD_SIZE: (u16, u16) = (200, 200);
pub const DEFAULT_MAX_PLAYERS: u16 = 50;
pub const DEFAULT_GAME_SPEED: u8 = 10;
pub const DEFAULT_FOOD_RATE: u8 = 10;

/// The main structure, holds everything related to server together
pub struct Server {
    /// Maximum limit of the players connected to this server
    pub max_players: u16,
    /// A hash map mapping player IDs to their structures
    pub players: HashMap<u16, Player>,
    /// A hash map mapping player IDs to their corresponding TCP streams
    pub client_streams: HashMap<u16, TcpStream>,
    /// The size of the world that the server hosts
    pub world_size: (u16, u16),
    /// A collection of positions of foods in the world with the amount of food on that field
    pub foods: HashMap<(u16, u16), u8>,
    /// The amount of frames/ticks per second. Bigger number = faster gameplay
    pub game_speed: u8,
    /// A hash map mapping world positions where are snakes to the ID of the snake and amount of parts on that field.
    pub snake_parts: HashMap<(u16, u16), (u16, u8)>,
}

/// Holds the TCP stream of the player, and their snake's data
pub struct Player {
    /// The nickname of the snake, chosen by the client
    nickname: String,
    /// The direction which the snake faces
    direction: Direction,
    /// The direction which the snake was facing last tick.
    /// This is here to make sure that snakes don't do 180 degree turns
    last_direction: Direction,
    /// Player's parts collection
    parts: VecDeque<(u16, u16)>,
    /// The amount of snakes this player killed
    kills: u16,
    /// The score of the player (how much food did it eat)
    score: u16,
    /// When in fast mode, snakes move 2x faster but lose length and score
    fast_mode: bool,
}

/// A simple enum used to express the direction a snake is facing
#[derive(PartialEq, Copy, Clone, Debug)]
pub enum Direction {
    Left,
    Up,
    Right,
    Down,
}

impl Direction {
    /// Returns `true` if the given directions are opposite of each other
    pub fn is_opposite_of(self: Self, other: Self) -> bool {
        (self as i8 + 2) % 4 == other as i8
    }
    /// Constructs the Direction enum from a u8 integer
    pub fn from_byte(byte: u8) -> Self {
        match byte {
            0 => Self::Left,
            1 => Self::Up,
            2 => Self::Right,
            3 | _ => Self::Down,
        }
    }
    /// Converts the Direction to an Euclidean vector (not the collection)
    pub fn to_vector(self: Self) -> (i32, i32) {
        match self {
            Direction::Left => (-1, 0),
            Direction::Right => (1, 0),
            Direction::Down => (0, 1),
            Direction::Up => (0, -1),
        }
    }
}

impl Server {
    /// Constructs a new Server instance and starts it
    pub fn start(
        max_players: u16,
        game_speed: u8,
        port: u16,
        world_size: (u16, u16),
        food_rate: u8,
    ) {
        let mut server = Server {
            max_players,
            players: HashMap::new(),
            client_streams: HashMap::new(),
            world_size,
            foods: HashMap::new(),
            game_speed,
            snake_parts: HashMap::new(),
        };

        // Generate foods
        let amount_of_foods =
            ((world_size.0 as u32 * world_size.1 as u32) as f64 / food_rate as f64) as u32;
        println!("Generating {} foods...", amount_of_foods);
        for _ in 0..amount_of_foods {
            server.add_food();
        }

        // Start the thread for accepting new connections
        println!("Binding to port {}", port);
        let listener = match TcpListener::bind(format!("0.0.0.0:{}", port)) {
            Ok(listener) => listener,
            Err(_) => {
                println!("Can't bind to port {}!", port);
                return;
            }
        };
        let (add_player_sender, add_player_receiver) = channel();
        let (send_status_sender, send_status_receiver) = channel();
        thread::Builder::new()
            .name("connections_acceptor".to_string())
            .spawn(move || {
                Server::accept_connections(listener, add_player_sender, send_status_sender)
            })
            .unwrap();

        // Spawn the thread for ticks
        let (read_player_input_sender, read_player_input_receiver) = channel();
        let (move_snakes_sender, move_snakes_receiver) = channel();
        let (send_game_data_sender, send_game_data_receiver) = channel();
        thread::Builder::new()
            .name("ticks".to_string())
            .spawn(move || {
                Server::ticks(
                    game_speed,
                    read_player_input_sender,
                    move_snakes_sender,
                    send_game_data_sender,
                )
            })
            .unwrap();
        // The main thread will execute received instructions from channels
        loop {
            // HIGHER PRIORITY:
            // Read player inputs
            if let Ok(()) = read_player_input_receiver.try_recv() {
                let ids: Vec<u16> = server.players.keys().copied().collect();
                for player_id in ids {
                    server.read_player_data(player_id);
                }
                // No need to read again straight off, even if more requests have queued up,
                // so clear the channel
                let _: Vec<()> = read_player_input_receiver.try_iter().collect();
            }
            // Move snakes
            for _ in move_snakes_receiver.try_iter() {
                server.move_snakes();
            }
            // Send players game data
            if let Ok(()) = send_game_data_receiver.try_recv() {
                server.send_data_to_players();
                // No need to send again straight off, even if more requests have queued up,
                // so clear the channel
                let _: Vec<()> = send_game_data_receiver.try_iter().collect();
            }

            // LOWER PRIORITY:
            // Send server status
            if let Ok(stream) = send_status_receiver.try_recv() {
                server.send_server_data_to_stream(stream, food_rate);
            }

            // Add new players
            if let Ok((stream, nickname)) = add_player_receiver.try_recv() {
                server.add_player(stream, nickname);
            }
        }
    }
    /// Generates a single food object and adds it to the world
    pub fn add_food(self: &mut Self) {
        let x = Uniform::new(0, self.world_size.0);
        let y = Uniform::new(0, self.world_size.1);
        let mut food_pos: (u16, u16);
        loop {
            food_pos = (thread_rng().sample(x) as u16, thread_rng().sample(y) as u16);

            // If there's a snake already there, generate another position
            if self.snake_parts.contains_key(&food_pos) {
                continue;
            }
            break;
        }
        // Ok now add it to the game
        if self.foods.contains_key(&food_pos) {
            *self.foods.get_mut(&food_pos).unwrap() = self.foods[&food_pos].saturating_add(1);
        } else {
            self.foods.insert(food_pos, 1);
        }
    }
    /// Accepts new connections
    pub fn accept_connections(
        listener: TcpListener,
        add_player_sender: Sender<(TcpStream, String)>,
        send_status_sender: Sender<TcpStream>,
    ) {
        loop {
            // Accept a new connection
            if let Ok((mut stream, _addr)) = listener.accept() {
                // Set the timeout to 1 seconds
                stream
                    .set_read_timeout(Some(Duration::new(1, 0)))
                    .expect("set_read_timeout call failed");

                // Determine what the client wants:
                // - If they send a \0 byte followed by a nickname, they're here to play
                // - If they send a \x01 byte they're here to fetch game stats: leaderboard and stuff
                let bytes = match Server::read_from_stream(&mut stream) {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        // Conection lost already :O
                        continue;
                    }
                };
                if bytes.is_empty() {
                    // Client refuses to tell what he's here for
                    continue;
                }
                if bytes[0] == 0x00 {
                    // They're here to play

                    // Get the nickname
                    let mut nickname: String = match std::str::from_utf8(&bytes[1..]) {
                        Ok(string) => string.to_string(),
                        Err(_) => {
                            // Can't read username
                            // Send message and drop the connection
                            // \x05 at the beginning means that it's an error and that there's text following it
                            Server::send_to_stream(&mut stream, b"\x05can't read nickname");
                            continue;
                        }
                    };
                    // Escape the username to make it consist only of visible ascii characters
                    nickname = nickname.escape_default().to_string();
                    // Make sure that the nickname is not too long and not too short
                    if nickname.is_empty() || nickname.len() > 10 {
                        // Send message and drop the connection
                        // \x05 at the beginning means that it's an error and that there's text following it
                        Server::send_to_stream(&mut stream, b"\x05nickname too short/long");
                        continue;
                    }

                    // Send instructions to generate new player object for the player to main thread
                    add_player_sender.send((stream, nickname)).unwrap();
                } else if bytes[0] == 0x01 {
                    // Send instructions to main thread to send server stats to this stream
                    send_status_sender.send(stream).unwrap();
                } else if bytes[0] == 0x07 {
                    // It's a ping, send response
                    Server::send_to_stream(&mut stream, &[0x01, 0x07]);
                }
            }
        }
    }
    /// Sends bytes to stream, with the buffer length appended to the beginning as an u16 integer
    pub fn send_to_stream(stream: &mut TcpStream, data: &[u8]) {
        let size: [u8; 2] = u16::to_be_bytes(data.len() as u16);
        let mut message: Vec<u8> = Vec::new();
        message.extend_from_slice(&size);
        message.extend_from_slice(data);

        let _ = stream.write_all(&message);
    }

    /// Reads 1 message from stream
    /// Returns `Ok(bytes)` if the reading was successful
    /// and `Err(e)` if an error was encountered while reading
    pub fn read_from_stream(stream: &mut TcpStream) -> Result<Vec<u8>, io::ErrorKind> {
        // Figure out the size of the incoming message
        let mut size = [0u8];
        if let Err(e) = stream.read_exact(&mut size) {
            return Err(e.kind());
        }
        let size = u8::from_be_bytes(size);

        // Get the actual message
        let mut bytes = vec![0u8; size as usize];
        if let Err(e) = stream.read_exact(&mut bytes) {
            return Err(e.kind());
        }
        Ok(bytes)
    }
    /// Reads all data from a single player stream and parses it
    pub fn read_player_data(self: &mut Self, id: u16) {
        let stream = &mut self.client_streams.get_mut(&id).unwrap();
        loop {
            let bytes = match Server::read_from_stream(stream) {
                Ok(bytes) => bytes,
                Err(io::ErrorKind::WouldBlock) => {
                    // This means that we have the beginning of a message but not the end,
                    // So we will have to wait a little longer before parsing it
                    // But for now just carry on, so we don't block
                    return;
                }
                Err(e) => {
                    // Conection was lost
                    // Clean everything up and exit thread
                    println!(
                        "connection to player {} was lost: {:?}",
                        self.players[&id].nickname, e
                    );

                    // Generate food where the snake was
                    // Skip the first 3 parts though, to
                    // keep the amount of food circulating the exact same
                    let mut temp_parts = self.players[&id].parts.iter();
                    // remove the first 3 parts from the iterator
                    temp_parts.nth(2);
                    for part in temp_parts {
                        if self.foods.contains_key(part) {
                            *self.foods.get_mut(part).unwrap() = self.foods[part].saturating_add(1);
                        } else {
                            self.foods.insert(*part, 1);
                        }
                    }

                    // Remove the player's parts from the snake_parts
                    for part in &self.players[&id].parts {
                        self.snake_parts.remove(&part);
                    }
                    // Remove the player object from the players list
                    self.players.remove(&id);
                    // Remove the stream object
                    self.client_streams.remove(&id);

                    return;
                }
            };

            // Messages starting with \x02 contain a new direction that a snake faces
            if bytes.len() == 2 && bytes[0] == 0x02 {
                let new_direction = Direction::from_byte(bytes[1]);
                // Make sure that the snake isn't doing a 180 degree turn, 'cause that shit illegal
                if new_direction.is_opposite_of(self.players[&id].last_direction) {
                    continue;
                }

                // Ok, save the new direction
                self.players.get_mut(&id).unwrap().direction = new_direction;
            }

            // Messages starting with \x08 are requests to toggle fast mode
            if bytes.len() == 1 && bytes[0] == 0x08 {
                // Make sure the snake is at least 4 parts long
                if self.players[&id].parts.len() < 4 {
                    continue;
                }

                // Ok, toggle it
                self.players.get_mut(&id).unwrap().fast_mode = !self.players[&id].fast_mode;
            }
        }
    }
    /// Generate a random position for a new snake to spawn to, without overlapping
    /// with other snakes or foods
    pub fn generate_snake_pos(self: &Self, parts: &mut VecDeque<(u16, u16)>, direction: Direction) {
        let direction_vector = direction.to_vector();
        loop {
            let head_pos: (u16, u16) = (
                thread_rng().gen_range(5, self.world_size.0 - 5) as u16,
                thread_rng().gen_range(5, self.world_size.1 - 5) as u16,
            );
            let part2_pos = (
                ((head_pos.0 as i32) + direction_vector.0) as u16,
                ((head_pos.1 as i32) + direction_vector.1) as u16,
            );
            let part3_pos = (
                ((head_pos.0 as i32) + 2 * direction_vector.0) as u16,
                ((head_pos.1 as i32) + 2 * direction_vector.1) as u16,
            );
            // If there's another snake already there, generate another position
            if self.snake_parts.contains_key(&head_pos) {
                continue;
            }
            if self.snake_parts.contains_key(&part2_pos) {
                continue;
            }
            if self.snake_parts.contains_key(&part3_pos) {
                continue;
            }
            // If there's food already there, generate another position
            if self.foods.contains_key(&head_pos) {
                continue;
            }
            if self.foods.contains_key(&part2_pos) {
                continue;
            }
            if self.foods.contains_key(&part3_pos) {
                continue;
            }

            // All good bro 😎👍

            parts.push_front(head_pos);
            parts.push_front(part2_pos);
            parts.push_front(part3_pos);
            return;
        }
    }
    /// Moves all the snakes 1 field ahead to their facing direction, eating food along the way
    /// (if there's any), or killing them if they crash into other snakes
    pub fn move_snakes(self: &mut Self) {
        // Move each snake to it's facing direction
        let ids: Vec<u16> = self.players.keys().copied().collect();
        'snake: for snake_id in ids {
            // If snake not long enough anymore, turn of fast mode
            if self.players[&snake_id].fast_mode && self.players[&snake_id].score < 1 {
                self.players.get_mut(&snake_id).unwrap().fast_mode = false;
            }

            let mut moves = 1;

            // If snake in fast mode
            if self.players[&snake_id].fast_mode {
                // Remove 1 score
                self.players.get_mut(&snake_id).unwrap().score -= 1;
                // Remove 1 part from tail
                let tail_pos = self
                    .players
                    .get_mut(&snake_id)
                    .unwrap()
                    .parts
                    .pop_front()
                    .unwrap();
                self.snake_parts.get_mut(&tail_pos).unwrap().1 -= 1;
                // Only remove from the hashset if there are no more parts on that position
                if self.snake_parts[&tail_pos].1 == 0 {
                    self.snake_parts.remove(&tail_pos).unwrap();
                }
                // add food where the part was
                if self.foods.contains_key(&tail_pos) {
                    *self.foods.get_mut(&tail_pos).unwrap() =
                        self.foods[&tail_pos].saturating_add(1);
                } else {
                    self.foods.insert(tail_pos, 1);
                }
                // And make the snake move twice
                moves = 2;
            }

            // move it
            for _ in 0..moves {
                // Change the last_direction
                self.players.get_mut(&snake_id).unwrap().last_direction =
                    self.players[&snake_id].direction;

                // Calculate the new head position
                let mut new_head_pos = *self.players[&snake_id].parts.back().unwrap();

                let (dx, dy) = self.players[&snake_id].direction.to_vector();

                let width = self.world_size.0 as i32;
                let height = self.world_size.1 as i32;
                new_head_pos.0 = (((new_head_pos.0 as i32 + dx) + width) % width) as u16;
                new_head_pos.1 = (((new_head_pos.1 as i32 + dy) + height) % height) as u16;

                // If the head is on food, eat it
                if self.foods.contains_key(&new_head_pos) {
                    self.players.get_mut(&snake_id).unwrap().score +=
                        self.foods[&new_head_pos] as u16;
                    self.foods.remove(&new_head_pos);
                } else {
                    // It's impossible to crash and eat food at the same time, so only check if crashed
                    // if no food was eaten

                    if self.snake_parts.contains_key(&new_head_pos) &&
					// Make sure it's a foreign snake (not myself)
					self.snake_parts[&new_head_pos].0 != snake_id
                    {
                        // CRASH!
                        // Clean the snakes_parts
                        for part in &self.players[&snake_id].parts {
                            self.snake_parts.remove(&part);
                        }

                        // Add a kill for the snake that killed it
                        self.players
                            .get_mut(&self.snake_parts[&new_head_pos].0)
                            .unwrap()
                            .kills += 1;

                        // Add food where the dead snake was when it died
                        // Skip the first 3 parts though, to
                        // keep the amount of food circulating the exact same
                        let mut temp_parts = self.players[&snake_id].parts.iter();
                        // remove the first 3 parts from the iterator
                        temp_parts.nth(2);
                        for part in temp_parts {
                            if self.foods.contains_key(part) {
                                *self.foods.get_mut(part).unwrap() =
                                    self.foods[part].saturating_add(1);
                            } else {
                                self.foods.insert(*part, 1);
                            }
                        }

                        // Send a message to the dead player telling them that they're dead
                        // message starting with \x03 means that you died
                        Server::send_to_stream(
                            &mut self.client_streams.get_mut(&snake_id).unwrap(),
                            &[0x03],
                        );

                        // Remove the dead snake
                        self.players.remove(&snake_id);

                        // move onto the next snake, we're done with this one
                        continue 'snake;
                    }
                }

                // Add the new part to the head
                self.players
                    .get_mut(&snake_id)
                    .unwrap()
                    .parts
                    .push_back(new_head_pos);
                if self.snake_parts.contains_key(&new_head_pos) {
                    self.snake_parts.get_mut(&new_head_pos).unwrap().1 =
                        self.snake_parts[&new_head_pos].1.saturating_add(1);
                } else {
                    self.snake_parts.insert(new_head_pos, (snake_id, 1));
                }

                // Only remove last part if we don't need to grow
                if self.players[&snake_id].score <= (self.players[&snake_id].parts.len() - 4) as u16
                {
                    let last_part_pos = self
                        .players
                        .get_mut(&snake_id)
                        .unwrap()
                        .parts
                        .pop_front()
                        .unwrap();

                    self.snake_parts.get_mut(&last_part_pos).unwrap().1 -= 1;
                    // Only remove from the hashset if there are no more parts on that position
                    if self.snake_parts[&last_part_pos].1 == 0 {
                        self.snake_parts.remove(&last_part_pos).unwrap();
                    }
                }
            }
        }
    }
    /// Send game data to all connected players
    pub fn send_data_to_players(self: &mut Self) {
        // First generate the general/shared part of the
        // buffer that's going to be sent to each player
        let mut bytes: Vec<u8> = Vec::new();

        // \x04 means that it's the game data
        bytes.push(0x04u8.to_be_bytes()[0]);

        // amount of snakes in total -> 2 bytes
        bytes.extend_from_slice(&(self.players.len() as u16).to_be_bytes()[..]);

        for snake_id in self.players.keys() {
            let snake = &self.players[&snake_id];
            bytes.extend_from_slice(&snake_id.to_be_bytes()[..]); // id -> 2 bytes
            bytes.push((snake.nickname.len() as u8).to_be_bytes()[0]); // nickname length -> 1 byte
            bytes.extend_from_slice(snake.nickname.as_bytes()); // nickname -> 1-20 bytes
            bytes.extend_from_slice(&snake.score.to_be_bytes()[..]); // score -> 2 bytes
            bytes.extend_from_slice(&snake.kills.to_be_bytes()[..]); // kills -> 2 bytes
            bytes.extend_from_slice(&(snake.fast_mode as u8).to_be_bytes()[..]);
            // fast mode -> 1 byte
        }

        // Now individual data for each player
        for id in self.players.keys() {
            let mut individual_bytes = bytes.clone();

            let player_head_pos = *self.players[&id].parts.back().unwrap();
            let world_size = (self.world_size.0 as i32, self.world_size.1 as i32);

            let mut temp_snakes: Vec<u8> = Vec::new();
            let mut temp_foods: Vec<u8> = Vec::new();

            // Iterate through every field in the view of the player
            for y in -14i32..15i32 {
                for x in -24i32..25i32 {
                    let field: (u16, u16) = (
                        ((player_head_pos.0 as i32 + x + world_size.0) % world_size.0) as u16,
                        ((player_head_pos.1 as i32 + y + world_size.1) % world_size.1) as u16,
                    );
                    // Check if there's any snake here
                    if self.snake_parts.contains_key(&field) {
                        // there is
                        temp_snakes.push((x as i8).to_be_bytes()[0]); // x pos (relative to player's head) of snake part -> 1 byte
                        temp_snakes.push((y as i8).to_be_bytes()[0]); // y pos (relative to player's head) of snake part -> 1 byte
                        temp_snakes
                            .extend_from_slice(&(self.snake_parts[&field].0).to_be_bytes()[..]);
                    // id of the snake that the part belongs to -> 2 bytes
                    } else if self.foods.contains_key(&field) {
                        // Check if there's any food here
                        // there is
                        temp_foods.push((x as i8).to_be_bytes()[0]); // x pos (relative to player's head) of food -> 1 byte
                        temp_foods.push((y as i8).to_be_bytes()[0]); // y pos (relative to player's head) of food -> 1 byte
                        temp_foods.push((self.foods[&field] as u8).to_be_bytes()[0]);
                        // amount of food here -> 1 byte
                    }
                }
            }
            individual_bytes.extend_from_slice(&((temp_foods.len() / 3) as u16).to_be_bytes()[..]); // Count of foods -> 2 bytes
            individual_bytes.extend_from_slice(&temp_foods[..]); // Foods -> 0-2842 bytes

            individual_bytes.extend_from_slice(&((temp_snakes.len() / 4) as u16).to_be_bytes()[..]); // Count of snake parts -> 2 bytes
            individual_bytes.extend_from_slice(&temp_snakes[..]); // Snake parts -> 0-5684 bytes

            // Snake's head position relative to world -> 4 bytes
            individual_bytes.extend_from_slice(&(player_head_pos.0).to_be_bytes()[..]);
            individual_bytes.extend_from_slice(&(player_head_pos.1).to_be_bytes()[..]);

            // Send it
            Server::send_to_stream(
                &mut self.client_streams.get_mut(&id).unwrap(),
                &individual_bytes[..],
            );
        }
    }
    /// Send server status to stream which requested it
    pub fn send_server_data_to_stream(self: &Self, mut stream: TcpStream, food_rate: u8) {
        let mut bytes: Vec<u8> = Vec::new();
        // max players -> 2 bytes
        bytes.extend_from_slice(&self.max_players.to_be_bytes()[..]);
        // players playing now -> 2 bytes
        let playing_now = self.players.len() as u16;
        bytes.extend_from_slice(&playing_now.to_be_bytes()[..]);
        // world size -> 4 bytes
        bytes.extend_from_slice(&self.world_size.0.to_be_bytes()[..]);
        bytes.extend_from_slice(&self.world_size.1.to_be_bytes()[..]);
        // food rate -> 1 byte
        bytes.extend_from_slice(&food_rate.to_be_bytes()[..]);
        // game speed -> 1 byte
        bytes.extend_from_slice(&self.game_speed.to_be_bytes()[..]);
        Server::send_to_stream(&mut stream, &bytes);
    }
    /// Adds a new player object to the world
    pub fn add_player(self: &mut Self, mut stream: TcpStream, nickname: String) {
        // Make sure the server is not full yet
        let playing_now = self.players.len() as u16;
        if playing_now >= self.max_players {
            // Send error and drop connection
            // \x05 at the beginning means that it's an error and that there's text following it
            Server::send_to_stream(&mut stream, b"\x05server full");
            return;
        }
        // Generate a Player object for our new player :)
        // Generate random direction
        let direction = Direction::from_byte(thread_rng().gen_range(0, 4) as u8);
        // Generate parts
        let mut parts: VecDeque<(u16, u16)> = VecDeque::new();
        // generate random position for parts
        self.generate_snake_pos(&mut parts, direction);

        let player = Player {
            nickname: nickname.clone(),
            direction,
            last_direction: direction,
            parts,
            kills: 0,
            score: 0,
            fast_mode: false,
        };
        // generate an ID for this new player
        let mut id: u16 = 0;
        for i in 0..=u16::max_value() as u32 {
            if !self.players.contains_key(&(i as u16)) {
                id = i as u16;
                break;
            }
        }
        // Add the player's parts to the snake_parts
        self.snake_parts
            .extend(player.parts.iter().map(|pos| (*pos, (id, 1u8))));
        // Add the player object to the hashmap
        self.players.insert(id, player);

        // Send the client his ID:
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&[0x06]); // \x06 means that it's the player's ID and world size
        bytes.extend_from_slice(&id.to_be_bytes()[..]); // the id -> 2 bytes
        bytes.extend_from_slice(&(self.world_size.0).to_be_bytes()[..]); // world width -> 2 bytes
        bytes.extend_from_slice(&(self.world_size.1).to_be_bytes()[..]); // world height -> 2 bytes
        Server::send_to_stream(&mut stream, &bytes);

        // Increase the timeout and make the stream nonblocking
        stream
            .set_read_timeout(Some(Duration::new(30, 0)))
            .expect("set_read_timeout call failed");
        stream
            .set_nonblocking(true)
            .expect("set_nonblocking call failed");

        println!(
            "{} connected with nickname {}",
            match stream.peer_addr() {
                Ok(addr) => format!("{}", addr),
                Err(_) => "{unknown}".to_string(),
            },
            nickname
        );

        // Add the stream to streams hashmap
        self.client_streams.insert(id, stream);
    }
    pub fn ticks(
        game_speed: u8,
        read_player_input_sender: Sender<()>,
        move_snakes_sender: Sender<()>,
        send_game_data_sender: Sender<()>,
    ) {
        loop {
            read_player_input_sender.send(()).unwrap();
            move_snakes_sender.send(()).unwrap();
            send_game_data_sender.send(()).unwrap();
            // wait for next tick
            sleep(Duration::from_millis((1000f64 / game_speed as f64) as u64));
        }
    }
}
