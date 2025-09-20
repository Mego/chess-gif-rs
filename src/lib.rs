use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, Cursor},
    ops::ControlFlow,
    path::Path,
    str::FromStr,
    sync::LazyLock,
};

use anyhow::Result;
use gif::{Encoder, Frame, Repeat};
use image::{ImageReader, Rgba, RgbaImage, imageops};
use itertools::Itertools;
use pgn_reader::Skip;
use shakmaty::{
    Bitboard, Board, CastlingMode, Chess, Piece, Position, Square,
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
    [
        (
            Piece::from_char('p').unwrap(),
            include_bytes!("../resources/p.png").to_vec(),
        ),
        (
            Piece::from_char('r').unwrap(),
            include_bytes!("../resources/r.png").to_vec(),
        ),
        (
            Piece::from_char('n').unwrap(),
            include_bytes!("../resources/n.png").to_vec(),
        ),
        (
            Piece::from_char('b').unwrap(),
            include_bytes!("../resources/b.png").to_vec(),
        ),
        (
            Piece::from_char('q').unwrap(),
            include_bytes!("../resources/q.png").to_vec(),
        ),
        (
            Piece::from_char('k').unwrap(),
            include_bytes!("../resources/k.png").to_vec(),
        ),
        (
            Piece::from_char('P').unwrap(),
            include_bytes!("../resources/P.png").to_vec(),
        ),
        (
            Piece::from_char('R').unwrap(),
            include_bytes!("../resources/R.png").to_vec(),
        ),
        (
            Piece::from_char('N').unwrap(),
            include_bytes!("../resources/N.png").to_vec(),
        ),
        (
            Piece::from_char('B').unwrap(),
            include_bytes!("../resources/B.png").to_vec(),
        ),
        (
            Piece::from_char('Q').unwrap(),
            include_bytes!("../resources/Q.png").to_vec(),
        ),
        (
            Piece::from_char('K').unwrap(),
            include_bytes!("../resources/K.png").to_vec(),
        ),
    ]
    .map(|(p, data)| {
        (
            p,
            ImageReader::with_format(Cursor::new(data), image::ImageFormat::Png)
                .decode()
                .unwrap()
                .into_rgba8(),
        )
    })
    .into()
});

fn render_board(board: &Board, flip: bool) -> RgbaImage {
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

struct GameRenderer {
    last_image: Option<(Vec<u8>, HashSet<usize>)>,
    flip: bool,
    encoder: Encoder<File>,
}

impl GameRenderer {
    const TRANSPARENT_IDX: u8 = 20;
    const PALETTE: [[u8; 3]; 21] = [
        [255, 206, 158],
        [209, 139, 71],
        [159, 129, 99],
        [111, 90, 69],
        [131, 87, 44],
        [91, 61, 31],
        [47, 38, 29],
        [0, 0, 0],
        [79, 64, 49],
        [207, 167, 128],
        [63, 51, 39],
        [39, 26, 13],
        [175, 141, 108],
        [169, 112, 57],
        [143, 116, 89],
        [127, 102, 78],
        [156, 104, 53],
        [118, 78, 40],
        [192, 155, 118],
        [79, 63, 48],
        [255, 255, 255],
    ];

    fn new(flip: bool, output: &Path) -> Self {
        let f = File::create(output).unwrap();
        let mut encoder = Encoder::new(f, 400, 400, Self::PALETTE.as_flattened()).unwrap();
        encoder.set_repeat(Repeat::Infinite).unwrap();
        Self {
            flip,
            last_image: None,
            encoder,
        }
    }

    fn render_frame(&mut self, pos: &VariantPosition) {
        let board = pos.board();
        let img = render_board(board, self.flip);
        let indexed_pixels = img
            .pixels()
            .map(|p| {
                Self::PALETTE
                    .iter()
                    .position_min_by_key(|pos| {
                        (pos[0].abs_diff(p[0]) as usize)
                            + (pos[1].abs_diff(p[1]) as usize)
                            + (pos[2].abs_diff(p[2]) as usize)
                    })
                    .unwrap() as u8
            })
            .collect_vec();
        let transparent_indexes = if let Some((prev_pixels, _)) = &self.last_image {
            indexed_pixels
                .iter()
                .zip(prev_pixels)
                .positions(|(&curr, &prev)| curr == prev)
                .collect()
        } else {
            HashSet::new()
        };
        if let Some((prev_pixels, prev_transparent)) = self
            .last_image
            .replace((indexed_pixels, transparent_indexes))
        {
            let mut frame = Frame::from_indexed_pixels(
                400,
                400,
                prev_pixels
                    .into_iter()
                    .enumerate()
                    .map(|(i, p)| {
                        if prev_transparent.contains(&i) {
                            Self::TRANSPARENT_IDX
                        } else {
                            p
                        }
                    })
                    .collect_vec(),
                Some(Self::TRANSPARENT_IDX),
            );
            frame.palette = None;
            frame.delay = 62;
            self.encoder.write_frame(&frame).unwrap();
        }
    }

    fn render_final_frame(&mut self) {
        if let Some((prev_pixels, prev_transparent)) = self.last_image.take() {
            let mut frame = Frame::from_indexed_pixels(
                400,
                400,
                prev_pixels
                    .into_iter()
                    .enumerate()
                    .map(|(i, p)| {
                        if prev_transparent.contains(&i) {
                            Self::TRANSPARENT_IDX
                        } else {
                            p
                        }
                    })
                    .collect_vec(),
                Some(Self::TRANSPARENT_IDX),
            );
            frame.palette = None;
            frame.delay = 500;
            self.encoder.write_frame(&frame).unwrap();
        }
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
                    let curr = tags.clone().unwrap_or(VariantPosition::new(variant));
                    let mut setup = curr.to_setup(shakmaty::EnPassantMode::PseudoLegal);
                    if variant.uci() == "antichess" {
                        // antichess requires no castling rights at start, but VariantPosition::to_setup fails to set that correctly
                        setup.castling_rights = Bitboard::EMPTY;
                    }
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
        self.render_frame(&pos);
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
                self.render_frame(movetext);

                ControlFlow::Continue(())
            }
            Err(err) => {
                ControlFlow::Break(Err(anyhow::Error::from(err).context(san_plus.to_string())))
            }
        }
    }

    fn end_game(&mut self, _: Self::Movetext) -> Self::Output {
        self.render_final_frame();
        Ok(())
    }

    fn begin_variation(
        &mut self,
        _: &mut Self::Movetext,
    ) -> ControlFlow<Self::Output, pgn_reader::Skip> {
        ControlFlow::Continue(Skip(true))
    }
}

pub fn render_game(pgn: &str, output: &Path, flip: bool) -> io::Result<Option<Result<()>>> {
    let mut reader = pgn_reader::Reader::new(Cursor::new(pgn));
    if !reader.has_more()? {
        Ok(None)
    } else {
        let mut renderer = GameRenderer::new(flip, output);
        reader.read_game(&mut renderer)
    }
}

pub fn render_position(fen: &str, output: &Path, flip: bool) -> Result<(), image::ImageError> {
    let img = render_board(&Board::from_str(fen).unwrap(), flip);
    img.save_with_format(output, image::ImageFormat::Png)
}
