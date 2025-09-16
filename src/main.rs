use std::{
    collections::HashMap,
    fs::File,
    io::{Cursor, Read, stdin},
    ops::ControlFlow,
    path::{Path, PathBuf},
    str::FromStr,
    sync::LazyLock,
};

use anyhow::Result;
use clap::{Parser, Subcommand};
use gif::{Encoder, Frame, Repeat};
use image::{ImageReader, Rgba, RgbaImage, imageops};
use itertools::Itertools;
use shakmaty::{
    Board, CastlingMode, Chess, Color, Piece, Position, Role, Square,
    fen::Fen,
    variant::{Variant, VariantPosition},
};

fn square_to_pixels(square: Square) -> (i64, i64) {
    (
        (square.file().to_u32() * 50) as i64,
        ((7 - square.rank().to_u32()) * 50) as i64,
    )
}

fn blank_board() -> RgbaImage {
    RgbaImage::from_par_fn(8 * 50, 8 * 50, |x, y| {
        if ((x / 50) ^ (y / 50)) % 2 == 0 {
            Rgba([0xff, 0xce, 0x9e, 0xFF])
        } else {
            Rgba([0xd1, 0x8b, 0x47, 0xFF])
        }
    })
}

static PIECE_IMAGES: LazyLock<HashMap<Piece, RgbaImage>> = LazyLock::new(|| {
    Role::ALL
        .iter()
        .flat_map(|&role| {
            Color::ALL.map(|color| {
                let piece = Piece { role, color };
                let fname = Path::new(file!())
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("resources")
                    .join(format!("{}.png", piece.char()));
                let img = ImageReader::open(fname).unwrap().decode().unwrap();
                (piece, img.into_rgba8())
            })
        })
        .collect()
});

fn render_position(board: &Board, flip: bool) -> RgbaImage {
    let mut img = blank_board();
    for square in Square::ALL {
        if let Some(piece) = board.piece_at(square) {
            let piece_img = PIECE_IMAGES.get(&piece).unwrap();
            let (mut x, mut y) = square_to_pixels(square);
            if flip {
                x = 350 - x;
                y = 350 - y;
            }
            imageops::overlay(&mut img, piece_img, x, y);
        }
    }

    img
}

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

struct GameRenderer {
    images: Vec<RgbaImage>,
    flip: bool,
}

impl GameRenderer {
    pub fn new(flip: bool) -> Self {
        Self {
            flip,
            images: vec![],
        }
    }

    fn render_board(&mut self, pos: &VariantPosition) {
        let board = pos.board();
        self.images.push(render_position(board, self.flip));
    }
}

impl pgn_reader::Visitor for GameRenderer {
    type Tags = Option<VariantPosition>;

    type Movetext = VariantPosition;

