use crate::*;

pub struct Server {
	clients: Vec<TcpStream>,
	pub global_termsize: (u16, u16),
	snakes: Vec<Snake>,
	foods: Vec<(u16, u16)>,
	speed: u8, // How many times per second the snake moves and the screen is redrawed
	input: AsyncReader,
    ended: bool,
    pub scores: Vec<u16>
}

impl Server {
	pub fn new(speed: u8) -> Server {
		let input = input();
        input
            .disable_mouse_mode()
            .expect("can't disable mouse mode");

		Server {
			clients: Vec::new(),
			global_termsize: get_terminal_size(),
			snakes: Vec::new(),
			foods: Vec::new(),
			speed: speed,
			input: input.read_async(),
			ended: false,
			scores: Vec::new()
		}
	}
	/// Start the server
	pub fn start_server(self: &mut Server, player_count: u8) {
		// First accept the connections
		self.accept_connections(player_count);
		// Ok now start the game
		// Send everyone the termsize
		let termsize = self.get_termsize();
		for mut stream in &mut self.clients {
			send_to_stream(
				&mut stream,
				&( ((termsize.0 as u32) << 16) | termsize.1 as u32 ).to_be_bytes()
			)
		}
		// Start the game

		// Get the Server struct ready for game

		// Generate player data
		for id in 0..self.clients.len()+1 {
			// Generate the snakes
			self.snakes.push(Snake {
				direction: if id%2==0 { Direction::Right } else { Direction::Left },
				parts: [(0u16, (id*2) as u16), (1u16, (id*2) as u16), (2u16, (id*2) as u16)].iter().cloned().collect(),
				ordered_parts: [(0u16, (id*2) as u16), (1u16, (id*2) as u16), (2u16, (id*2) as u16)].iter().cloned().collect(),
				dead: false
			});
			// Make each second snake different direction
			if id%2==1 {
				self.snakes[id as usize].ordered_parts = self.snakes[id as usize].ordered_parts.iter().rev().cloned().collect();
			}

			// Generate foods, 1 food for each snake
			let food_pos = self.generate_food_pos();
			self.foods.push(food_pos);

			// Add scores
			self.scores.push(0);
		}
		// Send everyone the game data, which will also act as a signal to start the game
		self.send_game_data();
		// Start the game
		self.start_game();

	} 
	/// Start listening for connections
	fn accept_connections(self: &mut Server, player_count: u8) {
		let listener = TcpListener::bind("0.0.0.0:50403").unwrap();

		// Collection of the terminal sizes of the clients
		let mut termsizes: Vec<(u16, u16)> = Vec::new();

	    // accept connections
	    loop {
	    	// If all the players who we we're waiting for connected, stop accepting new connections
	        if self.clients.len() as u8 == player_count {
	        	break;
	        }
	        // Accept a new connection
	    	if let Ok((mut stream, addr)) = listener.accept() {

	    		// wait 1 second for terminal size, if the client doesn't send it, drop the connection
	    		stream.set_read_timeout(Some(Duration::new(1, 0))).expect("set_read_timeout call failed");

	    		let terminal_size = match read_from_stream(&mut stream) {
	    			Ok(bytes) => bytes,
	    			Err(_) => {
	    				stream.shutdown(Shutdown::Both).expect("failed to drop connection");
    					continue;
	    			}
	    		};

	    		// The terminal size was received
	    		let terminal_size: (u16, u16) = (
	    			u16::from_be_bytes( [terminal_size[0], terminal_size[1]] ),
	    			u16::from_be_bytes( [terminal_size[2], terminal_size[3]] )
	    		);
	    		// Make sure it's not too small
	    		if terminal_size.0 < 30 || terminal_size.1 < 12 {
	    			println!("\x1b[1mSome dude ({}) connected with terminal size \x1b[4mso small\x1b[24m, that it would make the game literally unplayable for everyone in the party.\r\nI will kick him now, tell him that he can try to connect again after making his terminal bigger. (minimum size: 60x24)\x1b[0m", addr);
	    			stream.shutdown(Shutdown::Both).expect("failed to drop connection");
	    			continue;
	    		}
	    		termsizes.push(terminal_size);

		    	println!("{} connected, termsize: {:?}", addr, terminal_size);
		        self.clients.push(stream);
	    	}
	    }
	    // Now that we have all the clients connected, we can figure out the global terminal size
	    for termsize in termsizes {
	    	// Width (columns)
	    	if termsize.0 < self.get_termsize().0 {
	    		self.global_termsize.0 = termsize.0;
	    	}
	    	// Height (lines)
	    	if termsize.1 < self.get_termsize().1 {
	    		self.global_termsize.1 = termsize.1;
	    	}
	    }
	}

