#![feature(vec_remove_item)]

mod shared;
mod server;
mod client;

use crossterm_input::{input, AsyncReader, InputEvent, KeyEvent, RawScreen};
use rand::prelude::*;
use std::collections::{HashSet, VecDeque, HashMap};
use std::thread::sleep;
use std::time::Duration;
use std::io::prelude::*;
use std::net::{TcpListener, TcpStream, Shutdown};
use std::convert::TryInto;

use shared::*;

fn main() {
	// First ask the player if they want to host the server or to connect to another server
	println!("Would you like to host the server, or connect to a server?");
	print!("If you would like to connect to a server, type the IP,\nand if you want to host a server, just press enter: ");
	std::io::stdout().flush().unwrap();
	let mut input = String::new();

	std::io::stdin().read_line(&mut input).unwrap();
	
	input = input.trim().to_owned(); // remove the \n from the end, and maybe other whitespace

	if input.is_empty() {
		// Host the server
		print!("How many other players are you expecting? (0-5) ");
		std::io::stdout().flush().unwrap();

		// Put this in a loop, to just get that info from the host
		let mut player_count: u8;
		loop {
			let mut raw_player_count = String::new();
			std::io::stdin().read_line(&mut raw_player_count).unwrap();
			player_count = match raw_player_count.trim().parse() { Err(_) => { println!("\x1b[1mSorry but I can't parse this number. Please rethink your choice\x1b[0m"); continue; }, Ok(i) => i};
			if player_count > 5 {
				println!("\x1b[1mBro I said 5 is maximum!\x1b[0m");
				continue;
			}
			if player_count == 0 {
				println!("\x1b[1m( •_•) Playing with yourself I see?\x1b[0m");
			}
			break;
		}

		// Ask for the game speed
		print!("What do you want the game speed to be? (1-250, Default=5) ");
		std::io::stdout().flush().unwrap();

		// Again, put in a loop so the host can have a normal conversation with me
		let mut game_speed: u8;
		loop {
			let mut raw_game_speed = String::new();
			std::io::stdin().read_line(&mut raw_game_speed).unwrap();
			game_speed = if raw_game_speed.trim().len()==0 { 5 } else { match raw_game_speed.trim().parse() {
				Err(_) => {println!("\x1b[1mcan you just enter a normal number\x1b[0m"); continue; },
				Ok(i) => i
			} };
			if game_speed < 1 {
				println!("\x1b[1mBro... I'm sure you're not that bad at this game. Try higher speeds\x1b[0m");
				continue;
			}
			if game_speed > 250 {
				println!("\x1b[1mThis would be too fast for you bro\x1b[0m");
				continue;
			}
			break;
		}
		// Ok good, we finally have all the data

		println!("Hosting server...");
		let mut server = server::Server::new(game_speed);
		server.start_server(player_count);
	} else {
		// Try to connect to the server
		let mut client = match client::Client::connect(format!("{}:50403", input)) {
			Err(e) => {
				println!("Failed to connect to the server: {}", e);
				return;
			},
			Ok(client) => client
		};
		println!("Connected successfully!");
		println!("Waiting for others to join...");
		if let Err(()) = client.wait_for_start() {
			println!("\x1b[1mThe connection with the server was dropped.\x1b[0m");
			return;
		}
		println!("Starting game...");
		client.start_game();
	}

}
