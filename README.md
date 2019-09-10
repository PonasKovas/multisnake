# multisnake
A multiplayer snake game based on https://github.com/PonasKovas/snake

## Usage

```
A multiplayer online snake game

USAGE:
    multisnake [FLAGS] [OPTIONS]

FLAGS:
        --client     Starts as client (default behaviour)
    -h, --help       Prints help information
        --server     Starts as server instead of client
    -V, --version    Prints version information

OPTIONS:
    -b, --bots <AMOUNT>                [Server] adds bots to the game (Default 0)
    -f, --food-rate <RATE>             [Server] Rate of how much food should be constantly in the world in relation to
                                       the world size, bigger number = less food (Default 10) (2-255)
    -s, --game-speed <SPEED>           [Server] Ticks per second (Default 10) (1-255)
    -i, --ip <IP>                      [Client] tries to connect to server on this IP
    -m, --max-players <PLAYERS>        [Server] Player limit for the server (Default 50) (0-65535)
    -n, --nickname <NICKNAME>          [Client] your nickname (length 1-10)
    -p, --port <PORT>                  initializes server on this port (default 50403) or tries to connect to server on
                                       this port if started in client mode
    -w, --world-size <WIDTHxHEIGHT>    [Server] The size of the world (Default 200x200) (20-65535)
```

## Controls

<pre>
   [W]            [↑]   
[A][S][D]  or  [←][↓][→] . To toggle fast-mode, press [SPACE]
</pre>

## Game screenshots

![](https://i.imgur.com/b7hMPeW.png)
![](https://i.imgur.com/tQLPvbV.png)
![](https://i.imgur.com/k41bCCU.png)
![](https://i.imgur.com/UEwElc5.png)