    type Output = Result<()>;

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        ControlFlow::Continue(None)
    }

    fn tag(
        &mut self,
        tags: &mut Self::Tags,
        name: &[u8],
        value: pgn_reader::RawTag<'_>,
    ) -> ControlFlow<Self::Output> {
        if name == b"FEN" {
            let fen = match Fen::from_ascii(value.as_bytes()) {
                Ok(fen) => fen,
                Err(err) => return ControlFlow::Break(Err(err.into())),
            };
            let variant = tags
                .as_ref()
                .unwrap_or(&VariantPosition::Chess(Chess::new()))
                .variant();
            let setup = fen.into_setup();
            let castling_mode = CastlingMode::detect(&setup);
            let pos = match VariantPosition::from_setup(variant, setup, castling_mode) {
                Ok(pos) => pos,
                Err(err) => return ControlFlow::Break(Err(err.into())),
            };
            tags.replace(pos);
        } else if name == b"Variant" {
            match Variant::from_ascii(value.as_bytes()) {
                Ok(variant) => {
                    let curr = tags.clone().unwrap_or(VariantPosition::Chess(Chess::new()));
                    let setup = curr.to_setup(shakmaty::EnPassantMode::PseudoLegal);
                    let castling_mode = CastlingMode::detect(&setup);
                    let pos = match VariantPosition::from_setup(variant, setup, castling_mode) {
                        Ok(pos) => pos,
                        Err(err) => return ControlFlow::Break(Err(err.into())),
                    };
                    tags.replace(pos);
                }
                Err(err) => return ControlFlow::Break(Err(err.into())),
            }
        }
        ControlFlow::Continue(())
    }

    fn begin_movetext(&mut self, tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        let pos = tags.unwrap_or_default();
        self.render_board(&pos);
        ControlFlow::Continue(pos)
    }

    fn san(
        &mut self,
        movetext: &mut Self::Movetext,
        san_plus: pgn_reader::SanPlus,
    ) -> ControlFlow<Self::Output> {
        match san_plus.san.to_move(movetext) {
            Ok(m) => {
                movetext.play_unchecked(m);
                self.render_board(movetext);

                ControlFlow::Continue(())
            }
            Err(err) => ControlFlow::Break(Err(err.into())),
        }
    }

    fn end_game(&mut self, _: Self::Movetext) -> Self::Output {
        Ok(())
    }
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Game { output, pgn, flip } => {
            let mut reader = pgn_reader::Reader::new(Cursor::new(pgn.unwrap_or_else(|| {
                let mut input = String::new();
                stdin().read_to_string(&mut input).unwrap();
                input
            })));
            let mut renderer = GameRenderer::new(flip);
            reader.read_game(&mut renderer).unwrap().unwrap().unwrap();
            let f = File::create(output.unwrap_or("game.gif".into())).unwrap();
            let mut palette = renderer
                .images
                .iter()
                .flat_map(|f| f.pixels().map(|p| [p.0[0], p.0[1], p.0[2]]))
                .unique_by(|p| [p[0] / 32, p[1] / 32, p[2] / 32])
                .collect_vec();
            palette.push([0xff, 0xff, 0xff]);
            let transparent_idx = (palette.len() - 1) as u8;
            let mut enc = Encoder::new(
                f,
                400,
                400,
                &palette.iter().copied().flatten().collect_vec(),
            )
            .unwrap();
            enc.set_repeat(Repeat::Infinite).unwrap();
            let images = renderer.images;
            let last_img_idx = images.len() - 1;
            let indexed_pixels = images
                .into_iter()
                .map(|img| {
                    img.pixels()
                        .map(|p| {
                            palette
                                .iter()
                                .position_min_by_key(|pos| {
                                    (pos[0].abs_diff(p[0]) as usize)
                                        + (pos[1].abs_diff(p[1]) as usize)
                                        + (pos[2].abs_diff(p[2]) as usize)
                                })
                                .unwrap() as u8
                        })
                        .collect_vec()
                })
                .collect_vec();
            indexed_pixels.iter().enumerate().for_each(|(idx, pixels)| {
                let mut frame = Frame::from_indexed_pixels(
                    400,
                    400,
                    {
                        if idx > 0 {
                            let prev_pixels = &indexed_pixels[idx - 1];
                            pixels
                                .iter()
                                .zip(prev_pixels)
                                .map(
                                    |(&curr, &prev)| {
                                        if curr == prev { transparent_idx } else { curr }
                                    },
                                )
                                .collect_vec()
                        } else {
                            pixels.clone()
                        }
                    },
                    Some(transparent_idx),
                );
                frame.palette = None;
                if idx == last_img_idx {
                    frame.delay = 500;
                } else {
                    frame.delay = 62;
                }
                enc.write_frame(&frame).unwrap();
            });
        }
        Commands::Position { fen, flip, output } => {
            let img = render_position(&Board::from_str(&fen).unwrap(), flip);
            img.save_with_format(
                output.unwrap_or("position.png".into()),
                image::ImageFormat::Png,
            )
            .unwrap();
        }
    }
}
