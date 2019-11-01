use rand::prelude::*;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;

/// A bot structure, holds everything together
pub struct Bot {
    stream: TcpStream,
    my_id: u16,
    nickname: String,
}

impl Bot {
    pub fn start(port: u16, nickname: &str) {
        let mut stream = match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => stream,
            Err(e) => {
                println!("Bot {} couldn't connect to server: {}", nickname, e);
                return;
            }
        };
        // Send my nickname as a request to connect to the game
        let mut bytes: Vec<u8> = vec![0x00];
        bytes.extend_from_slice(nickname.as_bytes());
        send_to_stream(&mut stream, &bytes);
        // Read the response
        let my_id = match read_from_stream(&mut stream) {
            Err(_) => {
                println!(
                    "Bot {} lost connection after requesting to join game",
                    nickname
                );
                return;
            }
            Ok(bytes) => {
                if bytes[0] == 0x05 {
                    // It's an error
                    println!(
                        "Bot {} received an error from server: {}",
                        nickname,
                        std::str::from_utf8(&bytes[1..]).unwrap_or("{corrupted error}")
                    );
                    return;
                } else if bytes[0] == 0x06 && bytes.len() == 7 {
                    // It's a confirmation that I joined the game, with my ID and the world size
                    u16::from_be_bytes([bytes[1], bytes[2]])
                } else {
                    println!(
                        "Bot {} received a corrupted message from server: disconnecting.",
                        nickname
                    );
                    return;
                }
            }
        };

        let mut bot = Bot {
            stream,
            my_id,
            nickname: nickname.to_string(),
        };

