#![feature(option_unwrap_none)]

#[cfg(feature = "client")]
mod client;
#[cfg(feature = "server")]
mod server;

use clap::{App, Arg};
use std::io::{stdin, BufRead};

#[cfg(feature = "client")]
use dns_lookup::lookup_host;

fn main() {
    let max_players_help = format!(
        "[Server] Player limit for the server (Default {default}) ({min}-{max})",
        default = 50,
        min = u16::min_value(),
        max = u16::max_value()
    );
    let game_speed_help = format!(
        "[Server] Ticks per second (Default {default}) ({min}-{max})",
        default = 10,
        min = u8::min_value() + 1,
        max = u8::max_value()
    );
    let world_size_help = format!(
        "[Server] The size of the world (Default {default_width}x{default_height}) ({min}-{max})",
        default_width = 200,
        default_height = 200,
        min = u16::min_value() + 20,
        max = u16::max_value()
    );
    let food_rate_help = format!("[Server] Rate of how much food should be constantly in the world in relation to the world size, bigger number = less food (Default {default}) ({min}-{max})",
							   		default=10,
							   		min=u8::min_value()+1,
							   		max=u8::max_value());

    let mut matches = App::new("Multisnake")
        .version("0.1.0")
        .author("Ponas Kovas")
        .about("A multiplayer online snake game");
    #[cfg(feature = "server")]
    {
        matches = matches
            .arg(
                Arg::with_name("server")
                    .long("server")
                    .help("Starts as server instead of client")
                    .takes_value(false)
                    .conflicts_with("client"),
            )
            .arg(
                Arg::with_name("max_players")
                    .short("m")
                    .long("max-players")
                    .help(&max_players_help)
                    .takes_value(true)
                    .value_name("PLAYERS")
                    .conflicts_with("client"),
            )
            .arg(
                Arg::with_name("game_speed")
                    .short("s")
                    .long("game-speed")
                    .help(&game_speed_help)
                    .takes_value(true)
                    .value_name("SPEED")
                    .conflicts_with("client"),
            )
            .arg(
                Arg::with_name("world_size")
                    .short("w")
                    .long("world-size")
                    .help(&world_size_help)
                    .takes_value(true)
                    .value_name("WIDTHxHEIGHT")
                    .conflicts_with("client"),
            )
            .arg(
                Arg::with_name("food_rate")
                    .short("f")
                    .long("food-rate")
                    .help(&food_rate_help)
                    .takes_value(true)
                    .value_name("RATE")
                    .conflicts_with("client"),
            );
    }
    #[cfg(feature = "bots")]
    {
        matches = matches.arg(
            Arg::with_name("bots")
                .long("bots")
                .short("b")
                .help("[Server] adds bots to the game (Default 0)")
                .takes_value(true)
                .value_name("AMOUNT")
                .conflicts_with("client"),
        );
    }
    #[cfg(feature = "client")]
    {
        matches = matches
            .arg(
                Arg::with_name("client")
                    .long("client")
                    .help("Starts as client (default behaviour)")
                    .takes_value(false)
                    .conflicts_with("server"),
            )
            .arg(
                Arg::with_name("ip")
                    .short("i")
                    .long("ip")
                    .help("[Client] tries to connect to server on this IP")
                    .value_name("IP")
                    .takes_value(true)
                    .conflicts_with("server"),
            )
            .arg(
                Arg::with_name("nickname")
                    .short("n")
                    .long("nickname")
                    .help("[Client] your nickname (length 1-10)")
                    .value_name("NICKNAME")
                    .takes_value(true)
                    .conflicts_with("server"),
            );
    }
    #[cfg(any(feature = "client", feature = "server"))]
    {
        matches = matches.arg(Arg::with_name("port")
						   .short("p")
						   .long("port")
						   .value_name("PORT")
						   .help("initializes server on this port (default 50403) or tries to connect to server on this port if started in client mode")
						   .takes_value(true));
    }

    let matches = matches.get_matches();

    if matches.is_present("server") || cfg!(all(feature = "server", not(feature = "client"))) {
        // Server
        let port: u16 = match matches.value_of("port").unwrap_or("50403").parse() {
            Ok(n) => n,
            Err(_) => {
                println!("Failed to parse port!");
                return;
            }
        };
        let max_players: u16 = match matches
            .value_of("max_players")
            .unwrap_or("50")
            .parse()
        {
            Ok(n) => n,
            Err(_) => {
                println!("Failed to parse max players!");
                return;
            }
        };
        let game_speed: u8 = match matches
            .value_of("game_speed")
            .unwrap_or("10")
            .parse()
        {
            Ok(n) => {
                if n == 0 {
                    println!("Game speed can't be 0!");
                    return;
                }
                n
            }
            Err(_) => {
                println!("Failed to parse game speed!");
                return;
            }
        };
        let world_size: (u16, u16) = match matches.value_of("world_size") {
            Some(n) => {
                let p: Vec<&str> = n.split('x').collect();
                if p.len() != 2 {
                    println!("Failed to parse world size!");
                    return;
                }
                let w: u16 = match p[0].parse() {
                    Ok(i) => i,
                    Err(_) => {
                        println!("Failed to parse world size!");
                        return;
                    }
                };
                let h: u16 = match p[1].parse() {
                    Ok(i) => i,
                    Err(_) => {
                        println!("Failed to parse world size!");
                        return;
                    }
                };
                if w < 20 || h < 20 {
                    println!("World can't be that small!");
                    return;
                }
                (w, h)
            }
            None => (200, 200),
        };
        let food_rate: u8 = match matches
            .value_of("food_rate")
            .unwrap_or("10")
            .parse()
        {
            Ok(n) => {
                if n < 1 {
                    println!("Food rate can not be 0!");
                    return;
                }
                n
            }
            Err(_) => {
                println!("Failed to parse food rate!");
                return;
            }
        };

        let bots: u16 = match matches.value_of("bots").unwrap_or("0").parse() {
            Ok(amount) => amount,
            Err(_) => {
                println!("Couldn't parse the amount of bots");
                return;
            }
        };

        server::Server::start(max_players, game_speed, port, world_size, food_rate, bots);
    } else if cfg!(feature = "client") {
        // Client
        if let Some((w, h)) = term_size::dimensions() {
            if w < 98 || h < 30 {
                println!("Your terminal size is only {}x{}! For better game experience I really recommend having the terminal at least 98x30", w, h);
            }
        }
        let mut ip: String = match matches.value_of("ip") {
            Some(ip) => ip.to_string(),
            None => {
                println!("Please enter the IP of the server you want to connect to:");
                stdin().lock().lines().next().unwrap().unwrap()
            }
        };
        let port: u16 = match matches.value_of("port") {
            Some(n) => match n.parse() {
                Ok(p) => p,
                Err(_) => {
                    println!("Can't parse port!");
                    return;
                }
            },
            None => {
                println!("Please enter the PORT of the server you want to connect to (press ENTER for default 50403):");
                let input = stdin().lock().lines().next().unwrap().unwrap();
                if input.is_empty() {
                    50403
                } else {
                    match input.parse() {
                        Ok(a) => a,
                        Err(_) => {
                            println!("Failed to parse PORT!");
                            return;
                        }
                    }
                }
            }
        };
        let nickname: String = match matches.value_of("nickname") {
            Some(nickname) => nickname.to_string(),
            None => {
                println!("Choose a nickname:");
                stdin().lock().lines().next().unwrap().unwrap()
            }
        };
        if nickname.is_empty() || nickname.len() > 10 {
            println!("Nickname too short/too long. Allowed length 1-10");
            return;
        }

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
