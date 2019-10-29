#[cfg(feature = "bots")]
mod bot;

use rand::prelude::*;
use std::collections::{HashMap, VecDeque};
use std::io;
use std::io::prelude::*;
use std::net::{TcpListener, TcpStream, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::thread::sleep;
use std::time::{Duration, SystemTime, Instant};
use std::process::exit;

pub const DEFAULT_WORLD_SIZE: (u16, u16) = (200, 200);
pub const DEFAULT_MAX_PLAYERS: u16 = 50;
pub const DEFAULT_GAME_SPEED: u8 = 10;
pub const DEFAULT_FOOD_RATE: u8 = 10;

/// The main structure, holds everything related to server together
pub struct Server {
    /// Maximum limit of the players connected to this server
    pub max_players: u16,
    /// A hash map mapping player IDs to their structures
    pub players: Arc<Mutex<HashMap<u16, Player>>>,
    /// A hash map mapping player IDs to their corresponding TCP streams
    pub client_streams: Arc<Mutex<HashMap<u16, TcpStream>>>,
    /// The size of the world that the server hosts
    pub world_size: (u16, u16),
    /// A vector containing all objects in world: foods and snake parts
    pub world: Arc<Mutex<Vec<Field>>>,
    /// The amount of frames/ticks per second. Bigger number = faster gameplay
    pub game_speed: u8,
    /// How much food should be constantly in the world in relation to the world size
    pub food_rate: u8,
    /// The port that the server binds to
    pub port: u16,
}

/// Holds information about specific field in world and what's in it
#[derive(Copy, Clone, Debug)]
pub enum Field {
    /// The field is empty
    Empty,
    /// There's food on this field. Contains a number - how much food
    Food(u8),
    /// There's a snake part on this field. Contains the ID of the owner snake
    Snake(u16)
}

/// Contains data about a specific player
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
    /// Time when snake changed it's direction last time
    last_direction_change_time: u64,
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
        bot_amount: u16,
    ) {
        println!("Reserving memory for world... ({} bytes)", std::mem::size_of::<Field>() as u32 * world_size.0 as u32 * world_size.1 as u32);
        let now = Instant::now();

        let server = Server {
            max_players,
            players: Arc::new(Mutex::new(HashMap::new())),
            client_streams: Arc::new(Mutex::new(HashMap::new())),
            world_size,
            world: Arc::new(Mutex::new( vec![Field::Empty; world_size.0 as usize*world_size.1 as usize] )),
            game_speed,
            food_rate,
            port,
        };

        println!("Done! ({:.4} seconds)", now.elapsed().as_secs_f64());

        // Generate foods
        let amount_of_foods =
            ((world_size.0 as u32 * world_size.1 as u32) as f64 / food_rate as f64) as u32;
        println!("Generating food... ({})", amount_of_foods);
        let now = Instant::now();

        let mut rng = thread_rng();
        for _ in 0..amount_of_foods {
            server.add_food(&mut rng, &mut server.world.lock().unwrap());
        }

        println!("Done! ({:.4} seconds)", now.elapsed().as_secs_f64());

        // Start the thread for accepting new connections
        println!("Spawning a thread for accepting new connections...");
        // Make a clone of the server structure for the connections acceptor thread
        let server_clone = server.clone();
        thread::Builder::new()
            .name("connections_acceptor".to_string())
            .spawn(move || {
                server_clone.accept_connections()
            })
            .unwrap();

        // Wait for the connection acceptor to bind to the port
        sleep(Duration::from_secs(1));

        // Spawn the bots
        if bot_amount > 0 {
            println!("Spawning {} bots...", bot_amount);
        }
        for i in 0..bot_amount {
            // Generate a nickname for the bot
            let nickname = format!("bot_{}", i);
            thread::Builder::new()
                .name(nickname.clone())
                .spawn(move || {
                    loop { bot::Bot::start(port, &nickname); }
                })
                .unwrap();
        }
        println!("Server initialized");

        // Start the game logic
        loop {
            // Each loop is a 'tick'
            let tick_start = Instant::now();

            // Read snakes input
            server.read_players_input();

            // Move snakes
            server.move_snakes();

            // Send players game data
            server.send_data_to_players();

            // Wait for next tick, if need to
            let wait_for = Duration::from_micros((1000000f64 / server.game_speed as f64) as u64).checked_sub(tick_start.elapsed());
            if let Some(x) = wait_for {
                sleep(x);
            }
        }
    }
    /// Adds a single food object to a random place
    pub fn add_food(&self, rng: &mut ThreadRng, world_lock: &mut MutexGuard<Vec<Field>>) {
        let mut pos = (rng.gen::<u16>() % self.world_size.0, rng.gen::<u16>() % self.world_size.1);
        
        loop {
            // Check if there's any snake on the generated position
            let field = world_lock[self.coordinates_to_index(pos)];
            if let Field::Food(amount) = field {
                // Make sure that there's still room for more food
                if amount != 255 {
                    // Good position
                    break;
                }
            } else if let Field::Empty = field {
                // Good position
                break;
            }

            // Choose a new neighbor position and try again
            pos.0 += 1;
            if pos.0/self.world_size.0 == 1 {
                pos.0 = 0;
                pos.1 = (pos.1 + 1) % self.world_size.1;
            }
        }

        // Add the food to the generated position
        // If there's already food, just increment the count
        if let Field::Food(amount) = world_lock[self.coordinates_to_index(pos)] {
            world_lock[self.coordinates_to_index(pos)] = Field::Food(amount+1);
        } else {
            world_lock[self.coordinates_to_index(pos)] = Field::Food(1);
        }
    }
    /// Takes coordinates and returns an usize integer for indexing world
    #[inline(always)]
    pub fn coordinates_to_index(&self, coordinates: (u16, u16)) -> usize {
        ((coordinates.0 as usize) * self.world_size.1 as usize) + coordinates.1 as usize
    }
    /// Accepts and handles new connections
    pub fn accept_connections(self) {
        // First bind to the port and start listening
        println!("Binding to port {}", self.port);
        let listener = match TcpListener::bind(format!("0.0.0.0:{}", self.port)) {
            Ok(listener) => listener,
            Err(_) => {
                println!("Can't bind to port {}!", self.port);
                exit(1);
            }
        };
        loop {
            // Accept a new connection
            if let Ok((stream, addr)) = listener.accept() {
                // Set timeout to 60 seconds
                stream.set_read_timeout(Some(Duration::from_secs(60))).expect("set_read_timeout call failed");
                // Spawn a new thread for handling this new connection
                let server_clone = self.clone();
                thread::Builder::new()
                    .name("new_connection_handler".to_string())
                    .spawn(move || {
                        server_clone.handle_new_connection(stream, addr);
                    }).unwrap();
            }
        }
    }
    /// Handles a new connection, idk what else to say.
    pub fn handle_new_connection(self, mut stream: TcpStream, address: SocketAddr) {
        // Determine what the client wants:
        // - If they send a \x00 byte followed by a nickname, they're here to play
        // - If they send a \x01 byte they're here to fetch game stats: leaderboard and stuff
        let bytes = match read_from_stream(&mut stream) {
            Ok(bytes) => bytes,
            Err(_) => {
                // Conection lost already :O
                return;
            }
        };

        if bytes.is_empty() {
            // Client refuses to tell what he's here for
            // Drop connection
            return;
        }

        if bytes[0] == 0x00 {
            // They're here to play
            // Get the nickname
            let mut nickname: String = match std::str::from_utf8(&bytes[1..]) {
                Ok(string) => string.to_owned(),
                Err(_) => {
                    // Can't read username
                    // Send message and drop the connection
                    // \x05 at the beginning means that it's an error and that there's text following it
                    send_to_stream(&mut stream, b"\x05can't read nickname");
                    return;
                }
            };
            // Escape the username to make it consist only of visible ascii characters
            nickname = nickname.escape_default().to_string();
            // Make sure that the nickname is not too long and not too short
            if nickname.is_empty() || nickname.len() > 10 {
                // Send message and drop the connection
                // \x05 at the beginning means that it's an error and that there's text following it
                send_to_stream(&mut stream, b"\x05nickname too short/long");
                return;
            }
            let mut players = self.players.lock().unwrap();
            // Make sure the server is not full yet
            let playing_now = players.len() as u16;
            if playing_now >= self.max_players {
                // Send error and drop connection
                // \x05 at the beginning means that it's an error and that there's text following it
                send_to_stream(&mut stream, b"\x05server full");
                return;
            }
            // generate an ID for this new player
            let mut id: u16 = 0;
            for i in 0..=u16::max_value() as u32 {
                if !players.contains_key(&(i as u16)) {
                    id = i as u16;
                    break;
                }
            }
            // Make the stream nonblocking
            stream
                .set_nonblocking(true)
                .expect("set_nonblocking failed");
            
            // Add a new player instance to the game
            if let Err(_) = self.add_player(&mut players, &nickname, id) {
                println!("Failed to spawn a player because there's not enough space on world");
                send_to_stream(&mut stream, b"\x05not enough space in world. try again");
                return;
            }
            self.client_streams.lock().unwrap().insert(id, stream.try_clone().expect("try_clone failed!"));
            // drop the players lock
            drop(players);

            // Send the id to them
            let mut bytes: Vec<u8> = Vec::new();
            bytes.extend_from_slice(&[0x06]); // \x06 means that it's the player's ID and world size
            bytes.extend_from_slice(&id.to_be_bytes()[..]); // the id -> 2 bytes
            bytes.extend_from_slice(&(self.world_size.0).to_be_bytes()[..]); // world width -> 2 bytes
            bytes.extend_from_slice(&(self.world_size.1).to_be_bytes()[..]); // world height -> 2 bytes
            send_to_stream(&mut stream, &bytes);
            // Display a message
            if !address.ip().is_loopback() {
                println!("{} connected with nickname {}", address, nickname);
            }
        } else if bytes[0] == 0x01 {
            // Send the server status and drop connection
            self.send_server_data_to_stream(stream);
        }
    }
    /// Adds a player to the world
    pub fn add_player(&self, players_lock: &mut MutexGuard<HashMap<u16, Player>>, nickname: &str, id: u16) -> Result<(),()> {
        // Generate a Player object for our new player :)
        // Generate random direction
        let direction = Direction::from_byte(thread_rng().gen_range(0, 4) as u8);
        // Generate parts positions
        let parts = self.generate_snake_parts(direction, id)?;

        let player = Player {
            nickname: nickname.to_owned(),
            direction,
            last_direction: direction,
            parts,
            kills: 0,
            score: 0,
            fast_mode: false,
            last_direction_change_time: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("SystemTime before UNIX EPOCH! What the heck did you do bro")
                .as_secs(),
        };
        
        // Add the player object to the hashmap
        players_lock.insert(id, player);
        Ok(())
    }
    /// Generate a random position for a new snake to spawn to, without overlapping
    /// with other snakes or foods
    /// Returns `None` if no position to spawn the snake on was found
    pub fn generate_snake_parts(&self, direction: Direction, id: u16) -> Result<VecDeque<(u16, u16)>, ()> {
        let mut parts = VecDeque::with_capacity(3);
        let direction_vector = direction.to_vector();
        let mut head_pos: (u16, u16) = (
                thread_rng().gen_range(0, self.world_size.0) as u16,
                thread_rng().gen_range(0, self.world_size.1) as u16,
            );
        for _ in 0..(self.world_size.0 as u32*self.world_size.1 as u32) {
            head_pos.0 += 1;
            if head_pos.0 == self.world_size.0 {
                head_pos.0 = 0;
                head_pos.1 = (head_pos.1 + 1) % self.world_size.1;
            }
            let part2_pos = (
                (((head_pos.0 as i32) - direction_vector.0 + self.world_size.0 as i32) as u32 % self.world_size.0 as u32) as u16,
                (((head_pos.1 as i32) - direction_vector.1 + self.world_size.1 as i32) as u32 % self.world_size.1 as u32) as u16,
            );
            let part3_pos = (
                (((head_pos.0 as i32) - 2 * direction_vector.0 + self.world_size.0 as i32) as u32 % self.world_size.0 as u32) as u16,
                (((head_pos.1 as i32) - 2 * direction_vector.1 + self.world_size.1 as i32) as u32 % self.world_size.1 as u32) as u16,
            );
            // If there's another snake part or food already there, generate another position
            let mut world = self.world.lock().unwrap();

            if let Field::Snake(..) | Field::Food(..) = world[self.coordinates_to_index(head_pos)] {
                continue;
            }
            if let Field::Snake(..) | Field::Food(..) = world[self.coordinates_to_index(part2_pos)] {
                continue;
            }
            if let Field::Snake(..) | Field::Food(..) = world[self.coordinates_to_index(part3_pos)] {
                continue;
            }

            // All good bro 😎👍

            parts.push_front(head_pos);
            parts.push_front(part2_pos);
            parts.push_front(part3_pos);

            world[self.coordinates_to_index(head_pos)] = Field::Snake(id);
            world[self.coordinates_to_index(part2_pos)] = Field::Snake(id);
            world[self.coordinates_to_index(part3_pos)] = Field::Snake(id);

            return Ok(parts);
        }
        Err(())
    }
    /// Send server status to stream which requested it
    pub fn send_server_data_to_stream(&self, mut stream: TcpStream) {
        let mut bytes: Vec<u8> = Vec::new();
        // max players -> 2 bytes
        bytes.extend_from_slice(&self.max_players.to_be_bytes()[..]);
        // players playing now -> 2 bytes
        let playing_now = self.players.lock().expect(&format!("line: {}", line!())).len() as u16;
        bytes.extend_from_slice(&playing_now.to_be_bytes()[..]);
        // world size -> 4 bytes
        bytes.extend_from_slice(&self.world_size.0.to_be_bytes()[..]);
        bytes.extend_from_slice(&self.world_size.1.to_be_bytes()[..]);
        // food rate -> 1 byte
        bytes.push(self.food_rate);
        // game speed -> 1 byte
        bytes.push(self.game_speed);

        // TODO: send leaderboard

        send_to_stream(&mut stream, &bytes);
    }
    /// Iterates over all connected players and reads their inputs
    pub fn read_players_input(&self) {
        let mut players = self.players.lock().unwrap();
        let mut client_streams = self.client_streams.lock().unwrap();
        let ids: Vec<u16> = client_streams.iter().map(|(&id, _)| id).collect();
        for id in ids {
            loop {
                let bytes = match read_from_stream(client_streams.get_mut(&id).unwrap()) {
                    Ok(bytes) => bytes,
                    Err(io::ErrorKind::WouldBlock) => {
                        // This means that we have the beginning of a message but not the end,
                        // So we will have to wait a little longer before parsing it
                        // But for now just carry on, so we don't block the thread
                        break;
                    }
                    Err(e) => {
                        // Conection was lost
                        // Clean everything up and move on
                        println!(
                            "connection to player \"{}\" was lost: {:?}",
                            players[&id].nickname, e
                        );

                        // Remove the snake
                        self.remove_snake(id, &mut players, &mut self.world.lock().unwrap());
                        // Remove the stream object
                        client_streams.remove(&id);

                        break;
                    }
                };
                
                // Messages starting with \x02 contain a new direction that a snake faces
                if bytes.len() == 2 && bytes[0] == 0x02 {
                    let new_direction = Direction::from_byte(bytes[1]);
                    // Make sure that the snake isn't doing a 180 degree turn, 'cause that shit illegal
                    if new_direction.is_opposite_of(players[&id].last_direction) {
                        continue;
                    }

                    // Otherwise save the new direction
                    players.get_mut(&id).unwrap().direction = new_direction;
                }

                // Messages starting with \x08 are requests to toggle fast mode
                if bytes.len() == 1 && bytes[0] == 0x08 {
                    // Make sure the snake has at least 1 score
                    if players[&id].score == 0 {
                        continue;
                    }

                    // Ok, toggle it
                    players.get_mut(&id).unwrap().fast_mode = !players[&id].fast_mode;
                }
            }
        }
    }
    /// Removes the Snake structure from players hashmap, and removes snake's parts from world, adds food instead
    /// This method doesn't remove the stream from Server::client_streams though
    pub fn remove_snake(&self, id: u16, players_lock: &mut MutexGuard<HashMap<u16, Player>>, mut world_lock: &mut MutexGuard<Vec<Field>>) {
        // Generate food where the snake was
        let mut rng = thread_rng();
        for i in 0..players_lock[&id].score {
            match players_lock[&id].parts.get(i as usize) {
                Some(coordinates) => {
                    world_lock[self.coordinates_to_index(*coordinates)] = Field::Food(1);
                },
                None => {
                    self.add_food(&mut rng, &mut world_lock);
                },
            }
        }

        // Remove all left-over snake parts from world
        if (players_lock[&id].score as usize) < players_lock[&id].parts.len() {
            for i in 0..(players_lock[&id].parts.len()-players_lock[&id].score as usize) {
                let field = players_lock[&id].parts[i+players_lock[&id].score as usize];
                world_lock[self.coordinates_to_index(field)] = Field::Empty;
            }
        }

        // Remove the player object from the players list
        players_lock.remove(&id);
    }
    /// Moves all the snakes 1 field ahead to their facing direction, eating food along the way
    /// (if there's any), or killing them if they crash into other snakes
    /// Also checks if any snakes are AFK and kicks them
    pub fn move_snakes(&self) {
        // Move each snake to it's facing direction
        let mut players = self.players.lock().unwrap();
        let mut world = self.world.lock().unwrap();
        let ids: Vec<u16> = players.keys().copied().collect();
        'snake: for snake_id in ids {
            // Check when was the last direction change
            // The timeout is 60 seconds
            if players[&snake_id].last_direction_change_time + 60
                <= SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH).expect("bruh").as_secs()
            {
                println!("{} timed out. Kicking...", players[&snake_id].nickname);
                // Send a message to them telling them that they're dead
                // message starting with \x03 means that you died
                send_to_stream(
                    &mut self.client_streams.lock().unwrap().get_mut(&snake_id).unwrap(),
                    &[0x03],
                );

                // Kill it
                self.remove_snake(snake_id, &mut players, &mut world);
                self.client_streams.lock().unwrap().remove(&snake_id);

                // Move onto the next snake
                continue 'snake;
            }

            // If snake not long enough anymore, turn off fast mode
            let snake = players.get_mut(&snake_id).unwrap();
            if snake.fast_mode && snake.score < 1 {
                snake.fast_mode = false;
            }

            let mut moves = 1;

            // If snake in fast mode make it move twice
            if players[&snake_id].fast_mode {
                moves = 2;
            }

            // move it
            for _ in 0..moves {
                if players[&snake_id].direction != players[&snake_id].last_direction {
                    // save the time when this direction change happened
                    players.get_mut(&snake_id).unwrap().last_direction_change_time = SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH).expect("SystemTime before UNIX EPOCH!")
                        .as_secs();
                    // Change the last_direction
                    players.get_mut(&snake_id).unwrap().last_direction = players[&snake_id].direction;
                }

                // Calculate the new head position
                let mut new_head_pos = *players[&snake_id].parts.back().unwrap();

                let (dx, dy) = players[&snake_id].direction.to_vector();

                let width = self.world_size.0 as i32;
                let height = self.world_size.1 as i32;
                new_head_pos.0 = (((new_head_pos.0 as i32 + dx) + width) % width) as u16;
                new_head_pos.1 = (((new_head_pos.1 as i32 + dy) + height) % height) as u16;

                // If the head is on food, eat it
                if let Field::Food(amount) = world[self.coordinates_to_index(new_head_pos)] {
                    players.get_mut(&snake_id).unwrap().score += amount as u16;
                } else if let Field::Snake(id) = world[self.coordinates_to_index(new_head_pos)] {
                    // CRASH!
                    // Add a kill for the snake that killed it, unless it was a suicide
                    if id != snake_id {
                        players.get_mut(&id).unwrap().kills += 1;
                    }
                    // Send a message to them telling them that they're dead
                    // message starting with \x03 means that you died
                    send_to_stream(
                        &mut self.client_streams.lock().unwrap().get_mut(&snake_id).unwrap(),
                        &[0x03],
                    );

                    // Kill it
                    self.remove_snake(snake_id, &mut players, &mut world);
                    self.client_streams.lock().unwrap().remove(&snake_id);

                    // Move onto the next snake
                    continue 'snake;
                }

                // Add the new part to the head
                players.get_mut(&snake_id).unwrap().parts.push_back(new_head_pos);
                world[self.coordinates_to_index(new_head_pos)] = Field::Snake(snake_id);
            }

            // If in fast mode, remove 1 score
            if players[&snake_id].fast_mode {
                players.get_mut(&snake_id).unwrap().score -= 1;
            }

            let mut tail_pos = None;
            // If needed, remove parts from tail
            for _ in 0..((players[&snake_id].parts.len() - 3) as u16).saturating_sub(players[&snake_id].score) {
                tail_pos = Some(players.get_mut(&snake_id).unwrap().parts.pop_front().unwrap());
                world[self.coordinates_to_index(tail_pos.unwrap())] = Field::Empty;
            }

            // If was in fast mode, add food on tail
            if players[&snake_id].fast_mode {
                match tail_pos {
                    Some(pos) => {
                        world[self.coordinates_to_index(pos)] = Field::Food(1);
                    },
                    None => {
                        self.add_food(&mut thread_rng(), &mut world);
                    },
                }
            }
        }
    }
    /// Send game data to all connected players
    pub fn send_data_to_players(&self) {
        let players = self.players.lock().unwrap();
        let world = self.world.lock().unwrap();
        // First generate the general/shared part of the
        // buffer that's going to be sent to each player
        let mut bytes: Vec<u8> = Vec::new();

        // \x04 means that it's the game data
        bytes.push(0x04);

        // amount of snakes in total -> 2 bytes
        bytes.extend_from_slice(&(players.len() as u16).to_be_bytes()[..]);

        let snake_ids: Vec<u16> = players.keys().cloned().collect();
        for snake_id in &snake_ids {
            let snake = &players[&snake_id];
            bytes.extend_from_slice(&snake_id.to_be_bytes()[..]); // id -> 2 bytes
            bytes.push(snake.nickname.len() as u8); // nickname length -> 1 byte
            bytes.extend_from_slice(snake.nickname.as_bytes()); // nickname -> 1-20 bytes
            bytes.extend_from_slice(&snake.score.to_be_bytes()[..]); // score -> 2 bytes
            bytes.extend_from_slice(&snake.kills.to_be_bytes()[..]); // kills -> 2 bytes
            bytes.push(snake.fast_mode as u8);
            // fast mode -> 1 byte
        }

        let world_size = (self.world_size.0 as i32, self.world_size.1 as i32);

        // Now individual data for each player
        for id in snake_ids {
            let mut individual_bytes = bytes.clone();

            let player_head_pos = *players[&id].parts.back().unwrap();

            let mut temp_snakes: Vec<u8> = Vec::new();
            let mut temp_foods: Vec<u8> = Vec::new();

            // Iterate through every field in the view of the player
            for y in -14i32..15i32 {
                for x in -24i32..25i32 {
                    let field: (u16, u16) = (
                        ((player_head_pos.0 as i32 + x + world_size.0*2) % world_size.0) as u16,
                        ((player_head_pos.1 as i32 + y + world_size.1*2) % world_size.1) as u16,
                    );
                    // Check if there's any snake here
                    if let Field::Snake(snake_id) = world[self.coordinates_to_index(field)] {
                        // there is
                        temp_snakes.push((x as i8).to_be_bytes()[0]); // x pos (relative to player's head) of snake part -> 1 byte
                        temp_snakes.push((y as i8).to_be_bytes()[0]); // y pos (relative to player's head) of snake part -> 1 byte
                        // id of the snake that the part belongs to -> 2 bytes
                        temp_snakes
                            .extend_from_slice(&snake_id.to_be_bytes()[..]);
                    
                    // Check if there's any food here
                    } else if let Field::Food(amount) = world[self.coordinates_to_index(field)] {
                        // there is
                        temp_foods.push((x as i8).to_be_bytes()[0]); // x pos (relative to player's head) of food -> 1 byte
                        temp_foods.push((y as i8).to_be_bytes()[0]); // y pos (relative to player's head) of food -> 1 byte
                        // amount of food here -> 1 byte
                        temp_foods.push(amount.to_be_bytes()[0]); 
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
            send_to_stream(
                &mut self.client_streams.lock().unwrap()
                    .get_mut(&id).unwrap(),
                &individual_bytes[..],
            );
        }
    }
}

impl Clone for Server {
    fn clone(&self) -> Self {
        Server {
            max_players: self.max_players,
            players: self.players.clone(),
            client_streams: self.client_streams.clone(),
            world_size: self.world_size,
            world: self.world.clone(),
            game_speed: self.game_speed,
            food_rate: self.food_rate,
            port: self.port,
        }
    }
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

/// Sends bytes to stream, with the buffer length appended to the beginning as an u16 integer
pub fn send_to_stream(stream: &mut TcpStream, data: &[u8]) {
    let size: [u8; 2] = u16::to_be_bytes(data.len() as u16);
    let mut message: Vec<u8> = Vec::new();
    message.extend_from_slice(&size);
    message.extend_from_slice(data);

    let _ = stream.write_all(&message);
}