        // Then just read from server, and respond to each frame with a direction
        loop {
            // Read from stream
            match read_from_stream(&mut bot.stream) {
                Ok(data) => {
                    // Handle the data
                    if let Some(()) = bot.handle_server_data(data) {
                        break;
                    }
                }
                Err(e) => {
                    println!("Bot {} lost connection to server: {:?}", bot.nickname, e);
                    break;
                }
            }
        }
    }
    /// Handles the data sent by server and acts accordingly
    pub fn handle_server_data(self: &mut Self, data: Vec<u8>) -> Option<()> {
        if data.is_empty() {
            println!("Bot {} received empty packed.", self.nickname);
            return None;
        }
        if data[0] == 0x03 {
            // We died
            return Some(());
        }
        // We only care about the game data: \x04
        if data[0] != 0x04 {
            // Break the loop and spawn a new bot
            return None;
        }

        // Parse the data
        let data = self.parse_game_data(&data[1..]);

        // Decide the new direction based on that data
        self.turn(data);
        None
    }
    /// Parses the game data sent by server
    pub fn parse_game_data(
        self: &mut Self,
        data: &[u8],
    ) -> (HashMap<(i8, i8), u8>, HashMap<(i8, i8), u16>, bool) {
        // Parse the data
        let mut i = 0; // next byte to read

        // First 2 bytes are the amount of snakes in total
        let snake_amount = u16::from_be_bytes([data[i], data[i + 1]]);
        i += 2;
        // Then a lot of snake data that we don't need follows
        // We need to know if we're in fast mode though
        let mut in_fast_mode = false;
        for _snake in 0..snake_amount {
            // Check if it's our ID
            let id = u16::from_be_bytes([data[i], data[i + 1]]);
            for _character in 0..u8::from_be_bytes([data[i + 2]]) {
                i += 1;
            }
            if id == self.my_id {
                in_fast_mode = u8::from_be_bytes([data[i + 7]]) == 1;
            }
            i += 12;
        }

        // Foods
        let foods_amount = u16::from_be_bytes([data[i], data[i + 1]]);
        i += 2;
        let mut foods: HashMap<(i8, i8), u8> = HashMap::new();
        for _food in 0..foods_amount {
            foods.insert(
                (
                    i8::from_be_bytes([data[i]]),     // X pos of food relative to my head
                    i8::from_be_bytes([data[i + 1]]), // Y pos of food relative to my head
                ),
                u8::from_be_bytes([data[i + 2]]), // amount of food there
            );
            i += 3;
        }

        // Snake parts
        let snake_parts_amount = u16::from_be_bytes([data[i], data[i + 1]]);
        i += 2;
        // A hashmap mapping relative positions to snake IDs
        let mut snake_parts: HashMap<(i8, i8), u16> = HashMap::new();
        for _snake_part in 0..snake_parts_amount {
            snake_parts.insert(
                (
                    i8::from_be_bytes([data[i]]),     // X pos of part relative to my head
                    i8::from_be_bytes([data[i + 1]]), // Y pos of part relative to my head
                ),
                u16::from_be_bytes([data[i + 2], data[i + 3]]), // snake id
            );
            i += 4;
        }

        (foods, snake_parts, in_fast_mode)
    }
    /// Decides what direction to move and sends that direction to server
    pub fn turn(self: &mut Self, data: (HashMap<(i8, i8), u8>, HashMap<(i8, i8), u16>, bool)) {
        // if there's another snake within 3 fields, try to get into fast mode
        let mut other_snakes_nearby = false;
        'x: for x in -3i8..=3i8 {
            for y in -3i8..=3i8 {
                if let Some(id) = data.1.get(&(x, y)) {
                    if *id != self.my_id {
                        other_snakes_nearby = true;
                        break 'x;
                    }
                }
            }
        }
        // do I need to be in fast mode XOR am I in fast mode
        // So basically, if they're different, randomly toggle fast mode.
        if other_snakes_nearby ^ data.2 && thread_rng().gen_range(0, 5) == 0 {
            // Ask server to toggle fast-mode
            self.toggle_fast_mode();
        }

        // Find the best food to try to eat
        // It must give as much score as possible
        // And be as close as possible
        //				 ((x  , y  ), score)
        let mut target = ((0i8, 0i8), 0f32);
        for (food_pos, amount) in &data.0 {
            let s = (*amount as f32).powf(2.0)
                / ((food_pos.0 as i16).pow(2) + (food_pos.1 as i16).pow(2)) as f32;
            if s > target.1 {
                target = (*food_pos, s);
            }
        }

        let norm_vec = (
            (target.0).0 as f32
                / ((((target.0).0 as i16).pow(2) + ((target.0).1 as i16).pow(2)) as f32).sqrt(),
            (target.0).1 as f32
                / ((((target.0).0 as i16).pow(2) + ((target.0).1 as i16).pow(2)) as f32).sqrt(),
        );
        let dir = if ((norm_vec.0).abs() - (norm_vec.1).abs()).abs() <= std::f32::EPSILON {
            if thread_rng().gen_range(0, 2) == 0 {
                ((norm_vec.0).round() as i8, 0i8)
            } else {
                (0i8, (norm_vec.1).round() as i8)
            }
        } else {
            (
                if (norm_vec.0).abs() > (norm_vec.1).abs() {
                    (norm_vec.0).round() as i8
                } else {
                    0i8
                },
                if (norm_vec.1).abs() > (norm_vec.0).abs() {
                    (norm_vec.1).round() as i8
                } else {
                    0i8
                },
            )
        };

        if (data.1).contains_key(&(dir.0, dir.1)) {
            // we might crash into a snake if we go there
            // Check other directions
            if !(data.1).contains_key(&(1, 0)) {
                self.send_direction(3);
                return;
            } else if !(data.1).contains_key(&(-1, 0)) {
                self.send_direction(1);
                return;
            } else if !(data.1).contains_key(&(0, 1)) {
                self.send_direction(2);
                return;
            } else {
                self.send_direction(0);
                return;
            }
        }

        self.send_direction(match dir {
            (1, 0) => 2,
            (-1, 0) => 0,
            (0, 1) => 3,
            (0, -1) => 1,
            _ => thread_rng().gen_range(0, 4),
        });
    }
    /// Sends a new direction to server
    pub fn send_direction(self: &mut Self, direction: u8) {
        let mut bytes: Vec<u8> = vec![0x02];
        bytes.push(direction);
        send_to_stream(&mut self.stream, &bytes);
    }
    /// Sends a message to server asking to toggle fast mode
    pub fn toggle_fast_mode(self: &mut Self) {
        // \x08 means "toggle fast mode for me please"
        let bytes = vec![0x08];
        send_to_stream(&mut self.stream, &bytes);
    }
}

/// Sends bytes to stream, with the buffer length appended to the beginning as an u8 integer
pub fn send_to_stream(stream: &mut TcpStream, data: &[u8]) {
    let size: [u8; 1] = u8::to_be_bytes(data.len() as u8);
    let mut message: Vec<u8> = Vec::new();
    message.extend_from_slice(&size);
    message.extend_from_slice(data);

    stream.write_all(&message).unwrap();
}
/// Reads 1 message from stream
/// Returns `Ok(bytes)` if the reading was successful
/// and `Err(e)` if an error was encountered while reading
pub fn read_from_stream(stream: &mut TcpStream) -> Result<Vec<u8>, std::io::ErrorKind> {
    // Figure out the size of the incoming message
    let mut size = [0u8; 2];
    if let Err(e) = stream.read_exact(&mut size) {
        return Err(e.kind());
    }
    let size = u16::from_be_bytes(size);

    // Get the actual message
    let mut bytes = vec![0u8; size as usize];
    if let Err(e) = stream.read_exact(&mut bytes) {
        return Err(e.kind());
    }
    Ok(bytes)
}
