use crate::*;

pub struct Client {
	stream: TcpStream,
	termsize: (u16, u16),
	snakes: Vec<Snake>,
	foods: Vec<(u16, u16)>,
	speed: u8, // How many times per second the snake moves and the screen is redrawed
	input: AsyncReader,
    ended: bool,
    pub scores: Vec<u16>,
    my_id: usize
}

impl Client {
	pub fn connect(addr: String) -> std::io::Result<Client> {
		// Try connecting
		match TcpStream::connect(addr) {
			Ok(mut stream) => {
				// Connected OK
				// Send the server my terminal size, so it can find the optimal size for all the players
				let terminal_size = get_terminal_size();
				let bytes = ( ((terminal_size.0 as u32) << 16) | terminal_size.1 as u32 ).to_be_bytes();
				send_to_stream(&mut stream, &bytes);

				// return the client struct

				let input = input();
		        input
		            .disable_mouse_mode()
		            .expect("can't disable mouse mode");

				Ok( Client {
					stream: stream,
					termsize: get_terminal_size(),
					snakes: Vec::new(),
					foods: Vec::new(),
					speed: 0,
					input: input.read_async(),
					ended: false,
					scores: Vec::new(),
					my_id: 0
				} )
			},
			Err(e) => {
				Err(e)
			}
		}
	}
	pub fn wait_for_start(self: &mut Client) -> Result<(), ()> {
		// Wait for the server to announce the termsize
		let termsize = read_from_stream(&mut self.stream)?;
		// Save it
		self.termsize = (
			u16::from_be_bytes( [termsize[0], termsize[1]] ),
			u16::from_be_bytes( [termsize[2], termsize[3]] )
		);

		// Receive game data
		let game_data = read_from_stream(&mut self.stream).unwrap();

		// Parse and save it
		self.parse_game_data(game_data);

		Ok(())
	}
	fn parse_game_data(self: &mut Self, game_data: Vec<u8>) {
		let mut i = 0; // next byte position

		// ID
		let my_id = game_data[i]; i+=1;

		

		// Snakes
	
		let mut snakes = Vec::new();

		for _snake in 0..game_data[i] {
			let direction = Direction::from_byte( game_data[i+1] ); i+=1;
			let dead = game_data[i+1]==1; i+=1;
			let mut ordered_parts = VecDeque::new();
			for _part in 0..game_data[i+1] {
				ordered_parts.push_back((
					u16::from_be_bytes([game_data[i+2],game_data[i+3]]),
					u16::from_be_bytes([game_data[i+4],game_data[i+5]])
				));
				i+=4;
			}
			i+=1;
			let unordered_parts: HashSet<(u16, u16)> = ordered_parts.iter().cloned().collect();
			snakes.push(
				Snake {
					direction: direction,
					ordered_parts: ordered_parts,
					parts: unordered_parts,
					dead: dead
				}
			);
		}
		i+=1;

		// Foods
		let mut foods = Vec::new();
		for _food in 0..game_data[1] { // amount of snakes = amount of foods
			foods.push((
				u16::from_be_bytes([game_data[i],game_data[i+1]]),
				u16::from_be_bytes([game_data[i+2],game_data[i+3]])
			));
			i+=4;
		}
		// Game speed
		let speed = game_data[i]; i+=1;

		// Scores
		let mut scores = Vec::new();
		for _score in 0..game_data[1] { // amount of snakes = amount of scores
			scores.push(
				u16::from_be_bytes((&game_data[i..i+2]).try_into().unwrap())
			);
			i+=2;
		}

		// OK, now save the data
		self.snakes = snakes;
		self.foods = foods;
		self.speed = speed;
		self.scores = scores;
		self.my_id = my_id as usize;
	}
}

impl Game for Client {
	fn do_networking(self: &mut Self) {
		// Send the server my snake's direction
		if !self.dead() {
			send_to_stream(&mut self.stream, &[(self.snakes[self.my_id].direction as u8)]);
		}

		// Receive game data
		let game_data = match read_from_stream(&mut self.stream) {
			Ok(game_data) => game_data,
			Err(()) => {
				self.ended = true;
				return;
			}
		};

		// Parse and save it
		self.parse_game_data(game_data);
	}

	fn get_game_speed(self: &Self) -> u8 {
		self.speed
	}
	fn get_ended(self: &mut Self) -> &mut bool {
		&mut self.ended
	}
	fn get_termsize(self: &Self) -> (u16, u16) {
		self.termsize
	}
	fn get_snakes(self: &mut Self) -> &mut Vec<Snake> {
		&mut self.snakes
	}
	fn get_foods(self: &Self) -> &Vec<(u16, u16)> {
		&self.foods
	}
	fn get_score(self: &Self) -> u16 {
		self.scores[self.my_id]
	}
	fn get_input(self: &mut Self) -> &mut AsyncReader {
		&mut self.input
	}
	fn get_my_id(self: &Self) -> usize {
		self.my_id
	}
	fn dead(self: &Self) -> bool{
		self.snakes[self.my_id].dead
	}
}