	fn generate_food_pos(self: &Server) -> (u16, u16) {

		// Merge all the snakes parts into a single HashSet so we could check if there is snake
		// on a field faster
		let mut all_snakes_parts = HashSet::<(u16, u16)>::new();
		for snake in &self.snakes {
			all_snakes_parts.extend(snake.parts.clone());
		}

        loop {
            let food_pos: (u16, u16) = (
                thread_rng().gen_range(0, self.get_termsize().0) as u16,
                thread_rng().gen_range(0, self.get_termsize().1 - 1) as u16,
            );
            // If there's snake on the food, generate another value

            if all_snakes_parts.contains(&food_pos) {
                continue;
            }
            return food_pos;
        }
    }

    fn move_snake(self: &mut Server, snake_id: usize, all_snakes_parts: &HashMap<(u16, u16), usize>) -> Move {
        // Calculate the new head position

        let mut new_head_pos = *self.snakes[snake_id].ordered_parts.back().unwrap();

        let (dx, dy) = match self.snakes[snake_id].direction {
            Direction::Left => (-1, 0),
            Direction::Right => (1, 0),
            Direction::Down => (0, 1),
            Direction::Up => (0, -1),
        };

        let width = self.get_termsize().0 as i16;
        let height = self.get_termsize().1 as i16;
        new_head_pos.0 = (((new_head_pos.0 as i16 + dx) + width) % width) as u16;
        new_head_pos.1 = (((new_head_pos.1 as i16 + dy) + height) % height) as u16;

        // If the head is on food, eat it
        if self.foods.contains(self.snakes[snake_id].ordered_parts.back().unwrap()) {
            self.scores[snake_id] += 1;
            self.foods.remove_item(self.snakes[snake_id].ordered_parts.back().unwrap());
            self.foods.push(self.generate_food_pos());
        } else {
            // Only remove the last part if no food was eaten
            let last_part_pos = self.snakes[snake_id].ordered_parts.pop_front().unwrap();
            // Don't remove from the hashset if there are more parts on that position
            if !self.snakes[snake_id].ordered_parts.contains(&last_part_pos) {
            	self.snakes[snake_id].parts.remove(&last_part_pos);
            }
        }

        // See if the snake crashed
        
        if all_snakes_parts.contains_key(&new_head_pos) {
        	// Make sure it's a foreign snake (not myself)
        	if all_snakes_parts[&new_head_pos] != snake_id {
	            return Move::Crash;
        	}
        }

        self.snakes[snake_id].ordered_parts.push_back(new_head_pos);
        self.snakes[snake_id].parts.insert(new_head_pos);
        Move::Ok
    }

