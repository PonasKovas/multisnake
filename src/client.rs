use crossterm::input::AsyncReader;
use crossterm::{input, AlternateScreen, InputEvent, KeyEvent, RawScreen};
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::io::{stdin, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::sleep;
use std::time::Duration;

#[derive(Copy, Clone, Debug)]
enum ShowLeaderboard {
    Hide = 0,
    ByScore,
    ByKills,
}

lazy_static! {
    static ref SHOW_LEADERBOARD: Mutex<ShowLeaderboard> = Mutex::new(ShowLeaderboard::ByScore);
}

const SNAKE_COLORS: [&str; 9] = [
    "\x1b[41;30;1m",  // Red
    "\x1b[46;30;1m",  // Cyan
    "\x1b[100;97;1m", // Dark Gray
    "\x1b[101;30;1m", // Light Red
    "\x1b[102;30;1m", // Light Green
    "\x1b[103;30;1m", // Light Yellow
    "\x1b[104;30;1m", // Light Blue
    "\x1b[105;30;1m", // Light Magenta
    "\x1b[106;30;1m", // Light Cyan
];

// In tuples, first is for foreground, second is for background
const FOOD_COLORS: [(&str, &str); 4] = [
    ("\x1b[32m", "\x1b[42m"), // Green = 1 food
    ("\x1b[33m", "\x1b[43m"), // Yellow = 2 foods
    ("\x1b[34m", "\x1b[44m"), // Blue = 5 foods
    ("\x1b[35m", "\x1b[45m"), // Magenta = 11 or more foods
];

// Magic networking bytes:
const MAGIC_NET_REQUEST_TO_PLAY: u8 = 0x00;
const MAGIC_NET_CHANGE_DIRECTION: u8 = 0x02;
const MAGIC_NET_DEATH: u8 = 0x03;
const MAGIC_NET_GAME_DATA: u8 = 0x04;
const MAGIC_NET_ERROR: u8 = 0x05;
const MAGIC_NET_JOINED_GAME: u8 = 0x06;
const MAGIC_NET_TOGGLE_FAST: u8 = 0x08;
const MAGIC_NET_EXIT: u8 = 0x09;

pub enum Exit {
    Continue,
    Death,
}

/// Connects to the server and starts the client
pub fn start(ip: String, port: u16, nickname: String) {
    println!("connecting to {}:{} with nickname {}", ip, port, nickname);
    let mut stream = match TcpStream::connect((&ip[..], port)) {
        Ok(stream) => stream,
        Err(e) => {
            println!("Couldn't connect to host: {}", e);
            return;
        }
    };

    // Send my nickname as a request to connect to the game
    let mut bytes: Vec<u8> = vec![MAGIC_NET_REQUEST_TO_PLAY];
    bytes.extend_from_slice(nickname.as_bytes());
    send_to_stream(&mut stream, &bytes);

    // Read the response
    let (my_id, world_size) = match read_from_stream(&mut stream) {
        Err(_) => {
            println!("Connection lost after requesting to join game");
            return;
        }
        Ok(bytes) => {
            if bytes[0] == MAGIC_NET_ERROR {
                // It's an error
                println!(
                    "Error from server: {}",
                    std::str::from_utf8(&bytes[1..]).unwrap_or("{corrupted error}")
                );
                return;
            } else if bytes[0] == MAGIC_NET_JOINED_GAME && bytes.len() == 7 {
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
    println!("Connected successfully!");

    // Spawn the thread for handling user input and sending to server
    let stream_ref = Arc::new(Mutex::new(Some(
        stream
            .try_clone()
            .expect("Couldn't clone the TCP stream to server."),
    )));
    let stream_ref_clone = stream_ref.clone();
    let exit_input_handler = Arc::new(AtomicBool::new(false));
    let exit_input_handler_clone = exit_input_handler.clone();
    let input = input();
    let async_reader = input.read_async();

    // Get the terminal ready
    // Hide the carriage
    print!("\x1b[?25l");
    // Move to an alternate screen and into raw mode
    let alternate_screen_guard = AlternateScreen::to_alternate(true)
        .expect("Failed to put terminal into alternative screen.");

    // A thread for handling user input
    let join_handle = thread::Builder::new()
        .name("input_handler".to_string())
        .spawn(move || {
            handle_input(
                stream_ref_clone,
                async_reader,
                exit_input_handler_clone,
                alternate_screen_guard,
            )
        })
        .unwrap();

    // The main thread will be reading data from server and drawing it for the user
    loop {
        let bytes = match read_from_stream(&mut stream) {
            Err(_) => {
                exit_input_handler.store(true, Ordering::Relaxed);
                join_handle.join().unwrap();
                println!("Unexpectedly lost connection to server.");
                return;
            }
            Ok(bytes) => bytes,
        };
        // Handle it and draw the frame
        if let Exit::Death = handle_server_message(bytes, my_id, world_size) {
            *stream_ref.lock().unwrap() = None;
            if let Some((w, h)) = term_size::dimensions() {
                let text = "You died! Play again? [y/n]";
                let line = (h - 1) / 2;
                let column = (w - text.len()) / 2;
                print!(
                    "\x1b[{line};{column}H\x1b[107;30;1m{text}\x1b[0m",
                    line = line,
                    column = column,
                    text = text
                );
                std::io::stdout().flush().unwrap();
                let stdin = stdin();
                let mut stdinlock = stdin.lock();
                let mut c = [0u8];
                loop {
                    stdinlock.read_exact(&mut c[..]).unwrap();
                    if c[0] == b'y' {
                        drop(stdinlock);
                        exit_input_handler.store(true, Ordering::Relaxed);
                        join_handle.join().unwrap();
                        start(ip, port, nickname);
                        return;
                    }
                    if c[0] == b'n' {
                        break;
                    }
                }
            }
            exit_input_handler.store(true, Ordering::Relaxed);
            join_handle.join().unwrap();
            return;
        }
    }
}

/// Reads all input from user and sends valid directions to server
pub fn handle_input(
    stream: Arc<Mutex<Option<TcpStream>>>,
    mut input: AsyncReader,
    exit: Arc<AtomicBool>,
    altscreen_guard: AlternateScreen,
) {
    loop {
        if let Some(event) = input.next() {
            match event {
                // ctrl-c or Q to quit the game
                InputEvent::Keyboard(KeyEvent::Ctrl('c'))
                | InputEvent::Keyboard(KeyEvent::Char('q')) => {
                    if let Some(s) = stream.lock().unwrap().as_mut() {
                        // Show carriage, switch to main screen and terminate process
                        altscreen_guard.to_main().unwrap();
                        RawScreen::disable_raw_mode().unwrap();
                        print!("\x1b[?25h");
                        std::io::stdout().flush().unwrap();
                        // Send message to server
                        send_to_stream(s, &[MAGIC_NET_EXIT]);
                        std::process::exit(0);
                    }
                }
                // A or Left arrow - move left
                InputEvent::Keyboard(KeyEvent::Char('a'))
                | InputEvent::Keyboard(KeyEvent::Left) => {
                    if let Some(s) = stream.lock().unwrap().as_mut() {
                        send_direction(s, 0);
                    }
                }
                // S or Down arrow - move down
                InputEvent::Keyboard(KeyEvent::Char('s'))
                | InputEvent::Keyboard(KeyEvent::Down) => {
                    if let Some(s) = stream.lock().unwrap().as_mut() {
                        send_direction(s, 3);
                    }
                }
                // D or Right arrow - move right
                InputEvent::Keyboard(KeyEvent::Char('d'))
                | InputEvent::Keyboard(KeyEvent::Right) => {
                    if let Some(s) = stream.lock().unwrap().as_mut() {
                        send_direction(s, 2);
                    }
                }
                // W or Up arrow - move up
                InputEvent::Keyboard(KeyEvent::Char('w')) | InputEvent::Keyboard(KeyEvent::Up) => {
                    if let Some(s) = stream.lock().unwrap().as_mut() {
                        send_direction(s, 1);
                    }
                }
                // Space to toggle fast mode
                InputEvent::Keyboard(KeyEvent::Char(' ')) => {
                    if let Some(s) = stream.lock().unwrap().as_mut() {
                        toggle_fast_mode(s);
                    }
                }
                // L to toggle leaderboard
                InputEvent::Keyboard(KeyEvent::Char('l')) => {
                    // Toggle
                    let next = (*SHOW_LEADERBOARD.lock().unwrap() as u8 + 1) % 3;
                    *SHOW_LEADERBOARD.lock().unwrap() = match next {
                        0 => ShowLeaderboard::Hide,
                        1 => ShowLeaderboard::ByScore,
                        _ => ShowLeaderboard::ByKills,
                    }
                }
                _ => (),
            }
        }
        // If we need to exit, exit
        if exit.load(Ordering::Relaxed) {
            altscreen_guard.to_main().unwrap();
            RawScreen::disable_raw_mode().unwrap();
            print!("\x1b[?25h");
            std::io::stdout().flush().unwrap();
            if let Some(s) = stream.lock().unwrap().as_mut() {
                send_to_stream(s, &[MAGIC_NET_EXIT]);
            }
            return;
        }
        sleep(Duration::from_millis(1));
    }
}

/// Sends a new direction to server
pub fn send_direction(mut stream: &mut TcpStream, direction: u8) {
    let mut bytes: Vec<u8> = vec![MAGIC_NET_CHANGE_DIRECTION];
    bytes.push(direction);
    send_to_stream(&mut stream, &bytes);
}

/// Sends a message to server asking to toggle fast mode
pub fn toggle_fast_mode(mut stream: &mut TcpStream) {
    let bytes = vec![MAGIC_NET_TOGGLE_FAST];
    send_to_stream(&mut stream, &bytes);
}

/// Handles data sent by server and if the data is new game data, draws the new frame to user
pub fn handle_server_message(data: Vec<u8>, my_id: u16, world_size: (u16, u16)) -> Exit {
    // Messages starting with:
    //  - \x03 mean that I died
    //  - \x04 mean that it's the game data
    if data.len() == 1 && data[0] == MAGIC_NET_DEATH {
        // Exit
        return Exit::Death;
    } else if data[0] == MAGIC_NET_GAME_DATA {
        // Parse the data
        let mut i = 1; // next byte to read

        // First 2 bytes are the amount of snakes in total
        let snake_amount = u16::from_be_bytes([data[i], data[i + 1]]);
        i += 2;
        // A hashmap pointing snake ID to it's nickname, score and amount of kills
        let mut snakes: HashMap<u16, (String, u16, u16, bool)> = HashMap::new();
        // A hashmap mapping head positions to their owner-snakes IDs
        let mut head_positions: HashMap<(u16, u16), u16> = HashMap::new();
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
            let score = u16::from_be_bytes([data[i], data[i + 1]]);
            i += 2;
            let kills = u16::from_be_bytes([data[i], data[i + 1]]);
            i += 2;
            let head_pos = (
                u16::from_be_bytes([data[i], data[i + 1]]),
                u16::from_be_bytes([data[i + 2], data[i + 3]]),
            );
            head_positions.insert(head_pos, id);
            i += 4;
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
                    i8::from_be_bytes([data[i]]), // X pos of food relative to my head
                    i8::from_be_bytes([data[i + 1]]),
                ), // Y pos of food relative to my head
                u8::from_be_bytes([data[i + 2]]),
            ); // amount of food there
            i += 3;
        }

        // Snake parts
        let snake_parts_amount = u16::from_be_bytes([data[i], data[i + 1]]);
        i += 2;
        let mut snake_parts: HashMap<(i8, i8), u16> = HashMap::new();
        for _snake_part in 0..snake_parts_amount {
            snake_parts.insert(
                (
                    i8::from_be_bytes([data[i]]), // X pos of part relative to my head
                    i8::from_be_bytes([data[i + 1]]),
                ), // Y pos of part relative to my head
                u16::from_be_bytes([data[i + 2], data[i + 3]]),
            ); // snake ID
            i += 4;
        }

        let my_position = (
            u16::from_be_bytes([data[i], data[i + 1]]),
            u16::from_be_bytes([data[i + 2], data[i + 3]]),
        );

        // // OK, all the data is read and parsed - time to draw the frame
        draw(
            my_id,
            snakes,
            foods,
            snake_parts,
            my_position,
            world_size,
            head_positions,
        );
    }
    Exit::Continue
}

/// Draws the new frame
pub fn draw(
    my_id: u16,
    snakes_info: HashMap<u16, (String, u16, u16, bool)>,
    foods: HashMap<(i8, i8), u8>,
    snake_parts: HashMap<(i8, i8), u16>,
    my_pos: (u16, u16),
    world_size: (u16, u16),
    head_positions: HashMap<(u16, u16), u16>,
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
    for y in height.clone() {
        for x in width.clone() {
            if x < -24 || x > 24 || y < -14 || y > 14 {
                to_print += "  ";
                continue;
            }
            if snake_parts.contains_key(&(x, y)) {
                // Get the color
                to_print += SNAKE_COLORS[(snake_parts[&(x, y)] % 9) as usize];
                match (
                    snakes_info[&snake_parts[&(x, y)]].3,
                    head_positions.contains_key(&(
                        ((x as i32 + my_pos.0 as i32 + world_size.0 as i32) % world_size.0 as i32)
                            as u16,
                        ((y as i32 + my_pos.1 as i32 + world_size.1 as i32) % world_size.1 as i32)
                            as u16,
                    )),
                ) {
                    (_, true) => {
                        to_print += "φφ"; // Eyes/Head
                    }
                    (false, _) => {
                        to_print += "[]"; // snake moving at normal speed
                    }
                    (true, _) => {
                        to_print += "╬╬"; // snake in fast mode
                    }
                };

                to_print += "\x1b[0m"; // reset colors
            } else {
                // Check for food
                for i in 0..2 {
                    let fields = (
                        foods.get(&(2 * x + if i == 1 { 1 } else { 0 }, 2 * y)),
                        foods.get(&(2 * x + if i == 1 { 1 } else { 0 }, 2 * y + 1)),
                    );
                    match fields {
                        (None, None) => {
                            to_print += " ";
                        }
                        (Some(amount), None) => {
                            to_print += foodcolor(*amount, false);
                            to_print += "▀";
                        }
                        (None, Some(amount)) => {
                            to_print += foodcolor(*amount, false);
                            to_print += "▄";
                        }
                        (Some(amount0), Some(amount1)) => {
                            to_print += foodcolor(*amount0, false);
                            to_print += foodcolor(*amount1, true);
                            to_print += "▀";
                        }
                    }
                    to_print += "\x1b[0m"; // reset colors
                }
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
            (1_000f64 * my_pos.0 as f64 / world_size.0 as f64),
            (1_000f64 * my_pos.1 as f64 / world_size.1 as f64)
        )
    } else {
        "".to_string()
    };
    let snakes_count_text = format!("{} snakes", snakes_info.len());
    to_print += SNAKE_COLORS[(my_id % 9) as usize]; // colors
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

    // Print nicknames of snakes
    for head_pos in head_positions.keys() {
        if head_positions[head_pos] == my_id {
            continue;
        }
        let nick = &snakes_info[&head_positions[head_pos]].0;
        let leftpadding = " ".repeat(((10f32 - nick.len() as f32) / 2f32).floor() as usize);
        let rightpadding = " ".repeat(((10f32 - nick.len() as f32) / 2f32).ceil() as usize);
        let finalnickname = leftpadding + nick + &rightpadding;
        let nickname_bytes = finalnickname.as_bytes();

        for i in 0..10 {
            // Theoretically it's possible to display the same nickname on 4 different locations on the screen
            for l in 0..4 {
                // Check if the field is in frame
                let pos_x = if l / 2 == 1 {
                    2 * (head_pos.0 as i32
                        - my_pos.0 as i32
                        - world_size.0 as i32
                        - width.start as i32)
                        + i as i32
                        - 3
                } else {
                    2 * (head_pos.0 as i32 - my_pos.0 as i32 - width.start as i32) + i as i32 - 3
                };
                let pos_y = if l % 2 == 0 {
                    (head_pos.1 as i32
                        - my_pos.1 as i32
                        - world_size.1 as i32
                        - height.start as i32)
                        + 2
                } else {
                    (head_pos.1 as i32 - my_pos.1 as i32 - height.start as i32) + 2
                };
                if pos_x >= 0
                    && pos_x <= real_terminal_size.0 as i32
                    && pos_y >= 0
                    && pos_y < real_terminal_size.1 as i32
                {
                    if nickname_bytes[i] == 0x20 {
                        continue;
                    } // Skip spaces
                      // Move to the required position and print the text
                    to_print += &format!(
                        "\x1b[{line};{column}H{text}",
                        line = pos_y,
                        column = pos_x,
                        text = std::str::from_utf8(&[nickname_bytes[i]]).unwrap()
                    );
                }
            }
        }
    }

    // If needed, print leaderboard
    let show_board = *SHOW_LEADERBOARD.lock().unwrap();
    if let ShowLeaderboard::ByScore | ShowLeaderboard::ByKills = show_board {
        let column = real_terminal_size.0 - 20;
        let (by_what, board) = match show_board {
            ShowLeaderboard::ByScore => ("Score", get_top_by_score(&snakes_info)),
            ShowLeaderboard::ByKills => ("Kills", get_top_by_kills(&snakes_info)),
            _ => ("", Vec::new()),
        };
        let mut board = board.into_iter();
        to_print += &format!(
            "\x1b[1;{column}H\x1b[100;4;1m      By {what}       \x1b[0m",
            column = column,
            what = by_what
        );
        for ln in 2..11 {
            let player = board.next();
            let (nickname, score) = match player {
                Some((n, s)) => (n, format!("({})", s)),
                None => ("".to_string(), "".to_string()),
            };
            to_print += &format!("\x1b[{line};{column}H\x1b[100;1m{place}. \x1b[0m\x1b[100m{nickname}{padding}{score}\x1b[0m",
                line=ln,
                column=column,
                place=ln-1,
                nickname=nickname,
                score=score,
                padding=" ".repeat(18 - nickname.len() - score.len()),
                );
        }
    }

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

pub fn get_top_by_score(
    snakes_data: &HashMap<u16, (String, u16, u16, bool)>,
) -> Vec<(String, u16)> {
    let mut scores: Vec<(String, u16)> = snakes_data.iter()
        .map(|(_id, (nickname, score, _kills, _fast_mode))| (nickname.clone(), *score)).collect();
    scores.sort_unstable_by_key(|(nickname, score)| (*score, nickname.clone()));
    scores.reverse();
    scores
}

pub fn get_top_by_kills(
    snakes_data: &HashMap<u16, (String, u16, u16, bool)>,
) -> Vec<(String, u16)> {
    let mut scores: Vec<(String, u16)> = snakes_data.iter()
        .map(|(_id, (nickname, _score, kills, _fast_mode))| (nickname.clone(), *kills)).collect();
    scores.sort_unstable_by_key(|(nickname, kills)| (*kills, nickname.clone()));
    scores.reverse();
    scores
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

pub fn foodcolor(amount: u8, bg: bool) -> &'static str {
    let t = if amount < 2 {
        FOOD_COLORS[0]
    } else if amount < 5 {
        FOOD_COLORS[1]
    } else if amount < 11 {
        FOOD_COLORS[2]
    } else {
        FOOD_COLORS[3]
    };

    if bg {
        t.1
    } else {
        t.0
    }
}
