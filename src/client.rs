use crossterm_input::{input, InputEvent, KeyEvent, RawScreen, SyncReader};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;

const SNAKE_COLORS: [&str; 6] = [
    "\x1b[107m", // White
    "\x1b[46m",  // Cyan
    "\x1b[45m",  // Magenta
    "\x1b[44m",  // Blue
    "\x1b[43m",  // Yellow
    "\x1b[42m",  // Green
];

const FOOD_COLORS: [&str; 6] = [
    "\x1b[102m", // Green = 1 food
    "\x1b[103m", // Yellow = 2 foods
    "\x1b[104m", // Blue = 4 foods
    "\x1b[105m", // Magenta = 8 foods
    "\x1b[106m", // Cyan = 16 foods
    "\x1b[41m",  // Red = 32 foods
];

/// Connects to the server and starts the client
pub fn start(ip: String, port: u16, nickname: String) {
    // make sure to put the terminal back into cooked mode before exiting
    let _guard = scopeguard::guard((), |_| {
        println!("\x1b[?25h");
        RawScreen::disable_raw_mode().expect("Failed to put terminal into cooked mode.");
    });

    println!("connecting to {}:{} with nickname {}", ip, port, nickname);
    let mut stream = match TcpStream::connect((&ip[..], port)) {
        Ok(stream) => stream,
        Err(e) => {
            println!("Couldn't connect to host: {}", e);
            return;
        }
    };
    // Send my nickname as a request to connect to the game
    let mut bytes: Vec<u8> = vec![0x00];
    bytes.extend_from_slice(nickname.as_bytes());
    send_to_stream(&mut stream, &bytes);
    // Read the response
    let (my_id, world_size) = match read_from_stream(&mut stream) {
        Err(_) => {
            println!("Connection lost after requesting to join game");
            return;
        }
        Ok(bytes) => {
            if bytes[0] == 0x05 {
                // It's an error
                println!(
                    "Error from server: {}",
                    std::str::from_utf8(&bytes[1..]).unwrap_or("{corrupted error}")
                );
                return;
            } else if bytes[0] == 0x06 && bytes.len() == 7 {
                // It's a confirmation that I joined the game, with my ID and the world size
                (
                    u16::from_be_bytes([bytes[1], bytes[2]]),
                    (
                        u16::from_be_bytes([bytes[3], bytes[4]]),
                        u16::from_be_bytes([bytes[5], bytes[6]]),
                    ),
                )
            } else {
                println!("Corrupted message from server: disconnecting.");
                return;
            }
        }
    };
    println!("Connected successfully! My ID: {}", my_id);

    // Get the terminal ready
    let input = input();
    input
        .disable_mouse_mode()
        .expect("can't disable mouse mode");
    let input = input.read_sync();
    // put it into raw mode
    RawScreen::into_raw_mode()
        .expect("Failed to put terminal into raw mode.")
        .disable_drop();

    // Spawn the thread for handling user input and sending to server
    let stream_clone = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => {
            println!("Couldn't clone the TCP stream to server.");
            return;
        }
    };

    // A thread for handling user input
    thread::Builder::new()
        .name("input_handler".to_string())
        .spawn(move || handle_input(stream_clone, input))
        .unwrap();

    // Make some room for the game frames, without overwriting history text
    // And hide the carriage
    print!("\x1b[2J\x1b[?25l");

    // The main thread will be reading data from server and drawing it for the user
    loop {
        let bytes = match read_from_stream(&mut stream) {
            Err(_) => {
                println!("Unexpectedly lost connection to server.");
                return;
            }
            Ok(bytes) => bytes,
        };
        // Handle it and draw the frame
        handle_server_message(bytes, my_id, world_size);
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
/// Reads all input from user and sends valid directions to server
pub fn handle_input(mut stream: TcpStream, input: SyncReader) {
    for event in input {
        match event {
            // ctrl-c or Q to quit the game
            InputEvent::Keyboard(KeyEvent::Ctrl('c'))
            | InputEvent::Keyboard(KeyEvent::Char('q')) => {
                // Put the terminal into cooked mode and terminate process
                println!("\x1b[?25h");
                RawScreen::disable_raw_mode().expect("Failed to put terminal into cooked mode.");
                std::process::exit(0);
            }
            // A or Left arrow - move left
            InputEvent::Keyboard(KeyEvent::Char('a')) | InputEvent::Keyboard(KeyEvent::Left) => {
                send_direction(&mut stream, 0);
            }
            // S or Down arrow - move down
            InputEvent::Keyboard(KeyEvent::Char('s')) | InputEvent::Keyboard(KeyEvent::Down) => {
                send_direction(&mut stream, 3);
            }
            // D or Right arrow - move right
            InputEvent::Keyboard(KeyEvent::Char('d')) | InputEvent::Keyboard(KeyEvent::Right) => {
                send_direction(&mut stream, 2);
            }
            // W or Up arrow - move up
            InputEvent::Keyboard(KeyEvent::Char('w')) | InputEvent::Keyboard(KeyEvent::Up) => {
                send_direction(&mut stream, 1);
            }
            // Space to toggle fast mode
            InputEvent::Keyboard(KeyEvent::Char(' ')) => {
                toggle_fast_mode(&mut stream);
            }
            _ => (),
        }
    }
}
/// Sends a new direction to server
pub fn send_direction(mut stream: &mut TcpStream, direction: u8) {
    let mut bytes: Vec<u8> = vec![0x02];
    bytes.push(direction);
    send_to_stream(&mut stream, &bytes);
}
/// Sends a message to server asking to toggle fast mode
pub fn toggle_fast_mode(mut stream: &mut TcpStream) {
    // \x08 means "toggle fast mode for me please"
    let bytes = vec![0x08];
    send_to_stream(&mut stream, &bytes);
}
/// Handles data sent by server and if the data is new game data, draws the new frame to user
pub fn handle_server_message(data: Vec<u8>, my_id: u16, world_size: (u16, u16)) {
    // Messages starting with:
    //  - \x03 mean that I died
    //  - \x04 mean that it's the game data
    if data.len() == 1 && data[0] == 0x03 {
        // Exit
        RawScreen::disable_raw_mode().expect("Failed to put terminal into cooked mode.");
        println!("\r\nYou died!\x1b[?25h");
        std::process::exit(0);
    } else if data[0] == 0x04 {
        // Parse the data
        let mut i = 1; // next byte to read

        // First 2 bytes are the amount of snakes in total
        let snake_amount = u16::from_be_bytes([data[i], data[i + 1]]);
        i += 2;
        // A hashmap pointing snake ID to it's nickname, score and amount of kills
        let mut snakes: HashMap<u16, (String, u16, u16, bool)> = HashMap::new();
        for _snake in 0..snake_amount {
            let id = u16::from_be_bytes([data[i], data[i + 1]]);
            i += 2;
            let nickname_length = u8::from_be_bytes([data[i]]);
            i += 1;
            let mut nickname = String::new();
            for _character in 0..nickname_length {
                nickname.push(char::from(data[i]));
                i += 1;
            }
            // I couldn't find a way to decently display the nicknames of other snakes to the user
            // but if you can, pull requests are VERY welcome regarding this. - Ponas Kovas
            let score = u16::from_be_bytes([data[i], data[i + 1]]);
            i += 2;
            let kills = u16::from_be_bytes([data[i], data[i + 1]]);
            i += 2;
            let fast_mode = u8::from_be_bytes([data[i]]) == 1;
            i += 1;
            snakes.insert(id, (nickname, score, kills, fast_mode));
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
                u16::from_be_bytes([data[i + 2], data[i + 3]]), // snake ID
            );
            i += 4;
        }

        let my_position = (
            u16::from_be_bytes([data[i], data[i + 1]]),
            u16::from_be_bytes([data[i + 2], data[i + 3]]),
        );

        // OK, all the data is read and parsed - time to draw the frame
        draw(my_id, snakes, foods, snake_parts, my_position, world_size);
    }
}