    fn send_game_data(self: &mut Server) {
    	for (mut id, mut stream) in self.clients.iter_mut().enumerate() {
    		id += 1; // because the server always has id 0
    		let mut bytes = Vec::<u8>::new();

    		// The player's ID
    		bytes.push(id as u8);

    		// Snakes
    		bytes.push(self.snakes.len() as u8); // amount of snakes in total

    		for snake in &self.snakes {
    			bytes.push(snake.direction as u8); // direction
    			bytes.push(snake.dead as u8); // is it dead
    			bytes.push(snake.ordered_parts.len() as u8); // the amount of parts
    			for part in &snake.ordered_parts { // all the parts
    				bytes.extend_from_slice(&part.0.to_be_bytes()[..]); // x pos of a part
    				bytes.extend_from_slice(&part.1.to_be_bytes()[..]); // y pos of a part
    			}
    		}
    		// Foods
    		for foodpos in &self.foods { // Position of each food
    			bytes.extend_from_slice(&foodpos.0.to_be_bytes()[..]); // x pos of a food
    			bytes.extend_from_slice(&foodpos.1.to_be_bytes()[..]); // y pos of a food
    		}

    		// Game speed
    		bytes.push(self.speed);

    		// Scores
    		for score in &self.scores {
    			bytes.extend_from_slice(&score.to_be_bytes()[..]); // score
    		}

    		// send it
    		send_to_stream(&mut stream, &bytes);
    	}
    }
}

impl Game for Server {
	fn do_networking(self: &mut Self) {
		// If there's only 1 alive snake left, it's time to end the game
		let mut alive_snakes = 0;
		for snake in &self.snakes {
			if !snake.dead {
				alive_snakes += 1;
			}
		}
		if alive_snakes <= 1 {
			self.ended = true;
		}

		// Read all the data the players sent me, and then send them the new game data

		let mut clients_to_remove = Vec::<usize>::new();

		for (id, mut stream) in self.clients.iter_mut().enumerate() {
			if self.snakes[id+1].dead { continue; }
			// Every frame all the clients send the server their snake's facing direction
			let direction = match read_from_stream(&mut stream) {
				Err(_) => {
					// The connection with that player was lost, so remove him from the game
					// Always add 1 to the id, because the server (me) has ID 0
					self.scores.remove(id+1);
					self.foods.remove(id+1);
					self.snakes.remove(id+1);
					clients_to_remove.push(id);
					continue;
				},
				Ok(direction) => direction
			};
			self.snakes[id+1].direction = Direction::from_byte(direction[0]);
		}
		// Remove the disconnected clients streams
		for id in clients_to_remove {
			self.clients.remove(id);
		}

		// Ok now we have all the directions, now we shall move the snakes

		// Merge all the snakes parts into a single HashMap<pos->snake_id> so we could check if there is snake
		// on a field faster, and to which snake the part belongs
		let mut all_snakes_parts = HashMap::<(u16, u16), usize>::new();
		for (id, snake) in self.get_snakes().iter().enumerate() {
			if snake.dead { continue; }
			let temp_snake_hashmap: HashMap<(u16,u16), usize> = snake.parts.iter().map( |pos| (*pos, id as usize) ).collect();
			all_snakes_parts.extend( temp_snake_hashmap );
		}

		for snake_id in 0..self.snakes.len() {
			if self.snakes[snake_id].dead { continue; }
			if let Move::Crash = self.move_snake(snake_id, &all_snakes_parts) {
				// some dude crashed
				self.snakes[snake_id].ordered_parts = VecDeque::new();
				self.snakes[snake_id].parts = HashSet::new();
				self.snakes[snake_id].dead = true;
			}
		}

		// Now send the new data to the players
		self.send_game_data();
	}

	fn get_game_speed(self: &Self) -> u8 {
		self.speed
	}
	fn get_ended(self: &mut server::Server) -> &mut bool {
		&mut self.ended
	}
	fn get_termsize(self: &Self) -> (u16, u16) {
		self.global_termsize
	}
	fn get_snakes(self: &mut server::Server) -> &mut Vec<Snake> {
		&mut self.snakes
	}
	fn get_foods(self: &Self) -> &Vec<(u16, u16)> {
		&self.foods
	}
	fn get_score(self: &Self) -> u16 {
		self.scores[0]
	}
	fn get_input(self: &mut Self) -> &mut AsyncReader {
		&mut self.input
	}
	fn get_my_id(self: &Self) -> usize {
		0
	}
	fn dead(self: &Self) -> bool {
		self.snakes[0].dead
	}
}