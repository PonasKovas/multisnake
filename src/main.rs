#![feature(option_unwrap_none)]

mod client;
mod server;

use dns_lookup::lookup_host;
use std::num::NonZeroU8;
use std::str::FromStr;
use structopt::StructOpt;

struct WorldSize(u16, u16);

impl FromStr for WorldSize {
    type Err = &'static str;

    fn from_str(data: &str) -> Result<Self, Self::Err> {
        let mut split = data.split('x');

        let width = split
            .next()
            .and_then(|num| num.parse::<u16>().ok())
            .ok_or("Failed to parse width")?;
        let height = split
            .next()
            .and_then(|num| num.parse::<u16>().ok())
            .ok_or("Failed to parse height")?;

        if split.next().is_some() {
            return Err("Extra data");
        }

        if width < 20 || height < 20 {
            return Err("Width and height have to be at least 20");
        }

        Ok(WorldSize(width, height))
    }
}

struct Nickname(String);

impl FromStr for Nickname {
    type Err = &'static str;

    fn from_str(data: &str) -> Result<Self, Self::Err> {
        if data.is_empty() {
            return Err("Empty nicknames are not allowed");
        }

        if data.len() > 10 {
            return Err("Nickname is too long");
        }

        Ok(Nickname(data.to_owned()))
    }
}

#[derive(StructOpt)]
enum Args {
    Server {
        /// Amount of bots in the game (0-65535)
        #[structopt(default_value = "0", short = "b")]
        bots: u16,

        /// Rate of how much food should be constantly in the world in relation to the world size, bigger number = less food
        #[structopt(default_value = "10", short = "f")]
        food_rate: NonZeroU8,

        /// Ticks per second (1-255)
        #[structopt(default_value = "10", short = "s")]
        game_speed: NonZeroU8,

        /// Player limit for the server (0-65535)
        #[structopt(default_value = "50", short = "m")]
        max_players: u16,

        /// The size of the world (20-65535)
        #[structopt(default_value = "200x200", short = "w")]
        world_size: WorldSize,

        /// Initializes server on this port
        #[structopt(default_value = "50403", short = "p")]
        port: u16,
    },
    Client {
        /// Your nickname (1-10 characters)
        nickname: Nickname,

        /// IP address of the server
        ip: String,

        /// Port of the server
        #[structopt(default_value = "50403")]
        port: u16,
    },
}

fn main() {
    let args = Args::from_args();

    match args {
        Args::Server {
            bots,
            food_rate,
            game_speed,
            max_players,
            world_size: WorldSize(width, height),
            port,
        } => {
            server::Server::start(
                max_players,
                game_speed.into(),
                port,
                (width, height),
                food_rate.into(),
                bots,
            );
        },
        Args::Client {
            nickname: Nickname(nickname),
            mut ip,
            port,
        } => {
            // Resolve the address of the entered hostname
            if ip != "localhost" {
                match lookup_host(&ip).expect("could not resolve the IP").get(0) {
                    Some(addr) => {
                        ip = addr.to_string();
                    }
                    None => {
                        println!("Could not resolve the IP of host {}", ip);
                        return;
                    }
                }
            }

            // Start the client
            client::start(ip, port, nickname);
        }
    }
}