/// Draws the new frame
pub fn draw(
    my_id: u16,
    snakes_info: HashMap<u16, (String, u16, u16, bool)>,
    foods: HashMap<(i8, i8), u8>,
    snake_parts: HashMap<(i8, i8), u16>,
    my_pos: (u16, u16),
    world_size: (u16, u16),
) {
    let mut to_print = String::new();
    // First - move the cursor to the top left corner of the terminal
    to_print += "\x1b[H";

    // Get terminal size
    let mut real_terminal_size = (98, 30); // default if couldnt fetch
    if let Some((w, h)) = term_size::dimensions() {
        real_terminal_size = (w as u16, h as u16);
    }

    // Get the dimensions of frame that would fit in this terminal
    let frame_size = ((real_terminal_size.0 / 2), real_terminal_size.1 - 1);

    // Construct the ranges of field positions relative to center of terminal
    let width = (-(frame_size.0 as i16 / 2) as i8)
        ..((frame_size.0 as i16 - (frame_size.0 as i16 / 2)) as i8);
    let height = (-(frame_size.1 as i16 / 2) as i8)
        ..((frame_size.1 as i16 - (frame_size.1 as i16 / 2)) as i8);

    let right_side_padding = &" ".repeat(real_terminal_size.0 as usize - width.clone().count() * 2);

    // Iterate through all fields in the constructed ranges and check if there's anything there
    for y in height {
        for x in width.clone() {
            if foods.contains_key(&(x, y)) {
                // Figure out the color based on how much food there is
                let amount = foods[&(x, y)];
                if amount >= 32 {
                    to_print += FOOD_COLORS[5];
                } else if amount >= 16 {
                    to_print += FOOD_COLORS[4];
                } else if amount >= 8 {
                    to_print += FOOD_COLORS[3];
                } else if amount >= 4 {
                    to_print += FOOD_COLORS[2];
                } else if amount >= 2 {
                    to_print += FOOD_COLORS[1];
                } else if amount >= 1 {
                    to_print += FOOD_COLORS[0];
                }
                to_print += "\x1b[1m\x1b[30m[]\x1b[0m"; // a food square
            } else if snake_parts.contains_key(&(x, y)) {
                // Get the color
                to_print += SNAKE_COLORS[(snake_parts[&(x, y)] % 6) as usize]; // color
                                                                               // if snake in fast mode, draw it using ++
                if snakes_info[&snake_parts[&(x, y)]].3 {
                    to_print += "\x1b[30m++"; // body
                } else {
                    to_print += "  "; // body
                }
                to_print += "\x1b[0m"; // reset colors
            } else {
                to_print += "  "; // a background colored square
            }
        }
        to_print += right_side_padding;
    }
    // Add the status bar at the bottom
    let status_text = format!(
        "{nickname}: {score} ({score_place}), {kills} kills ({kills_place})",
        nickname = snakes_info[&my_id].0,
        score = snakes_info[&my_id].1,
        kills = snakes_info[&my_id].2,
        score_place = get_place_by_score(&snakes_info, my_id),
        kills_place = get_place_by_kills(&snakes_info, my_id)
    );
    let position_text = if real_terminal_size.0 as usize >= status_text.len() + 8 {
        format!(
            "{:3.0}#{:3.0}",
            (1000f64 * my_pos.0 as f64 / world_size.0 as f64),
            (1000f64 * my_pos.1 as f64 / world_size.1 as f64)
        )
    } else {
        "".to_string()
    };
    let snakes_count_text = format!("{} snakes", snakes_info.len());
    to_print += &(SNAKE_COLORS[(my_id % 6) as usize].to_owned() + "\x1b[30m"); // colors
    to_print += &snakes_count_text;
    to_print += &" ".repeat(
        ((real_terminal_size.0 as usize - status_text.len()) as f64 / 2f64).floor() as usize
            - snakes_count_text.len(),
    );
    to_print += &status_text;
    to_print += &" ".repeat(
        ((real_terminal_size.0 as usize - status_text.len()) as f64 / 2f64).ceil() as usize
            - position_text.len(),
    );
    to_print += &position_text;
    to_print += "\x1b[0m"; // reset colors

    // Print and flush the output
    print!("{}", to_print);
    std::io::stdout().flush().unwrap();
}
/// Get place amongst all alive snakes sorting by score
pub fn get_place_by_score(snakes_data: &HashMap<u16, (String, u16, u16, bool)>, id: u16) -> String {
    // Get the scores and sort them
    let mut scores: Vec<u16> = snakes_data
        .iter()
        .map(|(_id, (_nickname, score, _kills, _fast_mode))| *score)
        .collect();
    scores.sort();
    scores.reverse();
    let mut place: u16 = 1;
    // Iterate through all the scores to find out the place
    for score in scores {
        if score == snakes_data[&id].1 {
            break;
        }
        place += 1;
    }
    let place = place.to_string();
    let last_digit = place.chars().last().unwrap();
    if last_digit == '1' {
        place + &"st".to_string()
    } else if last_digit == '2' {
        place + &"nd".to_string()
    } else if last_digit == '3' {
        place + &"rd".to_string()
    } else {
        place + &"th".to_string()
    }
}
/// Get place amongst all alive snakes sorting by kills
pub fn get_place_by_kills(snakes_data: &HashMap<u16, (String, u16, u16, bool)>, id: u16) -> String {
    // Get the kills and sort them
    let mut kills: Vec<u16> = snakes_data
        .iter()
        .map(|(_id, (_nickname, _score, kills, _fast_mode))| *kills)
        .collect();
    kills.sort();
    kills.reverse();
    let mut place: u16 = 1;
    // Iterate through all the kills to find out the place
    for amount in kills {
        if amount == snakes_data[&id].2 {
            break;
        }
        place += 1;
    }
    let place = place.to_string();
    let last_digit = place.chars().last().unwrap();
    if last_digit == '1' {
        place + &"st".to_string()
    } else if last_digit == '2' {
        place + &"nd".to_string()
    } else if last_digit == '3' {
        place + &"rd".to_string()
    } else {
        place + &"th".to_string()
    }
}
