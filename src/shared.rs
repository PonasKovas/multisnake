use crate::*;

// A trait that the clients' and the hosts' games have
pub trait Game {
	fn start_game(self: &mut Self) {
		// Put the terminal into raw mode
		RawScreen::into_raw_mode()
			.expect("Failed to put terminal into raw mode.")
			.disable_drop();

		loop {
			self.handle_input();
			self.do_networking();
			self.draw();
			if *self.get_ended() {
				RawScreen::disable_raw_mode().expect("Failed to put terminal into normal mode.");
				print!("\r\n");
				return;
			}
			sleep(Duration::from_millis((1000f64 / self.get_game_speed() as f64) as u64));
		}
	}
	fn draw(self: &mut Self) {
		clear_terminal();

		// Merge all the snakes parts into a single HashMap<pos->snake_id> so we could check if there is snake
		// on a pixel faster, and to which snake the part belongs
		let mut all_snakes_parts = HashMap::<(u16, u16), usize>::new();
		for (id, snake) in self.get_snakes().iter().enumerate() {
			if snake.dead { continue; }
			let temp_snake_hashmap: HashMap<(u16,u16), usize> = snake.parts.iter().map( |pos| (*pos, id as usize) ).collect();
			all_snakes_parts.extend( temp_snake_hashmap );
		}

		// Draw the frame
		let mut frame = Vec::<u8>::new();

		for y in 0..self.get_termsize().1 {
			for x in 0..self.get_termsize().0 {
				// See if there's snake on this position
				if all_snakes_parts.contains_key(&(x, y)) {
					frame.extend_from_slice( SNAKE_COLORS[all_snakes_parts[&(x, y)]] );
					frame.extend_from_slice( b"  \x1b[0m"); // A colored square, the color depends on which snake is it.
					continue;
				}

				// If there's food in this position
				if self.get_foods().contains(&(x, y)) {
					frame.extend_from_slice(b"\x1b[42m  \x1b[0m"); // A green square
					continue;
				}

				frame.extend_from_slice(b"  ");
			}

			// Add the right side outline
			if self.get_termsize().0 < get_terminal_size().0 {
				frame.extend_from_slice( b"\x1b[47m" ); // \e[47m - ascii bg color
				frame.extend_from_slice( b"  " );
				frame.extend_from_slice( b"\x1b[49m" ); // \e[49m - ascii reset bg color
			}
			frame.extend_from_slice(b"\r\n");
		}

		// Add the status line at the bottom
		let status_text = format!("Score: {}", self.get_score());
		frame.extend_from_slice( SNAKE_COLORS[self.get_my_id()] );
		frame.extend_from_slice(b"\x1b[30m");
		frame.extend_from_slice(" ".repeat( (((self.get_termsize().0 * 2) as usize - status_text.len()) as f64 / 2f64).floor() as usize).as_bytes());
		frame.extend_from_slice(status_text.as_bytes());
		frame.extend_from_slice(" ".repeat( (((self.get_termsize().0 * 2) as usize - status_text.len()) as f64 / 2f64).ceil() as usize 
			+ if self.get_termsize().0 < get_terminal_size().0 { 2 } else { 0 }).as_bytes()); // Color the corner if needed
		frame.extend_from_slice(b"\x1b[0m\x1b[1D");

		// Print it to the terminal
		std::io::stdout().write_all(&frame[..]).unwrap();
		std::io::stdout().flush().unwrap();
	}
	fn handle_input(self: &mut Self) {
		let my_id = self.get_my_id();
        let mut new_direction = self.get_snakes()[my_id].direction;

        for event in &mut self.get_input() {
            match event {
                // ctrl-c or Q to quit the game
                InputEvent::Keyboard(KeyEvent::Ctrl('c'))
                | InputEvent::Keyboard(KeyEvent::Char('q')) => {
                    *self.get_ended() = true;
                    return;
                }
                // A or Left arrow - move left
                InputEvent::Keyboard(KeyEvent::Char('a')) | InputEvent::Keyboard(KeyEvent::Left) => {
                    new_direction = Direction::Left;
                }
                // S or Down arrow - move down
                InputEvent::Keyboard(KeyEvent::Char('s')) | InputEvent::Keyboard(KeyEvent::Down) => {
                    new_direction = Direction::Down;
                }
                // D or Right arrow - move right
                InputEvent::Keyboard(KeyEvent::Char('d')) | InputEvent::Keyboard(KeyEvent::Right) => {
                    new_direction = Direction::Right;
                }
                // W or Up arrow - move up
                InputEvent::Keyboard(KeyEvent::Char('w')) | InputEvent::Keyboard(KeyEvent::Up) => {
                    new_direction = Direction::Up;
                }
                _ => (),
            }
        }
        if self.get_snakes()[my_id].direction.is_opposite(new_direction) {
            new_direction = self.get_snakes()[my_id].direction;
        }
        self.get_snakes()[my_id].direction = new_direction;
    }

