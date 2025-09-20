use std::{
    io::{Read, stdin},
    path::PathBuf,
};

use chess_gif_rs::{render_game, render_position};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// creates a gif from a pgn string
    Game {
        /// the game pgn (reads from stdin if not provided)
        pgn: Option<String>,
        /// display the board from black's perspective
        #[arg(long)]
        flip: bool,
        /// output filename
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
    /// render a given position to png
    Position {
        /// the position as a FEN string
        fen: String,
        /// display the board from black's perspective
        #[arg(long)]
        flip: bool,
        /// output filename
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Game { output, pgn, flip } => render_game(
            &pgn.unwrap_or_else(|| {
                let mut input = String::new();
                stdin().read_to_string(&mut input).unwrap();
                input
            }),
            &output.unwrap_or("game.gif".into()),
            flip,
        )
        .unwrap()
        .unwrap()
        .unwrap(),
        Commands::Position { fen, flip, output } => {
            render_position(&fen, &output.unwrap_or("position.png".into()), flip).unwrap();
        }
    }
}