    fn do_networking(self: &mut Self);
	fn get_game_speed(self: &Self) -> u8;
	fn get_ended(self: &mut Self) -> &mut bool;
	fn get_termsize(self: &Self) -> (u16, u16);
	fn get_snakes(self: &mut Self) -> &mut Vec<Snake>;
	fn get_foods(self: &Self) -> &Vec<(u16, u16)>;
	fn get_score(self: &Self) -> u16;
	fn get_input(self: &mut Self) -> &mut AsyncReader;
	fn get_my_id(self: &Self) -> usize;
	fn dead(self: &Self) -> bool;
}

#[derive(Clone, Debug)]
pub struct Snake {
	pub direction: Direction,
	pub parts: HashSet<(u16, u16)>,
	pub ordered_parts: VecDeque<(u16, u16)>,
	pub dead: bool
}

#[derive(PartialEq, Copy, Clone, Debug)]
pub enum Direction {
	Left,
	Up,
	Right,
	Down,
}

pub enum Move {
	Ok,
	Crash,
}

impl Direction {
	pub fn is_opposite(self: Direction, other: Direction) -> bool {
		(self as i8 + 2) % 4 == other as i8
	}
	pub fn from_byte(byte: u8) -> Self {
		match byte {
			0 => Self::Left,
			1 => Self::Up,
			2 => Self::Right,
			_ => Self::Down
		}
	}
}

/// Returns terminal size
pub fn get_terminal_size() -> (u16, u16) {
	if let Some((mut w, h)) = term_size::dimensions() {
		// Width must be even.
		if w % 2 == 1 {
			w -= 1;
		}
		((w / 2) as u16, h as u16 - 1)
	} else {
		panic!("Can't get terminal size!");
	}
}

/// Clears the terminal screen, making it ready for drawing the next frame
pub fn clear_terminal() {
	print!("\x1b[2J\x1b[H");
}

pub fn send_to_stream(stream: &mut TcpStream, data: &[u8]) {
		let size: [u8; 2] = u16::to_be_bytes(data.len() as u16);
		let mut message: Vec<u8> = Vec::new();
		message.extend_from_slice(&size);
		message.extend_from_slice(data);

		stream.write_all(&message).unwrap();
}

pub fn read_from_stream(stream: &mut TcpStream) -> Result<Vec<u8>,()> {
		// Figure out the size of the incoming message
		let mut size = [0u8; 2];
		if let Err(_) = stream.read_exact(&mut size) {
			return Err(());
		}
		let size = u16::from_be_bytes(size);

		// Get the actual message
		let mut bytes = vec![0u8; size as usize];
		if let Err(_) = stream.read_exact(&mut bytes) {
			return Err(());
		}
		Ok(bytes)
}

const SNAKE_COLORS: [&[u8;6]; 6] = [
	b"\x1b[107m", // White
	b"\x1b[106m", // Cyan
	b"\x1b[105m", // Magenta
	b"\x1b[104m", // Blue
	b"\x1b[103m", // Yellow
	b"\x1b[102m", // Green
];